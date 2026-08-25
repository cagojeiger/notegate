use reqwest::Url;
use secrecy::SecretString;
use serde::Deserialize;

use crate::auth::{AUTHGATE_URL_ENV, AuthOverride, DEFAULT_POLL_INTERVAL_SECONDS, DEVICE_GRANT};
use crate::credentials::TokenBundle;
use crate::error::CliError;
use crate::url_policy::{is_origin, uses_secure_or_loopback_transport};

#[derive(Debug)]
pub(super) struct AuthConfiguration {
    pub(super) issuer: Url,
    pub(super) client_id: String,
    pub(super) token_endpoint: Url,
    pub(super) revocation_endpoint: Url,
    pub(super) device_authorization_endpoint: Url,
}

#[derive(Deserialize)]
pub(super) struct AuthorizationServerMetadata {
    issuer: String,
    token_endpoint: String,
    revocation_endpoint: String,
    device_authorization_endpoint: String,
    grant_types_supported: Vec<String>,
    cli_client_id: String,
}

impl AuthorizationServerMetadata {
    pub(super) fn validate(self) -> Result<AuthConfiguration, CliError> {
        if !self
            .grant_types_supported
            .iter()
            .any(|grant| grant == DEVICE_GRANT)
            || !self
                .grant_types_supported
                .iter()
                .any(|grant| grant == "refresh_token")
        {
            return Err(CliError::protocol(
                "unsupported_oauth_metadata",
                "NoteGate OAuth metadata does not advertise Device and refresh grants",
            ));
        }
        validate_client_id(&self.cli_client_id)?;
        let issuer = parse_origin_url(&self.issuer, "OAuth issuer")?;
        let token_endpoint =
            parse_same_origin_endpoint(&self.token_endpoint, &issuer, "OAuth token endpoint")?;
        let revocation_endpoint = parse_same_origin_endpoint(
            &self.revocation_endpoint,
            &issuer,
            "OAuth revocation endpoint",
        )?;
        let device_authorization_endpoint = parse_same_origin_endpoint(
            &self.device_authorization_endpoint,
            &issuer,
            "OAuth Device authorization endpoint",
        )?;
        Ok(AuthConfiguration {
            issuer,
            client_id: self.cli_client_id,
            token_endpoint,
            revocation_endpoint,
            device_authorization_endpoint,
        })
    }
}

pub(super) fn configuration_from_override(
    auth_override: AuthOverride,
) -> Result<AuthConfiguration, CliError> {
    let issuer = parse_origin_url(&auth_override.authgate_url, AUTHGATE_URL_ENV)?;
    let token_endpoint = issuer.join("oauth/token").map_err(|_error| {
        CliError::configuration("invalid_auth_override", "invalid AuthGate override URL")
    })?;
    let revocation_endpoint = issuer.join("oauth/revoke").map_err(|_error| {
        CliError::configuration("invalid_auth_override", "invalid AuthGate override URL")
    })?;
    let device_authorization_endpoint =
        issuer.join("oauth/device/authorize").map_err(|_error| {
            CliError::configuration("invalid_auth_override", "invalid AuthGate override URL")
        })?;
    Ok(AuthConfiguration {
        issuer,
        client_id: auth_override.client_id,
        token_endpoint,
        revocation_endpoint,
        device_authorization_endpoint,
    })
}

pub(super) struct DeviceAuthorization {
    pub(super) device_code: SecretString,
    pub(super) user_code: String,
    pub(super) verification_uri: String,
    pub(super) verification_uri_complete: String,
    pub(super) expires_in: u64,
    pub(super) interval: u64,
}

#[derive(Deserialize)]
pub(super) struct DeviceAuthorizationResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: Option<u64>,
}

impl DeviceAuthorization {
    pub(super) fn validate(
        response: DeviceAuthorizationResponse,
        issuer: &Url,
    ) -> Result<Self, CliError> {
        let interval = response
            .interval
            .unwrap_or(DEFAULT_POLL_INTERVAL_SECONDS)
            .max(1);
        if response.device_code.is_empty()
            || response.device_code.len() > 4096
            || response.user_code.is_empty()
            || response.user_code.len() > 128
            || response.expires_in == 0
        {
            return Err(CliError::protocol(
                "invalid_device_authorization_response",
                "AuthGate returned an invalid Device authorization response",
            ));
        }
        let verification_uri = parse_same_origin_endpoint(
            &response.verification_uri,
            issuer,
            "Device verification URI",
        )?;
        let verification_uri_complete = match response.verification_uri_complete {
            Some(uri) => parse_same_origin_verification_uri(
                &uri,
                issuer,
                "Device complete verification URI",
            )?
            .to_string(),
            None => {
                let mut uri = verification_uri.clone();
                uri.query_pairs_mut()
                    .append_pair("user_code", &response.user_code);
                uri.to_string()
            }
        };
        Ok(Self {
            device_code: SecretString::from(response.device_code),
            user_code: response.user_code,
            verification_uri: verification_uri.to_string(),
            verification_uri_complete,
            expires_in: response.expires_in,
            interval,
        })
    }
}

#[derive(Deserialize)]
pub(super) struct TokenResponse {
    pub(super) access_token: String,
    pub(super) refresh_token: String,
    token_type: String,
    pub(super) expires_in: u64,
}

pub(super) enum TokenRequestResult {
    Success(TokenResponse),
    Error(String),
}

pub(super) fn handle_device_poll_result(
    result: TokenRequestResult,
    interval: &mut u64,
) -> Result<Option<TokenResponse>, CliError> {
    match result {
        TokenRequestResult::Success(token) => validate_token_response(token).map(Some),
        TokenRequestResult::Error(error) if error == "authorization_pending" => Ok(None),
        TokenRequestResult::Error(error) if error == "slow_down" => {
            *interval = interval.checked_add(5).ok_or_else(|| {
                CliError::protocol(
                    "invalid_device_authorization_response",
                    "AuthGate requested an invalid polling interval",
                )
            })?;
            Ok(None)
        }
        TokenRequestResult::Error(error) if error == "access_denied" => Err(CliError::auth(
            "access_denied",
            "the Device authorization request was denied",
        )),
        TokenRequestResult::Error(error) if error == "expired_token" => Err(CliError::auth(
            "device_code_expired",
            "the Device authorization code expired; run auth login again",
        )),
        TokenRequestResult::Error(error) if error == "invalid_grant" => Err(login_required()),
        TokenRequestResult::Error(_error) => Err(CliError::auth(
            "device_authorization_failed",
            "AuthGate could not complete Device authorization",
        )),
    }
}

pub(super) fn validate_token_response(token: TokenResponse) -> Result<TokenResponse, CliError> {
    if token.access_token.is_empty()
        || token.refresh_token.is_empty()
        || !token.token_type.eq_ignore_ascii_case("bearer")
        || token.expires_in == 0
    {
        return Err(CliError::protocol(
            "invalid_token_response",
            "AuthGate returned an invalid token response",
        ));
    }
    Ok(token)
}

pub(super) fn validate_stored_endpoints(bundle: &TokenBundle) -> Result<(), CliError> {
    let issuer = parse_origin_url(&bundle.issuer, "stored OAuth issuer")?;
    parse_same_origin_endpoint(
        &bundle.token_endpoint,
        &issuer,
        "stored OAuth token endpoint",
    )?;
    parse_same_origin_endpoint(
        &bundle.revocation_endpoint,
        &issuer,
        "stored OAuth revocation endpoint",
    )?;
    Ok(())
}

fn parse_origin_url(value: &str, name: &str) -> Result<Url, CliError> {
    let url = Url::parse(value).map_err(|_error| {
        CliError::configuration(
            "invalid_oauth_url",
            format!("{name} must be an absolute HTTPS origin or a loopback HTTP origin"),
        )
    })?;
    if !is_origin(&url) || !uses_secure_or_loopback_transport(&url) {
        return Err(CliError::configuration(
            "invalid_oauth_url",
            format!("{name} must be an absolute HTTPS origin or a loopback HTTP origin"),
        ));
    }
    Ok(url)
}

fn parse_same_origin_endpoint(value: &str, issuer: &Url, name: &str) -> Result<Url, CliError> {
    let url = Url::parse(value).map_err(|_error| {
        CliError::protocol(
            "invalid_oauth_metadata",
            format!("{name} is not a valid URL"),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.has_host()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !uses_secure_or_loopback_transport(&url)
        || url.origin() != issuer.origin()
    {
        return Err(CliError::protocol(
            "invalid_oauth_metadata",
            format!("{name} must use the OAuth issuer origin and a secure transport"),
        ));
    }
    Ok(url)
}

fn parse_same_origin_verification_uri(
    value: &str,
    issuer: &Url,
    name: &str,
) -> Result<Url, CliError> {
    let url = Url::parse(value).map_err(|_error| {
        CliError::protocol(
            "invalid_oauth_metadata",
            format!("{name} is not a valid URL"),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.has_host()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !uses_secure_or_loopback_transport(&url)
        || url.origin() != issuer.origin()
    {
        return Err(CliError::protocol(
            "invalid_oauth_metadata",
            format!("{name} must use the OAuth issuer origin and a secure transport"),
        ));
    }
    Ok(url)
}

pub(super) fn oauth_error_code(body: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct OAuthError {
        error: String,
    }
    serde_json::from_slice::<OAuthError>(body)
        .ok()
        .map(|error| error.error)
        .filter(|error| !error.is_empty() && error.len() <= 128)
}

pub(super) fn map_oauth_error(
    body: &[u8],
    fallback_code: &'static str,
    fallback_message: &'static str,
) -> CliError {
    match oauth_error_code(body).as_deref() {
        Some("access_denied") => CliError::auth("access_denied", fallback_message),
        Some("invalid_grant") => login_required(),
        _ => CliError::auth(fallback_code, fallback_message),
    }
}

pub(super) fn validate_client_id(client_id: &str) -> Result<(), CliError> {
    if client_id.is_empty()
        || client_id.len() > 256
        || client_id.trim() != client_id
        || client_id.chars().any(char::is_control)
    {
        return Err(CliError::configuration(
            "invalid_cli_client_id",
            "the NoteGate CLI OAuth client id is invalid",
        ));
    }
    Ok(())
}

fn login_required() -> CliError {
    CliError::auth(
        "login_required",
        "run notegate-cli auth login, or set NOTEGATE_API_KEY for an Agent command",
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn device_response(verification_uri_complete: Option<&str>) -> DeviceAuthorizationResponse {
        DeviceAuthorizationResponse {
            device_code: "device-secret".to_owned(),
            user_code: "ABC+123".to_owned(),
            verification_uri: "https://auth.example.test/device".to_owned(),
            verification_uri_complete: verification_uri_complete.map(str::to_owned),
            expires_in: 300,
            interval: Some(5),
        }
    }

    #[test]
    fn complete_verification_uri_is_preserved_or_built_with_encoded_user_code() {
        let issuer = Url::parse("https://auth.example.test").expect("valid issuer");
        let supplied = "https://auth.example.test/device?user_code=SUPPLIED";

        let supplied_device =
            DeviceAuthorization::validate(device_response(Some(supplied)), &issuer)
                .expect("same-origin complete URI");
        assert_eq!(supplied_device.verification_uri_complete, supplied);

        let fallback = DeviceAuthorization::validate(device_response(None), &issuer)
            .expect("fallback complete URI");
        assert_eq!(
            fallback.verification_uri_complete,
            "https://auth.example.test/device?user_code=ABC%2B123"
        );
    }

    #[test]
    fn complete_verification_uri_must_stay_on_the_oauth_issuer_origin() {
        let issuer = Url::parse("https://auth.example.test").expect("valid issuer");

        let error = DeviceAuthorization::validate(
            device_response(Some("https://phishing.example/device?user_code=ABC%2B123")),
            &issuer,
        )
        .err()
        .expect("cross-origin complete URI must be rejected");

        assert_eq!(
            error
                .body()
                .get("error")
                .and_then(serde_json::Value::as_str),
            Some("invalid_oauth_metadata")
        );
    }
}
