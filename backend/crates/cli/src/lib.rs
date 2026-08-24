mod client;
mod error;

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use notegate_command::ReadInput;
use secrecy::SecretString;
use serde_json::Value;

use client::CommandClient;
pub use error::CliError;

const API_KEY_ENV: &str = "NOTEGATE_API_KEY";
const MAX_INPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "notegate-cli",
    version,
    about = "Path-first NoteGate commands for humans and AI agents",
    long_about = "Call NoteGate's shared Command API without the MCP transport.\n\
                  Set NOTEGATE_API_KEY to an Agent ngk_v2_ API key. The key is never accepted as a command-line argument.",
    after_help = "Examples:\n  \
                  notegate-cli --base-url https://notegate.example me\n  \
                  notegate-cli --base-url https://notegate.example read --input '{\"purpose\":\"list spaces\",\"op\":\"spaces\"}'\n  \
                  notegate-cli read --schema"
)]
pub struct Cli {
    /// NoteGate origin. May also be set with NOTEGATE_BASE_URL.
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
    /// Show the authenticated Agent identity and NoteGate server version.
    Me,
    /// Run one shared read command with the same JSON input as MCP read.
    Read(ReadArgs),
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct ReadArgs {
    /// Inline JSON object matching the shared ReadInput schema.
    #[arg(long, value_name = "JSON")]
    pub input: Option<String>,

    /// Read the JSON object from PATH, or use '-' for stdin.
    #[arg(long, value_name = "PATH")]
    pub input_file: Option<PathBuf>,

    /// Print the machine-readable shared ReadInput JSON Schema and exit.
    #[arg(long)]
    pub schema: bool,
}

pub async fn execute(cli: Cli) -> Result<Value, CliError> {
    let Cli {
        base_url,
        timeout_seconds,
        command,
    } = cli;
    match command {
        Command::Me => {
            command_client(base_url.as_deref(), timeout_seconds)?
                .me()
                .await
        }
        Command::Read(args) if args.schema => {
            serde_json::to_value(schemars::schema_for!(ReadInput)).map_err(|_error| {
                CliError::protocol(
                    "schema_serialization_failed",
                    "could not serialize the shared read schema",
                )
            })
        }
        Command::Read(args) => {
            let input = read_input(args)?;
            command_client(base_url.as_deref(), timeout_seconds)?
                .read(&input)
                .await
        }
    }
}

fn command_client(base_url: Option<&str>, timeout_seconds: u64) -> Result<CommandClient, CliError> {
    let base_url = base_url.ok_or_else(|| {
        CliError::configuration(
            "missing_base_url",
            "set NOTEGATE_BASE_URL or pass --base-url",
        )
    })?;
    let api_key = required_api_key()?;
    CommandClient::new(base_url, api_key, Duration::from_secs(timeout_seconds))
}

fn required_api_key() -> Result<SecretString, CliError> {
    let value = std::env::var_os(API_KEY_ENV).ok_or_else(|| {
        CliError::configuration(
            "missing_api_key",
            "set NOTEGATE_API_KEY to an Agent ngk_v2_ API key",
        )
    })?;
    let value = value.into_string().map_err(|_value: OsString| {
        CliError::configuration("invalid_api_key", "NOTEGATE_API_KEY must be valid UTF-8")
    })?;
    if value.is_empty() || value.trim() != value {
        return Err(CliError::configuration(
            "invalid_api_key",
            "NOTEGATE_API_KEY must be non-empty and contain no surrounding whitespace",
        ));
    }
    Ok(SecretString::from(value))
}

fn read_input(args: ReadArgs) -> Result<Value, CliError> {
    let raw = match (args.input, args.input_file) {
        (Some(input), None) => bounded_string(input)?,
        (None, Some(path)) => read_input_file(&path)?,
        _ => {
            return Err(CliError::invalid_input(
                "missing_read_input",
                "provide exactly one of --input, --input-file, or --schema",
            ));
        }
    };
    let value = serde_json::from_str::<Value>(&raw).map_err(|error| {
        CliError::invalid_input(
            "invalid_json",
            format!("read input is invalid JSON: {error}"),
        )
    })?;
    serde_json::from_value::<ReadInput>(value.clone()).map_err(|error| {
        CliError::invalid_input(
            "invalid_read_input",
            format!("read input does not match the shared schema: {error}"),
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
            "read input exceeded the 1 MiB CLI safety limit",
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
            "read input exceeded the 1 MiB CLI safety limit",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_read_type_rejects_unknown_fields_before_http() {
        let result = read_input(ReadArgs {
            input: Some(r#"{"purpose":"inspect","op":"spaces","unexpected":true}"#.to_owned()),
            input_file: None,
            schema: false,
        });

        assert!(result.is_err());
        if let Err(error) = result {
            assert_eq!(error.exit_code(), error::EXIT_INVALID_INPUT);
            assert_eq!(
                error.body().get("error").and_then(Value::as_str),
                Some("invalid_read_input")
            );
        }
    }

    #[test]
    fn read_schema_is_derived_from_the_shared_contract() {
        let result = serde_json::to_value(schemars::schema_for!(ReadInput));
        assert!(result.is_ok());
        let properties = result
            .ok()
            .and_then(|schema| schema.get("properties").cloned());
        assert!(
            properties
                .as_ref()
                .and_then(|value| value.get("purpose"))
                .is_some()
        );
        assert!(
            properties
                .as_ref()
                .and_then(|value| value.get("op"))
                .is_some()
        );
        assert!(
            properties
                .as_ref()
                .and_then(|value| value.get("target"))
                .is_some()
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
}
