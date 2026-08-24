use std::time::Duration;

use reqwest::header::{ACCEPT, USER_AGENT};
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::Value;
use url::Url;

use crate::error::CliError;

const COMMAND_PATH: &str = "api/commands/v1/";
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

pub struct CommandClient {
    http: reqwest::Client,
    command_base_url: Url,
    api_key: SecretString,
}

impl CommandClient {
    pub fn new(base_url: &str, api_key: SecretString, timeout: Duration) -> Result<Self, CliError> {
        let command_base_url = command_base_url(base_url)?;
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_error| {
                CliError::configuration(
                    "http_client_initialization_failed",
                    "could not initialize the HTTP client",
                )
            })?;

        Ok(Self {
            http,
            command_base_url,
            api_key,
        })
    }

    pub async fn me(&self) -> Result<Value, CliError> {
        let request = self.http.get(self.endpoint("me")?);
        self.send(request).await
    }

    pub async fn read(&self, input: &Value) -> Result<Value, CliError> {
        let request = self.http.post(self.endpoint("read")?).json(input);
        self.send(request).await
    }

    fn endpoint(&self, name: &str) -> Result<Url, CliError> {
        self.command_base_url.join(name).map_err(|_error| {
            CliError::configuration(
                "invalid_base_url",
                "NOTEGATE_BASE_URL could not be joined with the Command API path",
            )
        })
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Value, CliError> {
        let response = request
            .bearer_auth(self.api_key.expose_secret())
            .header(ACCEPT, "application/json")
            .header(
                USER_AGENT,
                concat!("notegate-cli/", env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await
            .map_err(map_transport_error)?;
        let status = response.status();
        let body = read_bounded(response).await?;
        let value = serde_json::from_slice::<Value>(&body).map_err(|_error| {
            CliError::protocol(
                "invalid_json_response",
                format!("NoteGate returned a non-JSON response with HTTP status {status}"),
            )
        })?;

        if status.is_success() {
            Ok(value)
        } else {
            Err(CliError::server(status, value))
        }
    }
}

fn command_base_url(input: &str) -> Result<Url, CliError> {
    let mut url = Url::parse(input).map_err(|_error| {
        CliError::configuration(
            "invalid_base_url",
            "NOTEGATE_BASE_URL must be an absolute HTTP or HTTPS URL",
        )
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.has_host()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(CliError::configuration(
            "invalid_base_url",
            "NOTEGATE_BASE_URL must contain only an HTTP or HTTPS origin",
        ));
    }
    url.set_path(COMMAND_PATH);
    Ok(url)
}

async fn read_bounded(mut response: reqwest::Response) -> Result<Vec<u8>, CliError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(response_too_large());
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_transport_error)? {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(response_too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn response_too_large() -> CliError {
    CliError::recoverable_protocol(
        "response_too_large",
        "NoteGate response exceeded the 8 MiB CLI safety limit",
        "Reduce limit or max_bytes, or narrow the target, before retrying the command",
    )
}

fn map_transport_error(error: reqwest::Error) -> CliError {
    let message = if error.is_timeout() {
        "NoteGate request timed out"
    } else if error.is_connect() {
        "could not connect to NoteGate"
    } else {
        "NoteGate request failed before a valid JSON response was received"
    };
    CliError::unavailable("request_failed", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_accepts_an_origin_and_rejects_credential_or_path_components() -> Result<(), CliError>
    {
        let url = command_base_url("https://notegate.example")?;
        assert_eq!(url.as_str(), "https://notegate.example/api/commands/v1/");

        for invalid in [
            "file:///tmp/notegate",
            "https://user@example.test",
            "https://example.test/prefix",
            "https://example.test?token=secret",
            "https://example.test/#fragment",
        ] {
            let result = command_base_url(invalid);
            assert!(result.is_err(), "unsafe URL was accepted: {invalid}");
            if let Err(error) = result {
                assert_eq!(error.exit_code(), crate::error::EXIT_INVALID_INPUT);
            }
        }
        Ok(())
    }
}
