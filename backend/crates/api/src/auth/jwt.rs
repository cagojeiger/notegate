use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::identity::ResolveAttrs;
use aliri::jwt::{Audiences, BasicClaims, CoreClaims, IssuerRef, SubjectRef};
use aliri::{Jwt, jwa, jwt};
use aliri_oauth2::{Authority, AuthorityError, HasScope, Scope, ScopePolicy};
use serde::Deserialize;

use crate::auth::bearer::AuthError;

// Initial fetches and unknown key IDs share this budget, even on failure or
// cancellation. A new key may need a retry after 5 seconds plus fetch latency.
const ON_DEMAND_REFRESH_COOLDOWN: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct JwtAuthority {
    inner: Arc<JwtAuthorityInner>,
}

struct JwtAuthorityInner {
    jwks_url: Option<String>,
    validator: jwt::CoreValidator,
    refresh_interval: Duration,
    authority: tokio::sync::OnceCell<Authority>,
    last_refresh_attempt: tokio::sync::Mutex<Option<Instant>>,
}

impl JwtAuthority {
    pub fn from_url(config: &notegate_core::Config, jwks_url: String) -> Self {
        Self {
            inner: Arc::new(JwtAuthorityInner {
                jwks_url: Some(jwks_url),
                validator: jwt_validator(config),
                refresh_interval: config.jwks_cache_ttl,
                authority: tokio::sync::OnceCell::new(),
                last_refresh_attempt: tokio::sync::Mutex::new(None),
            }),
        }
    }

    #[cfg(test)]
    pub fn from_jwks(config: &notegate_core::Config, jwks: aliri::Jwks) -> Self {
        let authority = Authority::new(jwks, jwt_validator(config));
        let cell = tokio::sync::OnceCell::new();
        let _ = cell.set(authority);
        Self {
            inner: Arc::new(JwtAuthorityInner {
                jwks_url: None,
                validator: jwt_validator(config),
                refresh_interval: config.jwks_cache_ttl,
                authority: cell,
                last_refresh_attempt: tokio::sync::Mutex::new(None),
            }),
        }
    }

    pub async fn verify_for_audiences(
        &self,
        token: &str,
        expected_audiences: &[&str],
    ) -> Result<ResolveAttrs, AuthError> {
        let claims = self.verify_claims(token).await?;
        let audience_matches = !expected_audiences.is_empty()
            && claims.aud().iter().any(|actual| {
                expected_audiences
                    .iter()
                    .any(|expected| actual.as_str() == *expected)
            });
        if !audience_matches {
            return Err(AuthError::InvalidToken);
        }
        attrs_from_claims(claims)
    }

    async fn authority(&self, now: &impl Fn() -> Instant) -> Result<&Authority, AuthError> {
        if let Some(authority) = self.inner.authority.get() {
            return Ok(authority);
        }
        let mut last_attempt = self.inner.last_refresh_attempt.lock().await;
        if let Some(authority) = self.inner.authority.get() {
            return Ok(authority);
        }
        if refresh_is_cooling_down(*last_attempt, now()) {
            return Err(AuthError::Unavailable);
        }
        let jwks_url = self.inner.jwks_url.clone().ok_or(AuthError::Internal)?;
        *last_attempt = Some(now());
        tracing::info!(event = "jwks.initial_fetch.start");
        let result = Authority::new_from_url(jwks_url, self.inner.validator.clone()).await;
        *last_attempt = Some(now());
        let authority = result.map_err(|error| {
            tracing::error!(event = "jwks.initial_fetch.failed", %error);
            AuthError::Internal
        })?;
        tracing::info!(
            event = "jwks.initial_fetch.ok",
            refresh_interval_secs = self.inner.refresh_interval.as_secs(),
        );
        authority.spawn_refresh(self.inner.refresh_interval);
        self.inner
            .authority
            .set(authority)
            .map_err(|_error| AuthError::Internal)?;
        self.inner.authority.get().ok_or(AuthError::Internal)
    }

    async fn verify_claims(&self, token: &str) -> Result<JwtClaims, AuthError> {
        self.verify_claims_with_clock(token, Instant::now).await
    }

    async fn verify_claims_with_clock(
        &self,
        token: &str,
        now: impl Fn() -> Instant,
    ) -> Result<JwtClaims, AuthError> {
        let authority = self.authority(&now).await?;
        let jwt = Jwt::from(token.trim());
        match authority.verify_token::<JwtClaims>(&jwt, &ScopePolicy::allow_any()) {
            Ok(claims) => Ok(claims),
            Err(AuthorityError::UnknownKeyId) => {
                let mut last_attempt = self.inner.last_refresh_attempt.lock().await;
                // A concurrent refresh may already have supplied the requested key.
                match authority.verify_token::<JwtClaims>(&jwt, &ScopePolicy::allow_any()) {
                    Err(AuthorityError::UnknownKeyId) => {}
                    result => return result.map_err(map_authority_error),
                }
                if refresh_is_cooling_down(*last_attempt, now()) {
                    return Err(AuthError::InvalidToken);
                }
                *last_attempt = Some(now());
                tracing::info!(event = "jwks.refresh.start", reason = "unknown_key_id");
                let result = authority.refresh().await;
                *last_attempt = Some(now());
                result.map_err(|error| {
                    tracing::warn!(event = "jwks.refresh.failed", reason = "unknown_key_id", %error);
                    AuthError::InvalidToken
                })?;
                tracing::info!(event = "jwks.refresh.ok", reason = "unknown_key_id");
                authority
                    .verify_token::<JwtClaims>(&jwt, &ScopePolicy::allow_any())
                    .map_err(map_authority_error)
            }
            Err(error) => Err(map_authority_error(error)),
        }
    }
}

fn refresh_is_cooling_down(last_attempt: Option<Instant>, now: Instant) -> bool {
    last_attempt
        .is_some_and(|last| now.saturating_duration_since(last) < ON_DEMAND_REFRESH_COOLDOWN)
}

fn jwt_validator(config: &notegate_core::Config) -> jwt::CoreValidator {
    // Audience is intentionally enforced by `verify_for_audiences` at each
    // protected surface so one JWKS cache can safely serve MCP and Command API.
    jwt::CoreValidator::default()
        .add_approved_algorithm(jwa::Algorithm::RS256)
        .require_issuer(jwt::Issuer::new(config.authgate_url.clone()))
}

#[derive(Clone, Debug, Deserialize)]
struct JwtClaims {
    #[serde(flatten)]
    basic: BasicClaims,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    scope: Scope,
}

impl CoreClaims for JwtClaims {
    fn nbf(&self) -> Option<aliri_clock::UnixTime> {
        self.basic.nbf()
    }

    fn exp(&self) -> Option<aliri_clock::UnixTime> {
        self.basic.exp()
    }

    fn aud(&self) -> &Audiences {
        self.basic.aud()
    }

    fn iss(&self) -> Option<&IssuerRef> {
        self.basic.iss()
    }

    fn sub(&self) -> Option<&SubjectRef> {
        self.basic.sub()
    }
}

impl HasScope for JwtClaims {
    fn scope(&self) -> &Scope {
        &self.scope
    }
}

fn attrs_from_claims(claims: JwtClaims) -> Result<ResolveAttrs, AuthError> {
    let sub = claims
        .sub()
        .ok_or(AuthError::InvalidToken)?
        .as_str()
        .to_owned();
    Ok(ResolveAttrs {
        sub,
        email: claims.email.unwrap_or_default(),
        name: claims.name.unwrap_or_default(),
    })
}

fn map_authority_error(error: AuthorityError) -> AuthError {
    match error {
        AuthorityError::UnknownKeyId | AuthorityError::JwtVerifyError(_) => AuthError::InvalidToken,
        AuthorityError::PolicyDenial(_error) => AuthError::InvalidToken,
    }
}

#[cfg(test)]
#[path = "jwt_tests.rs"]
mod tests;
