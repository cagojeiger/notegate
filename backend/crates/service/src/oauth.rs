//! NoteGate's resource-bound public-client grants. External OIDC tokens never enter this store.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use notegate_core::{Result, security::PiiCrypto};
use notegate_db::oauth_repo::{CredentialIssue, ExchangeOutcome, OAuthRepo};
use rand::RngExt as _;
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

pub const ACCESS_SECONDS: i64 = 600;
pub const FLOW_SECONDS: i64 = 300;
pub const CODE_SECONDS: i64 = 60;
const REFRESH_IDLE_DAYS: i64 = 7;
const REFRESH_MAX_DAYS: i64 = 30;

#[derive(Clone, Debug)]
pub struct OAuthService {
    pub store: OAuthRepo,
    crypto: PiiCrypto,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

pub enum TokenOutcome {
    Issued(TokenResponse),
    Rejected(&'static str),
}

#[derive(Serialize)]
pub struct DeviceResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: i64,
    pub interval: i64,
}

struct Tokens {
    access: String,
    refresh: String,
    access_hash: String,
    refresh_hash: String,
}

impl OAuthService {
    pub fn new(store: OAuthRepo, crypto: PiiCrypto) -> Self {
        Self { store, crypto }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn exchange_code(
        &self,
        code: &str,
        client: &str,
        resource: &str,
        redirect: &str,
        verifier: &str,
        now: DateTime<Utc>,
        environment: Option<&str>,
    ) -> Result<TokenOutcome> {
        if !(43..=128).contains(&verifier.len())
            || !verifier
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
        {
            return Ok(TokenOutcome::Rejected("invalid_grant"));
        }
        let tokens = self.tokens()?;
        let mut issue = self.issue(&tokens, now);
        issue.client_environment = environment;
        let outcome = self
            .store
            .exchange_code(
                &self.crypto.oauth_hash("code", code)?,
                client,
                resource,
                redirect,
                &pkce_challenge(verifier),
                &issue,
                now,
            )
            .await?;
        self.response(tokens, outcome, now)
    }

    pub async fn refresh(
        &self,
        token: &str,
        client: &str,
        resource: &str,
        now: DateTime<Utc>,
    ) -> Result<TokenOutcome> {
        self.refresh_with_scope(token, client, resource, None, now)
            .await
    }

    pub async fn refresh_with_scope(
        &self,
        token: &str,
        client: &str,
        resource: &str,
        scope: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<TokenOutcome> {
        let tokens = self.tokens()?;
        let hash = self.crypto.oauth_hash("token", token)?;
        let encrypted = self.crypto.encrypt_oauth_state(
            &format!("refresh-retry:{hash}"),
            &serde_json::to_string(&(
                &tokens.access,
                &tokens.refresh,
                now + Duration::seconds(ACCESS_SECONDS),
            ))
            .map_err(|_| notegate_core::Error::internal("refresh response encoding failed"))?,
        )?;
        let private_response = serde_json::json!({
            "refresh_hash": tokens.refresh_hash,
            "tokens": encrypted,
        });
        let mut issue = self.issue(&tokens, now);
        issue.private_refresh_response = Some(&private_response);
        let outcome = self
            .store
            .rotate_refresh(&hash, client, resource, scope, &issue, now)
            .await?;
        self.response(tokens, outcome, now)
    }

    pub async fn device(
        &self,
        client: &str,
        resource: &str,
        scope: &str,
        public_url: &str,
        now: DateTime<Utc>,
    ) -> Result<DeviceResponse> {
        let device_code = random_secret();
        // 40 random bits, human-readable and case-insensitive. Brute force is also ingress-limited.
        let bytes: [u8; 5] = rand::rng().random();
        let code = bytes.iter().map(|b| format!("{b:02X}")).collect::<String>();
        let user_code = format!(
            "{}-{}",
            code.get(..5).unwrap_or_default(),
            code.get(5..).unwrap_or_default()
        );
        self.store
            .insert_device(
                &self.crypto.oauth_hash("device", &device_code)?,
                &self
                    .crypto
                    .oauth_hash("user-code", &normalize_user_code(&user_code))?,
                client,
                resource,
                scope,
                now,
                now + Duration::seconds(FLOW_SECONDS),
            )
            .await?;
        let verification_uri = format!("{public_url}/oauth/device");
        Ok(DeviceResponse {
            device_code,
            user_code: user_code.clone(),
            verification_uri_complete: format!("{verification_uri}?user_code={user_code}"),
            verification_uri,
            expires_in: FLOW_SECONDS,
            interval: 5,
        })
    }

    pub async fn approve_device(
        &self,
        code: &str,
        user: Uuid,
        approve: bool,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        self.store
            .approve_device(
                &self
                    .crypto
                    .oauth_hash("user-code", &normalize_user_code(code))?,
                user,
                approve,
                now,
            )
            .await
    }

    pub async fn poll_device(
        &self,
        code: &str,
        client: &str,
        resource: &str,
        now: DateTime<Utc>,
        environment: Option<&str>,
    ) -> Result<TokenOutcome> {
        let tokens = self.tokens()?;
        let mut issue = self.issue(&tokens, now);
        issue.client_environment = environment;
        let outcome = self
            .store
            .poll_device(
                &self.crypto.oauth_hash("device", code)?,
                client,
                resource,
                &issue,
                now,
            )
            .await?;
        self.response(tokens, outcome, now)
    }

    pub async fn access_user(
        &self,
        token: &str,
        resource: &str,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<Uuid>> {
        if token.len() > 256 {
            return Ok(None);
        }
        self.store
            .access_user(
                &self.crypto.oauth_hash("token", token)?,
                resource,
                scope,
                now,
            )
            .await
    }

    pub async fn revoke(&self, token: &str, client: &str, now: DateTime<Utc>) -> Result<()> {
        self.store
            .revoke_token(&self.crypto.oauth_hash("token", token)?, client, now)
            .await
    }

    fn tokens(&self) -> Result<Tokens> {
        let access = random_secret();
        let refresh = random_secret();
        Ok(Tokens {
            access_hash: self.crypto.oauth_hash("token", &access)?,
            refresh_hash: self.crypto.oauth_hash("token", &refresh)?,
            access,
            refresh,
        })
    }

    fn issue<'a>(&'a self, tokens: &'a Tokens, now: DateTime<Utc>) -> CredentialIssue<'a> {
        CredentialIssue {
            client_environment: None,
            access_hash: &tokens.access_hash,
            refresh_hash: Some(&tokens.refresh_hash),
            key_id: self.crypto.lookup_key_id(),
            version: self.crypto.version(),
            access_expires_at: now + Duration::seconds(ACCESS_SECONDS),
            grant_expires_at: now + Duration::days(REFRESH_MAX_DAYS),
            idle_expires_at: now + Duration::days(REFRESH_IDLE_DAYS),
            private_refresh_response: None,
        }
    }

    fn response(
        &self,
        tokens: Tokens,
        outcome: ExchangeOutcome,
        now: DateTime<Utc>,
    ) -> Result<TokenOutcome> {
        match outcome {
            ExchangeOutcome::Rejected(error) => Ok(TokenOutcome::Rejected(error)),
            ExchangeOutcome::Issued(grant) => Ok(TokenOutcome::Issued(TokenResponse {
                access_token: tokens.access,
                token_type: "Bearer",
                expires_in: ACCESS_SECONDS.min((grant.expires_at - now).num_seconds()),
                refresh_token: grant
                    .scope
                    .split_whitespace()
                    .any(|s| s == "offline_access")
                    .then_some(tokens.refresh),
                scope: grant.scope,
            })),
            ExchangeOutcome::Replayed {
                grant,
                credential_hash,
                private_response,
            } => {
                let encrypted = serde_json::from_value(
                    private_response.get("tokens").cloned().unwrap_or_default(),
                )
                .map_err(|_| notegate_core::Error::internal("invalid refresh response cache"))?;
                let plaintext = self
                    .crypto
                    .decrypt_oauth_state(&format!("refresh-retry:{credential_hash}"), &encrypted)?;
                let (access, refresh, expires): (String, String, DateTime<Utc>) =
                    serde_json::from_str(&plaintext)
                        .map_err(|_| notegate_core::Error::internal("invalid refresh response"))?;
                Ok(TokenOutcome::Issued(TokenResponse {
                    access_token: access,
                    token_type: "Bearer",
                    expires_in: ACCESS_SECONDS
                        .min((expires.min(grant.expires_at) - now).num_seconds())
                        .max(0),
                    refresh_token: Some(refresh),
                    scope: grant.scope,
                }))
            }
        }
    }
}

pub fn random_secret() -> String {
    URL_SAFE_NO_PAD.encode(rand::rng().random::<[u8; 32]>())
}
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
pub fn normalize_user_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect()
}
