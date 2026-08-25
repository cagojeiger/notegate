mod auth;
mod client;
mod credentials;
mod error;
mod url_policy;

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use notegate_command::{ManageInput, ReadInput, WriteInput};
use schemars::JsonSchema;
use secrecy::SecretString;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use client::CommandClient;
pub use error::CliError;
use url_policy::canonical_origin;

use auth::{AuthManager, AuthOverride};

const API_KEY_ENV: &str = "NOTEGATE_API_KEY";
const MAX_INPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "notegate-cli",
    version,
    about = "Path-first NoteGate commands for humans and AI agents",
    long_about = "Call NoteGate's shared Command API without the MCP transport.\n\
                  Use auth login for a User Device login, or set NOTEGATE_API_KEY to an Agent ngk_v2_ API key. The API key is never accepted as a command-line argument; OAuth tokens stay in the OS keychain.",
    after_help = "Examples:\n  \
                  notegate-cli --base-url https://notegate.example me\n  \
                  notegate-cli --base-url https://notegate.example read --input '{\"purpose\":\"list spaces\",\"op\":\"spaces\"}'\n  \
                  notegate-cli write --input '{\"purpose\":\"create note\",\"op\":\"write\",\"target\":\"daily:/note.md\",\"content\":\"hello\",\"create\":true}'\n  \
                  notegate-cli manage --input '{\"purpose\":\"create folder\",\"op\":\"mkdir\",\"target\":\"daily:/notes\",\"parents\":true}'\n  \
                  notegate-cli read --schema"
)]
pub struct Cli {
    /// NoteGate HTTPS origin. Loopback HTTP is allowed for local development.
    /// May also be set with NOTEGATE_BASE_URL.
    #[arg(long, global = true, env = "NOTEGATE_BASE_URL", value_name = "URL")]
    pub base_url: Option<String>,

    /// HTTP request timeout. May also be set with NOTEGATE_TIMEOUT_SECONDS.
    #[arg(
        long,
        global = true,
        env = "NOTEGATE_TIMEOUT_SECONDS",
        default_value_t = 30,
        value_parser = clap::value_parser!(u64).range(1..=300),
        value_name = "SECONDS"
    )]
    pub timeout_seconds: u64,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage the User Device-login credential stored in the OS keychain.
    Auth(AuthArgs),
    /// Show the authenticated identity and NoteGate server version.
    Me,
    /// Run one shared read command with the same JSON input as MCP read.
    Read(CommandInputArgs),
    /// Run one shared write command with the same JSON input as MCP write.
    Write(CommandInputArgs),
    /// Run one shared manage command with the same JSON input as MCP manage.
    Manage(CommandInputArgs),
}

#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Sign in through production AuthGate using RFC 8628 Device Flow.
    Login,
    /// Inspect the local credential without contacting NoteGate or AuthGate.
    Status,
    /// Revoke the refresh token once, then delete the local credential.
    Logout,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct CommandInputArgs {
    /// Inline JSON object matching this command's shared schema.
    #[arg(long, value_name = "JSON")]
    pub input: Option<String>,

    /// Read the command JSON object from PATH, or use '-' for stdin.
    #[arg(long, value_name = "PATH")]
    pub input_file: Option<PathBuf>,

    /// Print this command's machine-readable shared JSON Schema and exit.
    #[arg(long)]
    pub schema: bool,
}

pub async fn execute(cli: Cli) -> Result<Value, CliError> {
    execute_with_events(cli, |_event| Ok(())).await
}

pub async fn execute_with_events(
    cli: Cli,
    mut emit: impl FnMut(&Value) -> Result<(), CliError>,
) -> Result<Value, CliError> {
    let Cli {
        base_url,
        timeout_seconds,
        command,
    } = cli;
    match command {
        Command::Auth(args) => {
            let base_url = required_base_url(base_url.as_deref())?;
            match args.command {
                AuthCommand::Login => {
                    let auth_override = AuthOverride::from_env()?;
                    AuthManager::system(Duration::from_secs(timeout_seconds))?
                        .login(base_url, auth_override, &mut emit)
                        .await
                }
                AuthCommand::Status => match api_key()? {
                    Some(_api_key) => Ok(json!({
                        "authenticated": true,
                        "credential": "agent_api_key",
                        "source": "environment",
                        "base_url": canonical_origin(base_url, "NOTEGATE_BASE_URL")?,
                    })),
                    None => AuthManager::system(Duration::from_secs(timeout_seconds))?
                        .status(base_url, AuthOverride::from_env()?),
                },
                AuthCommand::Logout => {
                    let auth_override = AuthOverride::from_env()?;
                    let api_key_active = std::env::var_os(API_KEY_ENV).is_some();
                    let mut result = AuthManager::system(Duration::from_secs(timeout_seconds))?
                        .logout(base_url, auth_override)
                        .await?;
                    if api_key_active && let Some(object) = result.as_object_mut() {
                        object.insert("agent_api_key_active".to_owned(), Value::Bool(true));
                        object.insert(
                            "hint".to_owned(),
                            Value::String(
                                "NOTEGATE_API_KEY remains active; unset it to stop Agent API-key commands"
                                    .to_owned(),
                            ),
                        );
                    }
                    Ok(result)
                }
            }
        }
        Command::Me => {
            command_client(base_url.as_deref(), timeout_seconds)
                .await?
                .me()
                .await
        }
        Command::Read(args) if args.schema => shared_schema::<ReadInput>("read"),
        Command::Read(args) => {
            let input = command_input::<ReadInput>(
                args,
                "read",
                "missing_read_input",
                "invalid_read_input",
            )?;
            command_client(base_url.as_deref(), timeout_seconds)
                .await?
                .read(&input)
                .await
        }
        Command::Write(args) if args.schema => shared_schema::<WriteInput>("write"),
        Command::Write(args) => {
            let input = command_input::<WriteInput>(
                args,
                "write",
                "missing_write_input",
                "invalid_write_input",
            )?;
            command_client(base_url.as_deref(), timeout_seconds)
                .await?
                .write(&input)
                .await
        }
        Command::Manage(args) if args.schema => shared_schema::<ManageInput>("manage"),
        Command::Manage(args) => {
            let input = command_input::<ManageInput>(
                args,
                "manage",
                "missing_manage_input",
                "invalid_manage_input",
            )?;
            command_client(base_url.as_deref(), timeout_seconds)
                .await?
                .manage(&input)
                .await
        }
    }
}

async fn command_client(
    base_url: Option<&str>,
    timeout_seconds: u64,
) -> Result<CommandClient, CliError> {
    let base_url = required_base_url(base_url)?;
    let bearer = match api_key()? {
        Some(api_key) => api_key,
        None => {
            AuthManager::system(Duration::from_secs(timeout_seconds))?
                .access_token(base_url)
                .await?
        }
    };
    CommandClient::new(base_url, bearer, Duration::from_secs(timeout_seconds))
}

fn required_base_url(base_url: Option<&str>) -> Result<&str, CliError> {
    base_url.ok_or_else(|| {
        CliError::configuration(
            "missing_base_url",
            "set NOTEGATE_BASE_URL or pass --base-url",
        )
    })
}

fn api_key() -> Result<Option<SecretString>, CliError> {
    let Some(value) = std::env::var_os(API_KEY_ENV) else {
        return Ok(None);
    };
    let value = value.into_string().map_err(|_value: OsString| {
        CliError::configuration("invalid_api_key", "NOTEGATE_API_KEY must be valid UTF-8")
    })?;
    if value.is_empty() || value.trim() != value {
        return Err(CliError::configuration(
            "invalid_api_key",
            "NOTEGATE_API_KEY must be non-empty and contain no surrounding whitespace",
        ));
    }
    Ok(Some(SecretString::from(value)))
}

fn shared_schema<T: JsonSchema>(command: &str) -> Result<Value, CliError> {
    serde_json::to_value(schemars::schema_for!(T)).map_err(|_error| {
        CliError::protocol(
            "schema_serialization_failed",
            format!("could not serialize the shared {command} schema"),
        )
    })
}

fn command_input<T: DeserializeOwned>(
    args: CommandInputArgs,
    command: &str,
    missing_code: &'static str,
    invalid_code: &'static str,
) -> Result<Value, CliError> {
    let raw = match (args.input, args.input_file) {
        (Some(input), None) => bounded_string(input)?,
        (None, Some(path)) => read_input_file(&path)?,
        _ => {
            return Err(CliError::invalid_input(
                missing_code,
                "provide exactly one of --input, --input-file, or --schema",
            ));
        }
    };
    let value = serde_json::from_str::<Value>(&raw).map_err(|error| {
        CliError::invalid_input(
            "invalid_json",
            format!("{command} input is invalid JSON: {error}"),
        )
    })?;
    serde_json::from_value::<T>(value.clone()).map_err(|error| {
        CliError::invalid_input(
            invalid_code,
            format!("{command} input does not match the shared schema: {error}"),
        )
    })?;
    Ok(value)
}

fn read_input_file(path: &Path) -> Result<String, CliError> {
    if path == Path::new("-") {
        let stdin = io::stdin();
        return read_bounded(stdin.lock(), "stdin");
    }
    let file = File::open(path).map_err(|error| {
        CliError::invalid_input(
            "input_read_failed",
            format!("could not open {}: {error}", path.display()),
        )
    })?;
    read_bounded(file, &path.display().to_string())
}

fn read_bounded(reader: impl io::Read, source: &str) -> Result<String, CliError> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            CliError::invalid_input(
                "input_read_failed",
                format!("could not read {source}: {error}"),
            )
        })?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(CliError::invalid_input(
            "input_too_large",
            "command input exceeded the 1 MiB CLI safety limit",
        ));
    }
    String::from_utf8(bytes).map_err(|_error| {
        CliError::invalid_input("invalid_utf8", format!("{source} is not valid UTF-8"))
    })
}

fn bounded_string(value: String) -> Result<String, CliError> {
    if value.len() > MAX_INPUT_BYTES {
        return Err(CliError::invalid_input(
            "input_too_large",
            "command input exceeded the 1 MiB CLI safety limit",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_command_types_reject_unknown_fields_before_http() {
        let cases = [
            (
                command_input::<ReadInput>(
                    inline_input(r#"{"purpose":"inspect","op":"spaces","unexpected":true}"#),
                    "read",
                    "missing_read_input",
                    "invalid_read_input",
                ),
                "invalid_read_input",
            ),
            (
                command_input::<WriteInput>(
                    inline_input(
                        r#"{"purpose":"write","op":"write","target":"daily:/note.md","content":"body","unexpected":true}"#,
                    ),
                    "write",
                    "missing_write_input",
                    "invalid_write_input",
                ),
                "invalid_write_input",
            ),
            (
                command_input::<ManageInput>(
                    inline_input(
                        r#"{"purpose":"mkdir","op":"mkdir","target":"daily:/notes","unexpected":true}"#,
                    ),
                    "manage",
                    "missing_manage_input",
                    "invalid_manage_input",
                ),
                "invalid_manage_input",
            ),
        ];

        for (result, expected_code) in cases {
            assert!(result.is_err());
            if let Err(error) = result {
                assert_eq!(error.exit_code(), error::EXIT_INVALID_INPUT);
                assert_eq!(
                    error.body().get("error").and_then(Value::as_str),
                    Some(expected_code)
                );
            }
        }
    }

    #[test]
    fn schemas_are_derived_from_the_shared_command_contracts() {
        assert_schema_properties::<ReadInput>("read", &["purpose", "op", "target"]);
        assert_schema_properties::<WriteInput>(
            "write",
            &["purpose", "op", "target", "content", "edits"],
        );
        assert_schema_properties::<ManageInput>(
            "manage",
            &["purpose", "op", "target", "source", "destination"],
        );
    }

    #[test]
    fn bounded_reader_rejects_oversized_and_non_utf8_input() {
        let oversized = read_bounded(
            io::Cursor::new(vec![b'x'; MAX_INPUT_BYTES + 1]),
            "test input",
        );
        assert_eq!(
            oversized
                .err()
                .and_then(|error| error.body().get("error").cloned()),
            Some(Value::String("input_too_large".to_owned()))
        );

        let non_utf8 = read_bounded(io::Cursor::new(vec![0xff]), "test input");
        assert_eq!(
            non_utf8
                .err()
                .and_then(|error| error.body().get("error").cloned()),
            Some(Value::String("invalid_utf8".to_owned()))
        );
    }

    fn inline_input(input: &str) -> CommandInputArgs {
        CommandInputArgs {
            input: Some(input.to_owned()),
            input_file: None,
            schema: false,
        }
    }

    fn assert_schema_properties<T: JsonSchema>(command: &str, expected: &[&str]) {
        let result = shared_schema::<T>(command);
        assert!(result.is_ok());
        if let Ok(schema) = result {
            for property in expected {
                assert!(
                    schema
                        .get("properties")
                        .and_then(|properties| properties.get(property))
                        .is_some(),
                    "missing {command} schema property: {property}"
                );
            }
        }
    }
}
