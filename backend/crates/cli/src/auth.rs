use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use reqwest::Url;
use reqwest::header::{ACCEPT, USER_AGENT};
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::{Value, json};
use tokio::time::sleep;

use crate::credentials::{
    CredentialKey, CredentialStore, KeyringCredentialStore, RefreshState, StoreError, TokenBundle,
    key_digest, lock_file,
};
use crate::error::CliError;
use crate::url_policy::{canonical_origin, invalid_base_origin};

mod oauth;

use oauth::{
    AuthConfiguration, AuthorizationServerMetadata, DeviceAuthorization,
    DeviceAuthorizationResponse, TokenRequestResult, TokenResponse, configuration_from_override,
    handle_device_poll_result, map_oauth_error, oauth_error_code, validate_client_id,
    validate_stored_endpoints, validate_token_response,
};

pub(crate) const AUTHGATE_URL_ENV: &str = "NOTEGATE_AUTHGATE_URL";
pub(crate) const CLI_CLIENT_ID_ENV: &str = "NOTEGATE_CLI_CLIENT_ID";

pub(super) const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEVICE_SCOPE: &str = "openid profile email offline_access";
const METADATA_PATH: &str = "/.well-known/oauth-authorization-server";
const MAX_AUTH_RESPONSE_BYTES: usize = 64 * 1024;
pub(super) const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 5;
const REFRESH_EARLY_SECONDS: u64 = 60;

pub(crate) struct AuthManager {
    http: reqwest::Client,
    store: Arc<dyn CredentialStore>,
    lock_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct AuthOverride {
    authgate_url: String,
    client_id: String,
}

impl AuthOverride {
    pub(crate) fn from_env() -> Result<Option<Self>, CliError> {
        Self::from_values(
            std::env::var(AUTHGATE_URL_ENV).ok(),
            std::env::var(CLI_CLIENT_ID_ENV).ok(),
        )
    }

    fn from_values(
        authgate_url: Option<String>,
        client_id: Option<String>,
    ) -> Result<Option<Self>, CliError> {
        match (authgate_url, client_id) {
            (None, None) => Ok(None),
            (Some(authgate_url), Some(client_id)) => {
                validate_client_id(&client_id)?;
                Ok(Some(Self {
                    authgate_url,
                    client_id,
                }))
            }
            _ => Err(CliError::configuration(
                "incomplete_auth_override",
                "NOTEGATE_AUTHGATE_URL and NOTEGATE_CLI_CLIENT_ID must be set together",
            )),
        }
    }
}

impl AuthManager {
    pub(crate) fn system(timeout: Duration) -> Result<Self, CliError> {
        let store = Arc::new(KeyringCredentialStore::new().map_err(map_store_error)?);
        let project_dirs = ProjectDirs::from("io", "project-jelly", "notegate-cli")
            .ok_or_else(credential_store_unavailable)?;
        Self::new(
            timeout,
            store,
            project_dirs.data_local_dir().join("refresh-locks"),
        )
    }

    pub(crate) fn new(
        timeout: Duration,
        store: Arc<dyn CredentialStore>,
        lock_dir: PathBuf,
    ) -> Result<Self, CliError> {
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
            store,
            lock_dir,
        })
    }

    pub(crate) async fn login(
        &self,
        base_url: &str,
        auth_override: Option<AuthOverride>,
        mut emit: impl FnMut(&Value) -> Result<(), CliError>,
    ) -> Result<Value, CliError> {
        let base_url = canonical_origin(base_url, "NOTEGATE_BASE_URL")?;
        let configuration = match auth_override.as_ref() {
            Some(auth_override) => configuration_from_override(auth_override.clone())?,
            None => self.discover(&base_url).await?,
        };
        let key = CredentialKey {
            issuer: configuration
                .issuer
                .to_string()
                .trim_end_matches('/')
                .to_owned(),
            client_id: configuration.client_id.clone(),
        };
        let lock_path = self.lock_dir.join(format!("{}.lock", key_digest(&key)));
        let mut lock = lock_file(&lock_path).map_err(map_store_error)?;
        let _guard = match lock.try_write() {
            Ok(guard) => guard,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(login_in_progress());
            }
            Err(_error) => return Err(credential_store_unavailable()),
        };
        let existing = self.store.load(&key).map_err(map_store_error)?;
        let has_uncertain_marker = self.has_uncertain_marker(&key)?;
        if let Some(existing) = existing {
            if existing.base_url != base_url {
                return Err(CliError::configuration(
                    "credential_base_url_mismatch",
                    "the selected OAuth credential belongs to a different NoteGate base URL",
                ));
            }
            if existing.refresh_state == RefreshState::Ready && !has_uncertain_marker {
                return Err(already_authenticated());
            }
            // An uncertain refresh token must never be submitted again. Removing
            // it under the same issuer+client lock makes a new Device Flow the
            // only recovery path.
            self.store.delete(&key).map_err(map_store_error)?;
            self.clear_uncertain_marker(&key)?;
        } else if has_uncertain_marker {
            // The rotated credential may already have been deleted after a
            // persistence failure. No reusable secret remains locally.
            self.clear_uncertain_marker(&key)?;
        }
        if self
            .store
            .load_for_base_url(&base_url)
            .map_err(map_store_error)?
            .is_some()
        {
            return Err(already_authenticated());
        }
        let device = self.request_device_authorization(&configuration).await?;
        emit(&json!({
            "event": "verification_required",
            "verification_uri": device.verification_uri,
            "verification_uri_complete": device.verification_uri_complete,
            "user_code": device.user_code,
            "expires_in": device.expires_in,
            "interval": device.interval,
        }))?;

        let token = self.poll_device(&configuration, &device).await?;
        let expires_at = expires_at(token.expires_in)?;
        let bundle = TokenBundle::new(
            base_url,
            key.issuer,
            key.client_id,
            configuration.token_endpoint.to_string(),
            configuration.revocation_endpoint.to_string(),
            token.access_token,
            token.refresh_token,
            expires_at,
        );
        if let Err(store_error) = self.store.create(&bundle) {
            let _revoke_result = self.revoke_once(&bundle).await;
            return Err(map_store_error(store_error));
        }
        self.clear_uncertain_marker(&bundle.key())?;

        Ok(json!({
            "event": "login_succeeded",
            "base_url": bundle.base_url,
            "issuer": bundle.issuer,
            "client_id": bundle.client_id,
            "expires_at": bundle.expires_at,
        }))
    }

    pub(crate) fn status(
        &self,
        base_url: &str,
        auth_override: Option<AuthOverride>,
    ) -> Result<Value, CliError> {
        let base_url = canonical_origin(base_url, "NOTEGATE_BASE_URL")?;
        let bundle = self.load_selected(&base_url, auth_override.as_ref())?;
        let Some(bundle) = bundle else {
            return Ok(json!({
                "authenticated": false,
                "base_url": base_url,
            }));
        };
        let now = unix_time()?;
        let refresh_uncertain = bundle.refresh_state == RefreshState::Uncertain
            || self.has_uncertain_marker(&bundle.key())?;
        Ok(json!({
            "authenticated": !refresh_uncertain,
            "credential": "oauth_device",
            "base_url": bundle.base_url,
            "issuer": bundle.issuer,
            "client_id": bundle.client_id,
            "expires_at": bundle.expires_at,
            "access_token_expired": bundle.expires_at <= now,
            "refresh_state": bundle.refresh_state,
            "needs_login": refresh_uncertain,
        }))
    }

    pub(crate) async fn logout(
        &self,
        base_url: &str,
        auth_override: Option<AuthOverride>,
    ) -> Result<Value, CliError> {
        let base_url = canonical_origin(base_url, "NOTEGATE_BASE_URL")?;
        let Some(bundle) = self.load_selected(&base_url, auth_override.as_ref())? else {
            return Ok(json!({
                "logged_out": true,
                "base_url": base_url,
                "revocation_attempted": false,
            }));
        };
        let key = bundle.key();
        let revoke_result = self.revoke_once(&bundle).await;
        let delete_result = self.store.delete(&key).map_err(map_store_error);
        delete_result?;
        self.clear_uncertain_marker(&key)?;
        revoke_result?;
        Ok(json!({
            "logged_out": true,
            "base_url": base_url,
            "revocation_attempted": true,
        }))
    }

    pub(crate) async fn access_token(&self, base_url: &str) -> Result<SecretString, CliError> {
        let base_url = canonical_origin(base_url, "NOTEGATE_BASE_URL")?;
        let Some(bundle) = self
            .store
            .load_for_base_url(&base_url)
            .map_err(map_store_error)?
        else {
            return Err(login_required());
        };
        if bundle.refresh_state == RefreshState::Uncertain {
            return Err(refresh_uncertain());
        }
        if !needs_refresh(&bundle, unix_time()?) {
            if self.has_uncertain_marker(&bundle.key())? {
                return Err(refresh_uncertain());
            }
            return Ok(bundle.access_token());
        }
        self.refresh_locked(bundle).await
    }

    fn load_selected(
        &self,
        base_url: &str,
        auth_override: Option<&AuthOverride>,
    ) -> Result<Option<TokenBundle>, CliError> {
        let bundle = match auth_override {
            Some(auth_override) => {
                let issuer = canonical_origin(&auth_override.authgate_url, AUTHGATE_URL_ENV)?;
                let key = CredentialKey {
                    issuer,
                    client_id: auth_override.client_id.clone(),
                };
                self.store.load(&key).map_err(map_store_error)?
            }
            None => self
                .store
                .load_for_base_url(base_url)
                .map_err(map_store_error)?,
        };
        if bundle
            .as_ref()
            .is_some_and(|bundle| bundle.base_url != base_url)
        {
            return Err(CliError::configuration(
                "credential_base_url_mismatch",
                "the selected OAuth credential belongs to a different NoteGate base URL",
            ));
        }
        Ok(bundle)
    }

    async fn discover(&self, base_url: &str) -> Result<AuthConfiguration, CliError> {
        let base =
            Url::parse(base_url).map_err(|_error| invalid_base_origin("NOTEGATE_BASE_URL"))?;
        let metadata_url = base
            .join(METADATA_PATH)
            .map_err(|_error| invalid_base_origin("NOTEGATE_BASE_URL"))?;
        let response = self
            .http
            .get(metadata_url)
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, user_agent())
            .send()
            .await
            .map_err(|_error| {
                CliError::unavailable(
                    "oauth_metadata_request_failed",
                    "could not retrieve NoteGate OAuth metadata",
                )
            })?;
        let status = response.status();
        let body = read_bounded(response).await?;
        if !status.is_success() {
            return Err(CliError::protocol(
                "oauth_metadata_rejected",
                format!("NoteGate OAuth metadata returned HTTP status {status}"),
            ));
        }
        let metadata =
            serde_json::from_slice::<AuthorizationServerMetadata>(&body).map_err(|_error| {
                CliError::protocol(
                    "invalid_oauth_metadata",
                    "NoteGate returned invalid OAuth authorization-server metadata",
                )
            })?;
        metadata.validate()
    }

    async fn request_device_authorization(
        &self,
        configuration: &AuthConfiguration,
    ) -> Result<DeviceAuthorization, CliError> {
        let response = self
            .http
            .post(configuration.device_authorization_endpoint.clone())
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, user_agent())
            .form(&[
                ("client_id", configuration.client_id.as_str()),
                ("scope", DEVICE_SCOPE),
            ])
            .send()
            .await
            .map_err(|_error| {
                CliError::unavailable(
                    "device_authorization_request_failed",
                    "could not start AuthGate Device authorization",
                )
            })?;
        let status = response.status();
        let body = read_bounded(response).await?;
        if !status.is_success() {
            return Err(map_oauth_error(
                &body,
                "device_authorization_rejected",
                "AuthGate rejected the Device authorization request",
            ));
        }
        let response =
            serde_json::from_slice::<DeviceAuthorizationResponse>(&body).map_err(|_error| {
                CliError::protocol(
                    "invalid_device_authorization_response",
                    "AuthGate returned an invalid Device authorization response",
                )
            })?;
        DeviceAuthorization::validate(response, &configuration.issuer)
    }

    async fn poll_device(
        &self,
        configuration: &AuthConfiguration,
        device: &DeviceAuthorization,
    ) -> Result<TokenResponse, CliError> {
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(device.expires_in))
            .ok_or_else(|| {
                CliError::protocol(
                    "invalid_device_authorization_response",
                    "AuthGate returned an invalid Device authorization expiry",
                )
            })?;
        let mut interval = device.interval;
        loop {
            if Instant::now()
                .checked_add(Duration::from_secs(interval))
                .is_none_or(|next| next >= deadline)
            {
                return Err(CliError::auth(
                    "device_code_expired",
                    "the Device authorization code expired; run auth login again",
                ));
            }
            sleep(Duration::from_secs(interval)).await;
            let result = self
                .token_request(
                    &configuration.token_endpoint,
                    &[
                        ("grant_type", DEVICE_GRANT),
                        ("device_code", device.device_code.expose_secret()),
                        ("client_id", configuration.client_id.as_str()),
                    ],
                )
                .await?;
            if let Some(token) = handle_device_poll_result(result, &mut interval)? {
                return Ok(token);
            }
        }
    }

    async fn token_request(
        &self,
        endpoint: &Url,
        form: &[(&str, &str)],
    ) -> Result<TokenRequestResult, CliError> {
        let response = self
            .http
            .post(endpoint.clone())
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, user_agent())
            .form(form)
            .send()
            .await
            .map_err(|_error| {
                CliError::unavailable(
                    "token_request_failed",
                    "AuthGate token request failed before a response was received",
                )
            })?;
        let status = response.status();
        let body = read_bounded(response).await?;
        if status.is_success() {
            serde_json::from_slice::<TokenResponse>(&body)
                .map(TokenRequestResult::Success)
                .map_err(|_error| {
                    CliError::protocol(
                        "invalid_token_response",
                        "AuthGate returned an invalid token response",
                    )
                })
        } else {
            let error = oauth_error_code(&body).ok_or_else(|| {
                CliError::protocol(
                    "invalid_oauth_error",
                    format!("AuthGate returned an invalid OAuth error with HTTP status {status}"),
                )
            })?;
            Ok(TokenRequestResult::Error(error))
        }
    }

    async fn refresh_locked(&self, original: TokenBundle) -> Result<SecretString, CliError> {
        let key = original.key();
        let lock_path = self.lock_dir.join(format!("{}.lock", key_digest(&key)));
        let mut lock = lock_file(&lock_path).map_err(map_store_error)?;
        let _guard = loop {
            match lock.try_write() {
                Ok(guard) => break guard,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    sleep(Duration::from_millis(25)).await;
                }
                Err(_error) => return Err(credential_store_unavailable()),
            }
        };

        let Some(mut current) = self.store.load(&key).map_err(map_store_error)? else {
            return Err(login_required());
        };
        if current.refresh_state == RefreshState::Uncertain || self.has_uncertain_marker(&key)? {
            return Err(refresh_uncertain());
        }
        if !needs_refresh(&current, unix_time()?) {
            return Ok(current.access_token());
        }
        validate_stored_endpoints(&current)?;
        let endpoint = Url::parse(&current.token_endpoint).map_err(|_error| {
            CliError::configuration(
                "invalid_stored_credential",
                "the stored OAuth credential has an invalid token endpoint",
            )
        })?;
        // Write-ahead marker: if this process exits after AuthGate consumes the
        // refresh token but before the rotated bundle is saved, the next process
        // must not submit the old refresh token again.
        self.write_uncertain_marker(&key)?;
        let response = self
            .http
            .post(endpoint)
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, user_agent())
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", current.refresh_token()),
                ("client_id", current.client_id.as_str()),
            ])
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) if error.is_connect() => {
                self.clear_uncertain_marker(&key)?;
                return Err(CliError::unavailable(
                    "refresh_request_failed",
                    "could not connect to AuthGate to refresh the access token",
                ));
            }
            Err(_error) => return self.mark_ambiguous_refresh(current),
        };
        let status = response.status();
        let body = match read_bounded_raw(response).await {
            Ok(body) => body,
            Err(()) => return self.mark_ambiguous_refresh(current),
        };
        if !status.is_success() {
            let Some(error) = oauth_error_code(&body) else {
                return self.mark_ambiguous_refresh(current);
            };
            if error == "invalid_grant" {
                self.store.delete(&key).map_err(map_store_error)?;
                self.clear_uncertain_marker(&key)?;
                return Err(login_required());
            }
            self.clear_uncertain_marker(&key)?;
            return Err(CliError::unavailable(
                "refresh_rejected",
                "AuthGate rejected the token refresh; the request was not retried",
            ));
        }
        let token = match serde_json::from_slice::<TokenResponse>(&body) {
            Ok(token) => match validate_token_response(token) {
                Ok(token) => token,
                Err(_error) => return self.mark_ambiguous_refresh(current),
            },
            Err(_error) => return self.mark_ambiguous_refresh(current),
        };
        let expires_at = expires_at(token.expires_in)?;
        current.replace_tokens(token.access_token, token.refresh_token, expires_at);
        let access_token = current.access_token();
        if self.store.replace(&current).is_err() {
            let marker_result = self.write_uncertain_marker(&key);
            let delete_result = self.store.delete(&key);
            if marker_result.is_err() && delete_result.is_err() {
                return Err(credential_store_unavailable());
            }
            return Err(CliError::auth(
                "credential_persistence_failed",
                "AuthGate rotated the credential, but it could not be saved safely; run auth login again",
            ));
        }
        self.clear_uncertain_marker(&key)?;
        Ok(access_token)
    }

    fn mark_ambiguous_refresh(&self, mut bundle: TokenBundle) -> Result<SecretString, CliError> {
        let key = bundle.key();
        let marker_result = self.write_uncertain_marker(&key);
        bundle.mark_refresh_uncertain();
        let save_result = self.store.replace(&bundle);
        if marker_result.is_err() && save_result.is_err() {
            self.store.delete(&key).map_err(map_store_error)?;
        }
        Err(refresh_uncertain())
    }

    fn uncertain_marker_path(&self, key: &CredentialKey) -> PathBuf {
        self.lock_dir
            .join(format!("{}.refresh-uncertain", key_digest(key)))
    }

    fn has_uncertain_marker(&self, key: &CredentialKey) -> Result<bool, CliError> {
        match fs::metadata(self.uncertain_marker_path(key)) {
            Ok(_metadata) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_error) => Err(credential_store_unavailable()),
        }
    }

    fn write_uncertain_marker(&self, key: &CredentialKey) -> Result<(), CliError> {
        fs::create_dir_all(&self.lock_dir).map_err(|_error| credential_store_unavailable())?;
        let path = self.uncertain_marker_path(key);
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options
            .open(path)
            .map_err(|_error| credential_store_unavailable())?;
        file.write_all(b"refresh outcome unknown\n")
            .and_then(|()| file.sync_all())
            .map_err(|_error| credential_store_unavailable())?;
        sync_directory(&self.lock_dir)
    }

    fn clear_uncertain_marker(&self, key: &CredentialKey) -> Result<(), CliError> {
        match fs::remove_file(self.uncertain_marker_path(key)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_error) => Err(credential_store_unavailable()),
        }
    }

    async fn revoke_once(&self, bundle: &TokenBundle) -> Result<(), CliError> {
        validate_stored_endpoints(bundle)?;
        let endpoint = Url::parse(&bundle.revocation_endpoint).map_err(|_error| {
            CliError::configuration(
                "invalid_stored_credential",
                "the stored OAuth credential has an invalid revocation endpoint",
            )
        })?;
        let response = self
            .http
            .post(endpoint)
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, user_agent())
            .form(&[
                ("token", bundle.refresh_token()),
                ("token_type_hint", "refresh_token"),
                ("client_id", bundle.client_id.as_str()),
            ])
            .send()
            .await
            .map_err(|_error| revocation_failed())?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(revocation_failed())
        }
    }
}

async fn read_bounded(response: reqwest::Response) -> Result<Vec<u8>, CliError> {
    read_bounded_raw(response).await.map_err(|()| {
        CliError::protocol(
            "oauth_response_too_large",
            "OAuth response exceeded the 64 KiB safety limit or could not be read",
        )
    })
}

async fn read_bounded_raw(mut response: reqwest::Response) -> Result<Vec<u8>, ()> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_AUTH_RESPONSE_BYTES as u64)
    {
        return Err(());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_error| ())? {
        if body.len().saturating_add(chunk.len()) > MAX_AUTH_RESPONSE_BYTES {
            return Err(());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn expires_at(expires_in: u64) -> Result<u64, CliError> {
    unix_time()?.checked_add(expires_in).ok_or_else(|| {
        CliError::protocol(
            "invalid_token_response",
            "AuthGate returned an invalid token expiry",
        )
    })
}

fn unix_time() -> Result<u64, CliError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_error| {
            CliError::configuration(
                "invalid_system_clock",
                "the system clock is before the Unix epoch",
            )
        })
}

fn needs_refresh(bundle: &TokenBundle, now: u64) -> bool {
    bundle.expires_at <= now.saturating_add(REFRESH_EARLY_SECONDS)
}

fn map_store_error(error: StoreError) -> CliError {
    if error.is_state_unknown() {
        CliError::configuration(
            "credential_store_state_unknown",
            "the keychain write could not be rolled back after the profile index failed; use the explicit AuthGate URL and client ID overrides with auth logout before retrying",
        )
    } else if error.is_already_exists() {
        already_authenticated()
    } else if error.is_missing() {
        login_required()
    } else if error.is_corrupt() {
        CliError::configuration(
            "invalid_stored_credential",
            "the stored NoteGate OAuth credential is invalid; run auth logout, then auth login",
        )
    } else {
        credential_store_unavailable()
    }
}

fn already_authenticated() -> CliError {
    CliError::auth(
        "already_authenticated",
        "a User credential already exists for this NoteGate URL; run auth logout before auth login",
    )
}

fn login_in_progress() -> CliError {
    CliError::retryable_auth(
        "login_in_progress",
        "another authentication operation is in progress; wait for it to finish, then run auth status",
    )
}

fn credential_store_unavailable() -> CliError {
    CliError::configuration(
        "credential_store_unavailable",
        "the operating-system credential store is unavailable",
    )
}

fn login_required() -> CliError {
    CliError::auth(
        "login_required",
        "run notegate-cli auth login, or set NOTEGATE_API_KEY for an Agent command",
    )
}

fn refresh_uncertain() -> CliError {
    CliError::auth(
        "refresh_outcome_unknown",
        "token refresh may have rotated the credential; run auth login before retrying",
    )
}

fn revocation_failed() -> CliError {
    CliError::unavailable(
        "revocation_failed",
        "the local credential was deleted, but AuthGate token revocation could not be confirmed",
    )
}

fn user_agent() -> &'static str {
    concat!("notegate-cli/", env!("CARGO_PKG_VERSION"))
}

#[cfg(unix)]
fn sync_directory(path: &std::path::Path) -> Result<(), CliError> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_error| credential_store_unavailable())
}

#[cfg(not(unix))]
fn sync_directory(_path: &std::path::Path) -> Result<(), CliError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        clippy::unwrap_in_result
    )]

    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use axum::extract::Form;
    use axum::http::StatusCode;
    use axum::response::IntoResponse as _;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use tokio::net::TcpListener;
    use tokio::sync::Notify;

    use super::*;

    #[derive(Default)]
    struct MemoryStore {
        bundles: Mutex<HashMap<CredentialKey, TokenBundle>>,
    }

    impl CredentialStore for MemoryStore {
        fn load(&self, key: &CredentialKey) -> Result<Option<TokenBundle>, StoreError> {
            Ok(self.bundles.lock().unwrap().get(key).cloned())
        }

        fn load_for_base_url(&self, base_url: &str) -> Result<Option<TokenBundle>, StoreError> {
            Ok(self
                .bundles
                .lock()
                .unwrap()
                .values()
                .find(|bundle| bundle.base_url == base_url)
                .cloned())
        }

        fn create(&self, bundle: &TokenBundle) -> Result<(), StoreError> {
            let mut bundles = self.bundles.lock().unwrap();
            if bundles.contains_key(&bundle.key())
                || bundles
                    .values()
                    .any(|existing| existing.base_url == bundle.base_url)
            {
                return Err(StoreError::already_exists());
            }
            bundles.insert(bundle.key(), bundle.clone());
            Ok(())
        }

        fn replace(&self, bundle: &TokenBundle) -> Result<(), StoreError> {
            let mut bundles = self.bundles.lock().unwrap();
            let Some(existing) = bundles.get(&bundle.key()) else {
                return Err(StoreError::missing());
            };
            if existing.base_url != bundle.base_url {
                return Err(StoreError::corrupt());
            }
            bundles.insert(bundle.key(), bundle.clone());
            Ok(())
        }

        fn delete(&self, key: &CredentialKey) -> Result<(), StoreError> {
            self.bundles.lock().unwrap().remove(key);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailingSaveStore {
        inner: MemoryStore,
        fail_saves: AtomicBool,
    }

    impl CredentialStore for FailingSaveStore {
        fn load(&self, key: &CredentialKey) -> Result<Option<TokenBundle>, StoreError> {
            self.inner.load(key)
        }

        fn load_for_base_url(&self, base_url: &str) -> Result<Option<TokenBundle>, StoreError> {
            self.inner.load_for_base_url(base_url)
        }

        fn create(&self, bundle: &TokenBundle) -> Result<(), StoreError> {
            self.inner.create(bundle)
        }

        fn replace(&self, bundle: &TokenBundle) -> Result<(), StoreError> {
            if self.fail_saves.load(Ordering::SeqCst) {
                return Err(StoreError::unavailable());
            }
            self.inner.replace(bundle)
        }

        fn delete(&self, key: &CredentialKey) -> Result<(), StoreError> {
            self.inner.delete(key)
        }
    }

    #[test]
    fn auth_overrides_are_all_or_nothing() {
        assert!(AuthOverride::from_values(None, None).unwrap().is_none());
        assert!(
            AuthOverride::from_values(
                Some("https://auth.example.test".to_owned()),
                Some("notegate-cli".to_owned())
            )
            .unwrap()
            .is_some()
        );
        for values in [
            (Some("https://auth.example.test".to_owned()), None),
            (None, Some("notegate-cli".to_owned())),
        ] {
            assert_eq!(
                AuthOverride::from_values(values.0, values.1)
                    .unwrap_err()
                    .body()
                    .get("error")
                    .and_then(Value::as_str),
                Some("incomplete_auth_override")
            );
        }
    }

    #[test]
    fn device_polling_handles_pending_slow_down_and_terminal_errors() {
        let mut interval = 5;
        assert!(
            handle_device_poll_result(
                TokenRequestResult::Error("authorization_pending".to_owned()),
                &mut interval,
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(interval, 5);
        assert!(
            handle_device_poll_result(
                TokenRequestResult::Error("slow_down".to_owned()),
                &mut interval,
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(interval, 10);

        for (oauth_error, cli_error) in [
            ("access_denied", "access_denied"),
            ("expired_token", "device_code_expired"),
            ("invalid_grant", "login_required"),
        ] {
            let result = handle_device_poll_result(
                TokenRequestResult::Error(oauth_error.to_owned()),
                &mut interval,
            );
            let error = match result {
                Err(error) => error,
                Ok(_token) => panic!("{oauth_error} must end Device polling"),
            };
            assert_eq!(
                error.body().get("error").and_then(Value::as_str),
                Some(cli_error)
            );
        }
    }

    #[tokio::test]
    async fn login_discovers_client_and_emits_only_safe_ndjson_fields() {
        let token_hits = Arc::new(AtomicUsize::new(0));
        let hits = Arc::clone(&token_hits);
        let base = spawn_with_builder(move |base| {
            let metadata_base = base.clone();
            Router::new()
                .route(
                    METADATA_PATH,
                    get(move || {
                        let metadata_base = metadata_base.clone();
                        async move {
                            Json(json!({
                                "issuer": metadata_base,
                                "token_endpoint": format!("{metadata_base}/oauth/token"),
                                "revocation_endpoint": format!("{metadata_base}/oauth/revoke"),
                                "device_authorization_endpoint": format!("{metadata_base}/oauth/device/authorize"),
                                "grant_types_supported": [DEVICE_GRANT, "refresh_token"],
                                "cli_client_id": "notegate-cli-local",
                            }))
                        }
                    }),
                )
                .route(
                    "/oauth/device/authorize",
                    post({
                        let base = base.clone();
                        move |Form(form): Form<HashMap<String, String>>| {
                            let base = base.clone();
                            async move {
                                assert_eq!(form.get("client_id").map(String::as_str), Some("notegate-cli-local"));
                                Json(json!({
                                    "device_code": "device-secret-never-print",
                                    "user_code": "BCDF-GHKM",
                                    "verification_uri": format!("{base}/device"),
                                    "verification_uri_complete": format!("{base}/device?user_code=BCDF-GHKM"),
                                    "expires_in": 30,
                                    "interval": 1,
                                }))
                            }
                        }
                    }),
                )
                .route(
                    "/oauth/token",
                    post(move |Form(form): Form<HashMap<String, String>>| {
                        hits.fetch_add(1, Ordering::SeqCst);
                        async move {
                            assert_eq!(form.get("device_code").map(String::as_str), Some("device-secret-never-print"));
                            Json(json!({
                                "access_token": "access-secret-never-print",
                                "refresh_token": "refresh-secret-never-print",
                                "token_type": "Bearer",
                                "expires_in": 900,
                            }))
                        }
                    }),
                )
        })
        .await;
        let store = Arc::new(MemoryStore::default());
        let manager = AuthManager::new(
            Duration::from_secs(5),
            store.clone(),
            test_lock_dir("login"),
        )
        .unwrap();
        let mut events = Vec::new();

        let result = manager
            .login(&base, None, |event| {
                events.push(event.clone());
                Ok(())
            })
            .await
            .unwrap();

        assert_eq!(token_hits.load(Ordering::SeqCst), 1);
        assert_eq!(events.len(), 1);
        let event = events.first().unwrap();
        assert_eq!(
            event.get("event").and_then(Value::as_str),
            Some("verification_required")
        );
        assert_eq!(
            event
                .get("verification_uri_complete")
                .and_then(Value::as_str),
            Some(format!("{base}/device?user_code=BCDF-GHKM").as_str())
        );
        assert_eq!(
            result.get("event").and_then(Value::as_str),
            Some("login_succeeded")
        );
        let output = format!("{event}\n{result}");
        for secret in [
            "device-secret-never-print",
            "access-secret-never-print",
            "refresh-secret-never-print",
        ] {
            assert!(!output.contains(secret));
        }
        assert_eq!(
            store.load_for_base_url(&base).unwrap().unwrap().client_id,
            "notegate-cli-local"
        );
    }

    #[tokio::test]
    async fn login_refuses_to_orphan_an_existing_refresh_credential() {
        let store = Arc::new(MemoryStore::default());
        let bundle = TokenBundle::new(
            "https://notegate.example".to_owned(),
            "https://auth.example.test".to_owned(),
            "notegate-cli".to_owned(),
            "https://auth.example.test/oauth/token".to_owned(),
            "https://auth.example.test/oauth/revoke".to_owned(),
            "existing-access".to_owned(),
            "existing-refresh".to_owned(),
            u64::MAX,
        );
        store.create(&bundle).unwrap();
        let manager = AuthManager::new(
            Duration::from_secs(1),
            store.clone(),
            test_lock_dir("relogin"),
        )
        .unwrap();

        let error = manager
            .login(
                "https://notegate.example",
                Some(AuthOverride {
                    authgate_url: "https://auth.example.test".to_owned(),
                    client_id: "notegate-cli".to_owned(),
                }),
                |_event| panic!("existing credentials must be rejected before Device Flow"),
            )
            .await
            .unwrap_err();

        assert_eq!(
            error.body().get("error").and_then(Value::as_str),
            Some("already_authenticated")
        );
        assert_eq!(
            store.load(&bundle.key()).unwrap().unwrap().refresh_token(),
            "existing-refresh"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_login_fails_without_waiting_and_issues_one_device_credential() {
        let device_hits = Arc::new(AtomicUsize::new(0));
        let authorization_entered = Arc::new(Notify::new());
        let release_authorization = Arc::new(Notify::new());
        let device_counter = Arc::clone(&device_hits);
        let entered = Arc::clone(&authorization_entered);
        let release = Arc::clone(&release_authorization);
        let base = spawn_with_builder(move |base| {
            let metadata_base = base.clone();
            Router::new()
                .route(
                    METADATA_PATH,
                    get(move || {
                        let metadata_base = metadata_base.clone();
                        async move {
                            Json(json!({
                                "issuer": metadata_base,
                                "token_endpoint": format!("{metadata_base}/oauth/token"),
                                "revocation_endpoint": format!("{metadata_base}/oauth/revoke"),
                                "device_authorization_endpoint": format!("{metadata_base}/oauth/device/authorize"),
                                "grant_types_supported": [DEVICE_GRANT, "refresh_token"],
                                "cli_client_id": "notegate-cli-local",
                            }))
                        }
                    }),
                )
                .route(
                    "/oauth/device/authorize",
                    post({
                        let base = base.clone();
                        move || {
                            let base = base.clone();
                            let device_counter = Arc::clone(&device_counter);
                            let entered = Arc::clone(&entered);
                            let release = Arc::clone(&release);
                            async move {
                                device_counter.fetch_add(1, Ordering::SeqCst);
                                entered.notify_one();
                                release.notified().await;
                                Json(json!({
                                    "device_code": "blocked-device",
                                    "user_code": "BCDF-GHKM",
                                    "verification_uri": format!("{base}/device"),
                                    "expires_in": 30,
                                    "interval": 1,
                                }))
                            }
                        }
                    }),
                )
                .route(
                    "/oauth/token",
                    post(|| async {
                        Json(json!({
                            "access_token": "blocked-access",
                            "refresh_token": "blocked-refresh",
                            "token_type": "Bearer",
                            "expires_in": 900,
                        }))
                    }),
                )
        })
        .await;
        let store = Arc::new(MemoryStore::default());
        let lock_dir = test_lock_dir("login-in-progress");
        let first =
            AuthManager::new(Duration::from_secs(5), store.clone(), lock_dir.clone()).unwrap();
        let second = AuthManager::new(Duration::from_secs(5), store, lock_dir).unwrap();
        let first_base = base.clone();
        let first_task =
            tokio::spawn(async move { first.login(&first_base, None, |_event| Ok(())).await });

        tokio::time::timeout(Duration::from_secs(1), authorization_entered.notified())
            .await
            .expect("first login must reach Device authorization");
        let second_error = tokio::time::timeout(
            Duration::from_millis(500),
            second.login(&base, None, |_event| Ok(())),
        )
        .await
        .expect("second login must fail without waiting for the first login")
        .unwrap_err();

        assert_eq!(
            second_error.body().get("error").and_then(Value::as_str),
            Some("login_in_progress")
        );
        assert_eq!(second_error.exit_code(), crate::error::EXIT_UNAVAILABLE);
        assert_eq!(
            second_error.body().get("kind").and_then(Value::as_str),
            Some("authentication_error")
        );
        assert_eq!(
            second_error
                .body()
                .pointer("/data/retryable")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(device_hits.load(Ordering::SeqCst), 1);
        release_authorization.notify_one();
        first_task.await.unwrap().unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn refresh_rotation_is_serialized_and_reread_after_lock() {
        let hits = Arc::new(AtomicUsize::new(0));
        let server_hits = Arc::clone(&hits);
        let base = spawn(Router::new().route(
            "/oauth/token",
            post(move |Form(form): Form<HashMap<String, String>>| {
                let server_hits = Arc::clone(&server_hits);
                async move {
                    server_hits.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(
                        form.get("refresh_token").map(String::as_str),
                        Some("old-refresh")
                    );
                    sleep(Duration::from_millis(100)).await;
                    Json(json!({
                        "access_token": "new-access",
                        "refresh_token": "new-refresh",
                        "token_type": "Bearer",
                        "expires_in": 900,
                    }))
                }
            }),
        ))
        .await;
        let store = Arc::new(MemoryStore::default());
        let bundle = bundle_for(&base, "old-access", "old-refresh", 0);
        store.create(&bundle).unwrap();
        let lock_dir = test_lock_dir("refresh-concurrency");
        let first =
            AuthManager::new(Duration::from_secs(5), store.clone(), lock_dir.clone()).unwrap();
        let second = AuthManager::new(Duration::from_secs(5), store.clone(), lock_dir).unwrap();

        let (first_token, second_token) = tokio::join!(
            first.access_token("http://localhost:9191"),
            second.access_token("http://localhost:9191")
        );

        assert_eq!(first_token.unwrap().expose_secret(), "new-access");
        assert_eq!(second_token.unwrap().expose_secret(), "new-access");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let saved = store.load(&bundle.key()).unwrap().unwrap();
        assert_eq!(saved.refresh_token(), "new-refresh");
    }

    #[tokio::test]
    async fn refresh_writes_a_durable_marker_before_sending() {
        let store = Arc::new(MemoryStore::default());
        let lock_dir = test_lock_dir("refresh-write-ahead");
        let observed_marker = Arc::new(Mutex::new(None::<PathBuf>));
        let server_marker = Arc::clone(&observed_marker);
        let base = spawn(Router::new().route(
            "/oauth/token",
            post(move || {
                let server_marker = Arc::clone(&server_marker);
                async move {
                    let marker_path = server_marker.lock().unwrap().clone().unwrap();
                    assert!(
                        marker_path.is_file(),
                        "refresh marker must be durable before the request arrives"
                    );
                    Json(json!({
                        "access_token": "new-access",
                        "refresh_token": "new-refresh",
                        "token_type": "Bearer",
                        "expires_in": 900,
                    }))
                }
            }),
        ))
        .await;
        let bundle = bundle_for(&base, "old-access", "old-refresh", 0);
        let marker_path = lock_dir.join(format!("{}.refresh-uncertain", key_digest(&bundle.key())));
        *observed_marker.lock().unwrap() = Some(marker_path.clone());
        store.create(&bundle).unwrap();
        let manager = AuthManager::new(Duration::from_secs(5), store, lock_dir).unwrap();

        let token = manager.access_token("http://localhost:9191").await.unwrap();

        assert_eq!(token.expose_secret(), "new-access");
        assert!(!marker_path.exists());
    }

    #[tokio::test]
    async fn ambiguous_refresh_is_marked_and_never_retried() {
        let hits = Arc::new(AtomicUsize::new(0));
        let server_hits = Arc::clone(&hits);
        let base = spawn(Router::new().route(
            "/oauth/token",
            post(move || {
                server_hits.fetch_add(1, Ordering::SeqCst);
                async { (StatusCode::OK, "not-json") }
            }),
        ))
        .await;
        let store = Arc::new(MemoryStore::default());
        let bundle = bundle_for(&base, "old-access", "old-refresh", 0);
        store.create(&bundle).unwrap();
        let manager = AuthManager::new(
            Duration::from_secs(5),
            store.clone(),
            test_lock_dir("refresh-ambiguous"),
        )
        .unwrap();

        for _ in 0..2 {
            let error = manager
                .access_token("http://localhost:9191")
                .await
                .unwrap_err();
            assert_eq!(
                error.body().get("error").and_then(Value::as_str),
                Some("refresh_outcome_unknown")
            );
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(
            store.load(&bundle.key()).unwrap().unwrap().refresh_state,
            RefreshState::Uncertain
        );
    }

    #[tokio::test]
    async fn ambiguous_refresh_recovers_directly_through_login() {
        let token_calls = Arc::new(AtomicUsize::new(0));
        let device_hits = Arc::new(AtomicUsize::new(0));
        let token_counter = Arc::clone(&token_calls);
        let device_counter = Arc::clone(&device_hits);
        let base = spawn_with_builder(move |base| {
            let metadata_base = base.clone();
            Router::new()
                .route(
                    METADATA_PATH,
                    get(move || {
                        let metadata_base = metadata_base.clone();
                        async move {
                            Json(json!({
                                "issuer": metadata_base,
                                "token_endpoint": format!("{metadata_base}/oauth/token"),
                                "revocation_endpoint": format!("{metadata_base}/oauth/revoke"),
                                "device_authorization_endpoint": format!("{metadata_base}/oauth/device/authorize"),
                                "grant_types_supported": [DEVICE_GRANT, "refresh_token"],
                                "cli_client_id": "notegate-cli-local",
                            }))
                        }
                    }),
                )
                .route(
                    "/oauth/device/authorize",
                    post({
                        let base = base.clone();
                        move || {
                            let base = base.clone();
                            let device_counter = Arc::clone(&device_counter);
                            async move {
                                device_counter.fetch_add(1, Ordering::SeqCst);
                                Json(json!({
                                    "device_code": "recovery-device",
                                    "user_code": "BCDF-GHKM",
                                    "verification_uri": format!("{base}/device"),
                                    "expires_in": 30,
                                    "interval": 1,
                                }))
                            }
                        }
                    }),
                )
                .route(
                    "/oauth/token",
                    post(move || {
                        let call = token_counter.fetch_add(1, Ordering::SeqCst);
                        async move {
                            if call == 0 {
                                (StatusCode::OK, "not-json").into_response()
                            } else {
                                Json(json!({
                                    "access_token": "recovered-access",
                                    "refresh_token": "recovered-refresh",
                                    "token_type": "Bearer",
                                    "expires_in": 900,
                                }))
                                .into_response()
                            }
                        }
                    }),
                )
        })
        .await;
        let store = Arc::new(MemoryStore::default());
        let bundle = TokenBundle::new(
            base.clone(),
            base.clone(),
            "notegate-cli-local".to_owned(),
            format!("{base}/oauth/token"),
            format!("{base}/oauth/revoke"),
            "old-access".to_owned(),
            "old-refresh".to_owned(),
            0,
        );
        store.create(&bundle).unwrap();
        let lock_dir = test_lock_dir("ambiguous-login-recovery");
        let marker_path = lock_dir.join(format!("{}.refresh-uncertain", key_digest(&bundle.key())));
        let manager = AuthManager::new(Duration::from_secs(5), store.clone(), lock_dir).unwrap();

        let refresh_error = manager.access_token(&base).await.unwrap_err();
        assert_eq!(
            refresh_error.body().get("error").and_then(Value::as_str),
            Some("refresh_outcome_unknown")
        );
        assert!(marker_path.exists());

        let result = manager.login(&base, None, |_event| Ok(())).await.unwrap();

        assert_eq!(
            result.get("event").and_then(Value::as_str),
            Some("login_succeeded")
        );
        assert_eq!(device_hits.load(Ordering::SeqCst), 1);
        assert_eq!(token_calls.load(Ordering::SeqCst), 2);
        let recovered = store.load(&bundle.key()).unwrap().unwrap();
        assert_eq!(recovered.refresh_state, RefreshState::Ready);
        assert_eq!(recovered.refresh_token(), "recovered-refresh");
        assert!(!marker_path.exists());
    }

    #[tokio::test]
    async fn rotated_refresh_is_not_reused_when_persistence_fails() {
        let hits = Arc::new(AtomicUsize::new(0));
        let server_hits = Arc::clone(&hits);
        let base = spawn(Router::new().route(
            "/oauth/token",
            post(move || {
                server_hits.fetch_add(1, Ordering::SeqCst);
                async {
                    Json(json!({
                        "access_token": "rotated-access",
                        "refresh_token": "rotated-refresh",
                        "token_type": "Bearer",
                        "expires_in": 900,
                    }))
                }
            }),
        ))
        .await;
        let store = Arc::new(FailingSaveStore::default());
        let bundle = bundle_for(&base, "old-access", "old-refresh", 0);
        store.create(&bundle).unwrap();
        store.fail_saves.store(true, Ordering::SeqCst);
        let manager = AuthManager::new(
            Duration::from_secs(5),
            store.clone(),
            test_lock_dir("refresh-save-failure"),
        )
        .unwrap();

        let first = manager
            .access_token("http://localhost:9191")
            .await
            .unwrap_err();
        let second = manager
            .access_token("http://localhost:9191")
            .await
            .unwrap_err();

        assert_eq!(
            first.body().get("error").and_then(Value::as_str),
            Some("credential_persistence_failed")
        );
        assert_eq!(
            second.body().get("error").and_then(Value::as_str),
            Some("login_required")
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(store.load(&bundle.key()).unwrap().is_none());
    }

    #[tokio::test]
    async fn refresh_invalid_grant_deletes_local_credential_and_requires_login() {
        let base = spawn(Router::new().route(
            "/oauth/token",
            post(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid_grant"})),
                )
            }),
        ))
        .await;
        let store = Arc::new(MemoryStore::default());
        let bundle = bundle_for(&base, "old-access", "old-refresh", 0);
        store.create(&bundle).unwrap();
        let lock_dir = test_lock_dir("refresh-invalid-grant");
        let marker_path = lock_dir.join(format!("{}.refresh-uncertain", key_digest(&bundle.key())));
        let manager = AuthManager::new(Duration::from_secs(5), store.clone(), lock_dir).unwrap();

        let error = manager
            .access_token("http://localhost:9191")
            .await
            .unwrap_err();

        assert_eq!(
            error.body().get("error").and_then(Value::as_str),
            Some("login_required")
        );
        assert!(store.load(&bundle.key()).unwrap().is_none());
        assert!(!marker_path.exists());
    }

    #[tokio::test]
    async fn logout_revokes_once_and_is_locally_idempotent() {
        let hits = Arc::new(AtomicUsize::new(0));
        let server_hits = Arc::clone(&hits);
        let base = spawn(Router::new().route(
            "/oauth/revoke",
            post(move |Form(form): Form<HashMap<String, String>>| {
                server_hits.fetch_add(1, Ordering::SeqCst);
                async move {
                    assert_eq!(form.get("token").map(String::as_str), Some("refresh"));
                    StatusCode::OK
                }
            }),
        ))
        .await;
        let store = Arc::new(MemoryStore::default());
        let bundle = bundle_for(&base, "access", "refresh", u64::MAX);
        store.create(&bundle).unwrap();
        let manager = AuthManager::new(
            Duration::from_secs(5),
            store.clone(),
            test_lock_dir("logout"),
        )
        .unwrap();

        let first = manager.logout("http://localhost:9191", None).await.unwrap();
        let second = manager.logout("http://localhost:9191", None).await.unwrap();

        assert_eq!(first.get("revocation_attempted"), Some(&Value::Bool(true)));
        assert_eq!(
            second.get("revocation_attempted"),
            Some(&Value::Bool(false))
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(store.load(&bundle.key()).unwrap().is_none());
    }

    #[tokio::test]
    async fn logout_deletes_local_credential_when_revocation_fails() {
        let hits = Arc::new(AtomicUsize::new(0));
        let server_hits = Arc::clone(&hits);
        let base = spawn(Router::new().route(
            "/oauth/revoke",
            post(move || {
                server_hits.fetch_add(1, Ordering::SeqCst);
                async { StatusCode::SERVICE_UNAVAILABLE }
            }),
        ))
        .await;
        let store = Arc::new(MemoryStore::default());
        let bundle = bundle_for(&base, "access", "refresh", u64::MAX);
        store.create(&bundle).unwrap();
        let manager = AuthManager::new(
            Duration::from_secs(5),
            store.clone(),
            test_lock_dir("logout-revoke-failure"),
        )
        .unwrap();

        let error = manager
            .logout("http://localhost:9191", None)
            .await
            .unwrap_err();

        assert_eq!(
            error.body().get("error").and_then(Value::as_str),
            Some("revocation_failed")
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(store.load(&bundle.key()).unwrap().is_none());
    }

    fn bundle_for(
        authgate_base: &str,
        access_token: &str,
        refresh_token: &str,
        expires_at: u64,
    ) -> TokenBundle {
        TokenBundle::new(
            "http://localhost:9191".to_owned(),
            authgate_base.to_owned(),
            "notegate-cli-local".to_owned(),
            format!("{authgate_base}/oauth/token"),
            format!("{authgate_base}/oauth/revoke"),
            access_token.to_owned(),
            refresh_token.to_owned(),
            expires_at,
        )
    }

    fn test_lock_dir(label: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "notegate-cli-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    async fn spawn(app: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{address}")
    }

    async fn spawn_with_builder(builder: impl FnOnce(String) -> Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let base = format!("http://{address}");
        let app = builder(base.clone());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        base
    }
}
