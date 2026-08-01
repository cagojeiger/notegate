//! Runtime configuration loaded and validated from layered sources.
//!
//! Load order is: built-in defaults < optional TOML files < environment.
//! Validation is fail-fast: a bad value aborts boot with a precise message
//! rather than surfacing as a confusing runtime error later.

use std::net::SocketAddr;
use std::time::Duration;

use config::{Config as LayeredConfig, Environment, File, FileFormat};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer};
use url::Url;
use validator::{Validate, ValidationError, ValidationErrors};

use crate::error::{Error, Result};
use crate::limits::{
    HTTP_API_SURFACE_RATE_LIMIT_BURST, HTTP_API_SURFACE_RATE_LIMIT_REQUESTS_PER_SECOND,
    HTTP_INGRESS_RATE_LIMIT_BURST, HTTP_INGRESS_RATE_LIMIT_REQUESTS_PER_SECOND, Limits,
};
use crate::tier::UserTier;

const DEFAULT_BIND_ADDR: &str = "0.0.0.0:9191";
const DEFAULT_DB_MAX_CONNECTIONS: u32 = 10;
const DEFAULT_JWKS_CACHE_TTL_SECS: u64 = 300;
const DEFAULT_BROWSER_SESSION_TTL_SECS: u64 = 3600;
const DEFAULT_BROWSER_SESSION_MAX_TTL_SECS: u64 = 30 * 86_400;
const DEFAULT_OPENAPI_ENABLED: bool = false;
const DEFAULT_METRICS_ENABLED: bool = false;
const DEFAULT_SEARCH_BODY_CACHE_MAX_CAPACITY_BYTES: u64 = 128 * 1024 * 1024;
const DEFAULT_SEARCH_BODY_CACHE_TTL_SECS: u64 = 30 * 60;
const DEFAULT_SEARCH_BODY_CACHE_TTI_SECS: u64 = 5 * 60;
const MAX_SEARCH_BODY_CACHE_EXPIRY_SECS: u64 = 999 * 365 * 86_400;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S3Config {
    pub endpoint: String,
    pub public_endpoint: Option<String>,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: SecretString,
    #[serde(default = "default_true")]
    pub force_path_style: bool,
}

/// Process-local cache policy for decrypted text bodies used by `grep`.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SearchBodyCacheConfig {
    /// Approximate maximum plaintext bytes retained. Zero disables the cache.
    #[serde(default = "default_search_body_cache_max_capacity_bytes")]
    pub max_capacity_bytes: u64,
    /// Absolute lifetime of a cached body.
    #[serde(
        default = "default_search_body_cache_ttl",
        rename = "ttl_secs",
        deserialize_with = "duration_from_secs"
    )]
    pub ttl: Duration,
    /// Maximum idle lifetime, refreshed by cache hits.
    #[serde(
        default = "default_search_body_cache_tti",
        rename = "tti_secs",
        deserialize_with = "duration_from_secs"
    )]
    pub tti: Duration,
}

impl Default for SearchBodyCacheConfig {
    fn default() -> Self {
        Self {
            max_capacity_bytes: default_search_body_cache_max_capacity_bytes(),
            ttl: default_search_body_cache_ttl(),
            tti: default_search_body_cache_tti(),
        }
    }
}

/// One process-local token bucket.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HttpRateLimitConfig {
    /// Sustained requests accepted per second.
    pub requests_per_second: u32,
    /// Maximum short burst accepted by the bucket.
    pub burst: u32,
}

/// Independent HTTP safety buckets sharing one process ingress cap.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HttpRateLimitsConfig {
    /// Shared cap for all data-plane routes.
    pub ingress: HttpRateLimitConfig,
    /// Browser-session REST V1 cap.
    pub browser_v1: HttpRateLimitConfig,
    /// API-key REST V2 cap.
    pub public_v2: HttpRateLimitConfig,
    /// User OAuth MCP transport cap.
    pub mcp: HttpRateLimitConfig,
    /// Agent API-key MCP V2 transport cap.
    pub mcp_v2: HttpRateLimitConfig,
}

impl Default for HttpRateLimitsConfig {
    fn default() -> Self {
        let api_surface = HttpRateLimitConfig {
            requests_per_second: HTTP_API_SURFACE_RATE_LIMIT_REQUESTS_PER_SECOND,
            burst: HTTP_API_SURFACE_RATE_LIMIT_BURST,
        };
        Self {
            ingress: HttpRateLimitConfig {
                requests_per_second: HTTP_INGRESS_RATE_LIMIT_REQUESTS_PER_SECOND,
                burst: HTTP_INGRESS_RATE_LIMIT_BURST,
            },
            browser_v1: api_surface,
            public_v2: api_surface,
            mcp: api_surface,
            mcp_v2: api_surface,
        }
    }
}

/// Server + database configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address the HTTP server binds to.
    pub bind_addr: SocketAddr,
    /// Postgres connection string.
    pub database_url: String,
    /// Max connections in the sqlx pool.
    pub db_max_connections: u32,
    /// Base URL for authgate, with trailing slash trimmed.
    pub authgate_url: String,
    /// Public URL for notegate as seen by browsers/MCP clients, with trailing slash trimmed.
    #[serde(rename = "public_url")]
    pub notegate_public_url: String,
    /// Public browser OAuth client id registered in authgate.
    #[serde(default)]
    pub oauth_client_id: String,
    /// Public MCP OAuth client id registered in authgate.
    #[serde(default)]
    pub mcp_oauth_client_id: String,
    /// Exact redirect URL registered in authgate.
    #[serde(default)]
    pub oauth_redirect_url: String,
    /// MCP resource/audience URL, with trailing slash trimmed.
    #[serde(default)]
    pub resource_url: String,
    /// Shared JWKS cache TTL.
    #[serde(
        rename = "jwks_cache_ttl_secs",
        deserialize_with = "duration_from_secs"
    )]
    pub jwks_cache_ttl: Duration,
    /// Active ENC root key id registered in crypto_key_epochs.
    pub enc_root_key_id: String,
    /// Active ENC root secret used to derive PII encryption subkeys.
    pub enc_root_secret: SecretString,
    /// Active LOOKUP root key id registered in crypto_key_epochs.
    pub lookup_root_key_id: String,
    /// Active LOOKUP root secret used to derive HMAC/session subkeys.
    pub lookup_root_secret: SecretString,
    /// Optional verify-only LOOKUP root key id for provider subject migration.
    pub lookup_verify_0_key_id: Option<String>,
    /// Optional verify-only LOOKUP root secret for provider subject migration.
    pub lookup_verify_0_secret: Option<SecretString>,
    /// Browser session local validation lease.
    #[serde(
        rename = "browser_session_ttl_secs",
        deserialize_with = "duration_from_secs"
    )]
    pub browser_session_ttl: Duration,
    /// Browser session absolute lifetime.
    #[serde(
        rename = "browser_session_max_ttl_secs",
        deserialize_with = "duration_from_secs"
    )]
    pub browser_session_max_ttl: Duration,
    /// Whether OpenAPI JSON and Swagger UI routes are exposed.
    pub openapi_enabled: bool,
    /// Whether Prometheus metrics are recorded and exposed at `/metrics`.
    pub metrics_enabled: bool,
    /// Optional directory containing the built web dashboard. When set, unknown
    /// non-API routes fall back to this directory's `index.html`.
    pub web_dist_dir: Option<String>,
    /// S3-compatible object storage connection.
    pub s3: S3Config,
    /// Tier assigned to newly created users.
    #[serde(default = "default_user_tier", deserialize_with = "user_tier_from_str")]
    pub default_user_tier: UserTier,
    /// Runtime-overridable capacity limits. Defaults match `docs/spec/performance-limits.md`.
    #[serde(default)]
    pub limits: Limits,
    /// Process-local HTTP safety limits for shared ingress and each API surface.
    #[serde(default)]
    pub http_rate_limits: HttpRateLimitsConfig,
    /// Decrypted text body cache used by content search.
    #[serde(default)]
    pub search_body_cache: SearchBodyCacheConfig,
    /// Whether login flow cookies must carry the Secure flag.
    #[serde(skip)]
    pub secure_cookies: bool,
}

impl Validate for Config {
    fn validate(&self) -> std::result::Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();

        if self.database_url.is_empty() {
            errors.add("database_url", ValidationError::new("length"));
        }
        if !(1..=256).contains(&self.db_max_connections) {
            errors.add("db_max_connections", ValidationError::new("range"));
        }
        if validate_http_url_value(&self.authgate_url).is_err() {
            errors.add("authgate_url", ValidationError::new("http_url"));
        }
        if validate_http_url_value(&self.notegate_public_url).is_err() {
            errors.add("notegate_public_url", ValidationError::new("http_url"));
        }
        if self.oauth_client_id.is_empty() {
            errors.add("oauth_client_id", ValidationError::new("length"));
        }
        if self.mcp_oauth_client_id.is_empty() {
            errors.add("mcp_oauth_client_id", ValidationError::new("length"));
        }
        if validate_http_url_value(&self.oauth_redirect_url).is_err() {
            errors.add("oauth_redirect_url", ValidationError::new("http_url"));
        }
        if validate_http_url_value(&self.resource_url).is_err() {
            errors.add("resource_url", ValidationError::new("http_url"));
        }
        if validate_jwks_cache_ttl(&self.jwks_cache_ttl).is_err() {
            errors.add("jwks_cache_ttl", ValidationError::new("range"));
        }
        if validate_key_id(&self.enc_root_key_id).is_err() {
            errors.add("enc_root_key_id", ValidationError::new("format"));
        }
        if validate_secret_min_32(&self.enc_root_secret).is_err() {
            errors.add("enc_root_secret", ValidationError::new("length"));
        }
        if validate_key_id(&self.lookup_root_key_id).is_err() {
            errors.add("lookup_root_key_id", ValidationError::new("format"));
        }
        if validate_secret_min_32(&self.lookup_root_secret).is_err() {
            errors.add("lookup_root_secret", ValidationError::new("length"));
        }
        if self.enc_root_key_id == self.lookup_root_key_id {
            errors.add("lookup_root_key_id", ValidationError::new("reused_root"));
        }
        if self.enc_root_secret.expose_secret() == self.lookup_root_secret.expose_secret() {
            errors.add("lookup_root_secret", ValidationError::new("reused_root"));
        }
        match (&self.lookup_verify_0_key_id, &self.lookup_verify_0_secret) {
            (Some(key_id), Some(secret)) => {
                if validate_key_id(key_id).is_err() {
                    errors.add("lookup_verify_0_key_id", ValidationError::new("format"));
                }
                if validate_secret_min_32(secret).is_err() {
                    errors.add("lookup_verify_0_secret", ValidationError::new("length"));
                }
            }
            (None, None) => {}
            _ => {
                errors.add("lookup_verify_0", ValidationError::new("paired"));
            }
        }
        if validate_browser_session_ttl(&self.browser_session_ttl).is_err() {
            errors.add("browser_session_ttl", ValidationError::new("range"));
        }
        if validate_browser_session_max_ttl(&self.browser_session_max_ttl).is_err() {
            errors.add("browser_session_max_ttl", ValidationError::new("range"));
        }
        if self.browser_session_ttl > self.browser_session_max_ttl {
            errors.add("browser_session_ttl", ValidationError::new("range"));
        }
        let s3 = &self.s3;
        if validate_http_url_value(&s3.endpoint).is_err() {
            errors.add("s3.endpoint", ValidationError::new("http_url"));
        }
        if s3
            .public_endpoint
            .as_deref()
            .is_some_and(|value| validate_http_url_value(value).is_err())
        {
            errors.add("s3.public_endpoint", ValidationError::new("http_url"));
        }
        for (field, value) in [
            ("s3.region", s3.region.as_str()),
            ("s3.bucket", s3.bucket.as_str()),
            ("s3.access_key", s3.access_key.as_str()),
        ] {
            if value.is_empty() {
                errors.add(field, ValidationError::new("length"));
            }
        }
        if s3.secret_key.expose_secret().is_empty() {
            errors.add("s3.secret_key", ValidationError::new("length"));
        }
        validate_limits(&self.limits, &mut errors);
        validate_http_rate_limits(&self.http_rate_limits, &mut errors);
        validate_search_body_cache(&self.search_body_cache, &mut errors);

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

impl Config {
    /// Load configuration from optional files and the process environment.
    ///
    /// Supported file layers, if present:
    /// - `config/default.toml`
    /// - `config/local.toml`
    ///
    /// `NOTEGATE_`-prefixed environment variables have highest precedence.
    pub fn load() -> Result<Self> {
        load_from_sources(true, Environment::with_prefix("NOTEGATE"))
    }

    fn normalize(&mut self) {
        self.authgate_url = trim_trailing_slashes(&self.authgate_url);
        self.notegate_public_url = trim_trailing_slashes(&self.notegate_public_url);
        self.oauth_redirect_url = if self.oauth_redirect_url.trim().is_empty() {
            format!("{}/auth/callback", self.notegate_public_url)
        } else {
            trim_trailing_slashes(&self.oauth_redirect_url)
        };
        self.resource_url = if self.resource_url.trim().is_empty() {
            format!("{}/mcp", self.notegate_public_url)
        } else {
            trim_trailing_slashes(&self.resource_url)
        };
        self.s3.endpoint = trim_trailing_slashes(&self.s3.endpoint);
        if let Some(public_endpoint) = &mut self.s3.public_endpoint {
            *public_endpoint = trim_trailing_slashes(public_endpoint);
        }
        self.secure_cookies = secure_cookies_for_redirect(&self.oauth_redirect_url);
    }
}

fn load_from_sources(include_files: bool, environment: Environment) -> Result<Config> {
    let rate_limits = HttpRateLimitsConfig::default();
    let mut builder = LayeredConfig::builder()
        .set_default("bind_addr", DEFAULT_BIND_ADDR)
        .map_err(map_config_error)?
        .set_default("db_max_connections", DEFAULT_DB_MAX_CONNECTIONS)
        .map_err(map_config_error)?
        .set_default("jwks_cache_ttl_secs", DEFAULT_JWKS_CACHE_TTL_SECS)
        .map_err(map_config_error)?
        .set_default("browser_session_ttl_secs", DEFAULT_BROWSER_SESSION_TTL_SECS)
        .map_err(map_config_error)?
        .set_default(
            "browser_session_max_ttl_secs",
            DEFAULT_BROWSER_SESSION_MAX_TTL_SECS,
        )
        .map_err(map_config_error)?
        .set_default("openapi_enabled", DEFAULT_OPENAPI_ENABLED)
        .map_err(map_config_error)?
        .set_default("metrics_enabled", DEFAULT_METRICS_ENABLED)
        .map_err(map_config_error)?
        .set_default(
            "http_rate_limits.ingress.requests_per_second",
            rate_limits.ingress.requests_per_second,
        )
        .map_err(map_config_error)?
        .set_default("http_rate_limits.ingress.burst", rate_limits.ingress.burst)
        .map_err(map_config_error)?
        .set_default(
            "http_rate_limits.browser_v1.requests_per_second",
            rate_limits.browser_v1.requests_per_second,
        )
        .map_err(map_config_error)?
        .set_default(
            "http_rate_limits.browser_v1.burst",
            rate_limits.browser_v1.burst,
        )
        .map_err(map_config_error)?
        .set_default(
            "http_rate_limits.public_v2.requests_per_second",
            rate_limits.public_v2.requests_per_second,
        )
        .map_err(map_config_error)?
        .set_default(
            "http_rate_limits.public_v2.burst",
            rate_limits.public_v2.burst,
        )
        .map_err(map_config_error)?
        .set_default(
            "http_rate_limits.mcp.requests_per_second",
            rate_limits.mcp.requests_per_second,
        )
        .map_err(map_config_error)?
        .set_default("http_rate_limits.mcp.burst", rate_limits.mcp.burst)
        .map_err(map_config_error)?
        .set_default(
            "http_rate_limits.mcp_v2.requests_per_second",
            rate_limits.mcp_v2.requests_per_second,
        )
        .map_err(map_config_error)?
        .set_default("http_rate_limits.mcp_v2.burst", rate_limits.mcp_v2.burst)
        .map_err(map_config_error)?;

    if include_files {
        builder = builder
            .add_source(File::new("config/default", FileFormat::Toml).required(false))
            .add_source(File::new("config/local", FileFormat::Toml).required(false));
    }

    let mut config = builder
        .add_source(
            environment
                .separator("__")
                .prefix_separator("_")
                .try_parsing(true),
        )
        .build()
        .map_err(map_config_error)?
        .try_deserialize::<Config>()
        .map_err(map_config_error)?;

    config.normalize();
    config.validate().map_err(map_validation_error)?;
    Ok(config)
}

fn map_config_error(error: config::ConfigError) -> Error {
    Error::validation(format!("configuration error: {error}"))
}

fn trim_trailing_slashes(value: &str) -> String {
    value.trim_end_matches('/').to_owned()
}

fn duration_from_secs<'de, D>(deserializer: D) -> std::result::Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Duration::from_secs(u64::deserialize(deserializer)?))
}

fn default_user_tier() -> UserTier {
    UserTier::DEFAULT
}

fn default_search_body_cache_max_capacity_bytes() -> u64 {
    DEFAULT_SEARCH_BODY_CACHE_MAX_CAPACITY_BYTES
}

fn default_search_body_cache_ttl() -> Duration {
    Duration::from_secs(DEFAULT_SEARCH_BODY_CACHE_TTL_SECS)
}

fn default_search_body_cache_tti() -> Duration {
    Duration::from_secs(DEFAULT_SEARCH_BODY_CACHE_TTI_SECS)
}

fn user_tier_from_str<'de, D>(deserializer: D) -> std::result::Result<UserTier, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    UserTier::parse(&value).ok_or_else(|| {
        serde::de::Error::custom("default_user_tier must be `tier0` or `system_max`")
    })
}

fn validate_http_url_value(value: &str) -> std::result::Result<(), ValidationError> {
    let url = Url::parse(value).map_err(|_error| ValidationError::new("http_url"))?;
    let allowed_scheme = matches!(url.scheme(), "http" | "https");
    if allowed_scheme && url.host_str().is_some() {
        Ok(())
    } else {
        Err(ValidationError::new("http_url"))
    }
}

fn validate_jwks_cache_ttl(value: &Duration) -> std::result::Result<(), ValidationError> {
    let seconds = value.as_secs();
    if (30..=3600).contains(&seconds) {
        Ok(())
    } else {
        Err(ValidationError::new("range"))
    }
}

fn validate_browser_session_ttl(value: &Duration) -> std::result::Result<(), ValidationError> {
    let seconds = value.as_secs();
    if (60..=86_400).contains(&seconds) {
        Ok(())
    } else {
        Err(ValidationError::new("range"))
    }
}

fn validate_browser_session_max_ttl(value: &Duration) -> std::result::Result<(), ValidationError> {
    let seconds = value.as_secs();
    if (86_400..=30 * 86_400).contains(&seconds) {
        Ok(())
    } else {
        Err(ValidationError::new("range"))
    }
}

fn validate_limits(limits: &Limits, errors: &mut ValidationErrors) {
    if limits.space_max_nodes == 0 {
        errors.add("limits.space_max_nodes", ValidationError::new("range"));
    }
    if limits.space_max_nodes > crate::limits::SPACE_MAX_NODES {
        errors.add("limits.space_max_nodes", ValidationError::new("range"));
    }
    if limits.space_max_text_bytes == 0 {
        errors.add("limits.space_max_text_bytes", ValidationError::new("range"));
    }
    if limits.space_max_text_bytes > crate::limits::SPACE_MAX_TEXT_BYTES {
        errors.add("limits.space_max_text_bytes", ValidationError::new("range"));
    }
    if limits.space_max_file_bytes == 0 {
        errors.add("limits.space_max_file_bytes", ValidationError::new("range"));
    }
    if limits.space_max_file_bytes > crate::limits::SPACE_MAX_FILE_BYTES {
        errors.add("limits.space_max_file_bytes", ValidationError::new("range"));
    }
    if limits.folder_max_children == 0 {
        errors.add("limits.folder_max_children", ValidationError::new("range"));
    }
    if limits.folder_max_children > crate::limits::FOLDER_MAX_CHILDREN {
        errors.add("limits.folder_max_children", ValidationError::new("range"));
    }
}

fn validate_http_rate_limits(rate_limits: &HttpRateLimitsConfig, errors: &mut ValidationErrors) {
    for (requests_field, burst_field, limit) in [
        (
            "http_rate_limits.ingress.requests_per_second",
            "http_rate_limits.ingress.burst",
            rate_limits.ingress,
        ),
        (
            "http_rate_limits.browser_v1.requests_per_second",
            "http_rate_limits.browser_v1.burst",
            rate_limits.browser_v1,
        ),
        (
            "http_rate_limits.public_v2.requests_per_second",
            "http_rate_limits.public_v2.burst",
            rate_limits.public_v2,
        ),
        (
            "http_rate_limits.mcp.requests_per_second",
            "http_rate_limits.mcp.burst",
            rate_limits.mcp,
        ),
        (
            "http_rate_limits.mcp_v2.requests_per_second",
            "http_rate_limits.mcp_v2.burst",
            rate_limits.mcp_v2,
        ),
    ] {
        if limit.requests_per_second == 0 {
            errors.add(requests_field, ValidationError::new("range"));
        }
        if limit.burst == 0 {
            errors.add(burst_field, ValidationError::new("range"));
        }
    }

    for (requests_field, burst_field, limit) in [
        (
            "http_rate_limits.browser_v1.requests_per_second",
            "http_rate_limits.browser_v1.burst",
            rate_limits.browser_v1,
        ),
        (
            "http_rate_limits.public_v2.requests_per_second",
            "http_rate_limits.public_v2.burst",
            rate_limits.public_v2,
        ),
        (
            "http_rate_limits.mcp.requests_per_second",
            "http_rate_limits.mcp.burst",
            rate_limits.mcp,
        ),
        (
            "http_rate_limits.mcp_v2.requests_per_second",
            "http_rate_limits.mcp_v2.burst",
            rate_limits.mcp_v2,
        ),
    ] {
        if limit.requests_per_second > rate_limits.ingress.requests_per_second {
            errors.add(requests_field, ValidationError::new("exceeds_ingress"));
        }
        if limit.burst > rate_limits.ingress.burst {
            errors.add(burst_field, ValidationError::new("exceeds_ingress"));
        }
    }
}

fn validate_search_body_cache(cache: &SearchBodyCacheConfig, errors: &mut ValidationErrors) {
    if cache.max_capacity_bytes == 0 {
        return;
    }
    for (field, duration) in [
        ("search_body_cache.ttl", cache.ttl),
        ("search_body_cache.tti", cache.tti),
    ] {
        if duration.is_zero() || duration.as_secs() > MAX_SEARCH_BODY_CACHE_EXPIRY_SECS {
            errors.add(field, ValidationError::new("range"));
        }
    }
}

fn validate_secret_min_32(value: &SecretString) -> std::result::Result<(), ValidationError> {
    if value.expose_secret().len() >= 32 {
        Ok(())
    } else {
        Err(ValidationError::new("length"))
    }
}

fn validate_key_id(value: &str) -> std::result::Result<(), ValidationError> {
    let valid = !value.is_empty()
        && value.len() <= 127
        && value.bytes().enumerate().all(|(idx, byte)| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => true,
            b'.' | b'_' | b'-' => idx > 0,
            _ => false,
        });
    if valid {
        Ok(())
    } else {
        Err(ValidationError::new("format"))
    }
}

fn default_true() -> bool {
    true
}

fn secure_cookies_for_redirect(oauth_redirect_url: &str) -> bool {
    oauth_redirect_url.starts_with("https://")
}

fn map_validation_error(error: validator::ValidationErrors) -> Error {
    let mut fields = error
        .field_errors()
        .into_iter()
        .flat_map(|(field, errors)| {
            errors
                .iter()
                .map(move |error| format!("{field}:{}", error.code))
        })
        .collect::<Vec<_>>();
    fields.sort();

    if fields.is_empty() {
        Error::validation("configuration validation error")
    } else {
        Error::validation(format!(
            "configuration validation error: {}",
            fields.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::time::Duration;

    use config::Environment;
    use secrecy::{ExposeSecret, SecretString};
    use validator::Validate;

    use crate::limits::Limits;
    use crate::tier::UserTier;

    use super::{
        Config, HttpRateLimitConfig, HttpRateLimitsConfig, SearchBodyCacheConfig, load_from_sources,
    };

    fn valid_config() -> Config {
        Config {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 9191)),
            database_url: "postgres://example".to_owned(),
            db_max_connections: 10,
            authgate_url: "https://auth.test".to_owned(),
            notegate_public_url: "http://localhost:9191".to_owned(),
            oauth_client_id: "notegate-web".to_owned(),
            mcp_oauth_client_id: "notegate-mcp".to_owned(),
            oauth_redirect_url: "http://localhost:9191/auth/callback".to_owned(),
            resource_url: "http://localhost:9191/mcp".to_owned(),
            jwks_cache_ttl: Duration::from_secs(300),
            enc_root_key_id: "test-enc".to_owned(),
            enc_root_secret: SecretString::from("test-enc-root-secret-32-bytes-long".to_owned()),
            lookup_root_key_id: "test-lookup".to_owned(),
            lookup_root_secret: SecretString::from(
                "test-lookup-root-secret-32-bytes-long".to_owned(),
            ),
            lookup_verify_0_key_id: None,
            lookup_verify_0_secret: None,
            browser_session_ttl: Duration::from_secs(3600),
            browser_session_max_ttl: Duration::from_secs(
                super::DEFAULT_BROWSER_SESSION_MAX_TTL_SECS,
            ),
            openapi_enabled: false,
            metrics_enabled: false,
            web_dist_dir: None,
            s3: super::S3Config {
                endpoint: "http://localhost:9000".to_owned(),
                public_endpoint: None,
                region: "us-east-1".to_owned(),
                bucket: "notegate".to_owned(),
                access_key: "notegate-test".to_owned(),
                secret_key: SecretString::from("notegate-test-secret".to_owned()),
                force_path_style: true,
            },
            default_user_tier: UserTier::DEFAULT,
            limits: Limits::default(),
            http_rate_limits: HttpRateLimitsConfig::default(),
            search_body_cache: SearchBodyCacheConfig::default(),
            secure_cookies: false,
        }
    }

    fn test_env(vars: &[(&str, &str)]) -> Environment {
        let mut values = HashMap::from([
            (
                "NOTEGATE_S3__ENDPOINT".to_owned(),
                "http://localhost:9000".to_owned(),
            ),
            ("NOTEGATE_S3__REGION".to_owned(), "us-east-1".to_owned()),
            ("NOTEGATE_S3__BUCKET".to_owned(), "notegate".to_owned()),
            (
                "NOTEGATE_S3__ACCESS_KEY".to_owned(),
                "notegate-test".to_owned(),
            ),
            (
                "NOTEGATE_S3__SECRET_KEY".to_owned(),
                "notegate-test-secret".to_owned(),
            ),
            (
                "NOTEGATE_S3__FORCE_PATH_STYLE".to_owned(),
                "true".to_owned(),
            ),
        ]);
        values.extend(
            vars.iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
        );
        Environment::with_prefix("NOTEGATE").source(Some(values))
    }

    #[test]
    fn environment_layer_accepts_web_dist_dir() -> crate::Result<()> {
        let config = load_from_sources(
            false,
            test_env(&[
                ("NOTEGATE_DATABASE_URL", "postgres://env"),
                ("NOTEGATE_AUTHGATE_URL", "https://auth.env"),
                ("NOTEGATE_PUBLIC_URL", "http://localhost:9191"),
                ("NOTEGATE_OAUTH_CLIENT_ID", "notegate-web"),
                ("NOTEGATE_MCP_OAUTH_CLIENT_ID", "notegate-mcp"),
                ("NOTEGATE_ENC_ROOT_KEY_ID", "env-enc"),
                (
                    "NOTEGATE_ENC_ROOT_SECRET",
                    "env-enc-root-secret-32-bytes-long",
                ),
                ("NOTEGATE_LOOKUP_ROOT_KEY_ID", "env-lookup"),
                (
                    "NOTEGATE_LOOKUP_ROOT_SECRET",
                    "env-lookup-root-secret-32-bytes-long",
                ),
                ("NOTEGATE_WEB_DIST_DIR", "/app/web"),
            ]),
        )?;

        assert_eq!(config.web_dist_dir.as_deref(), Some("/app/web"));
        assert_eq!(config.default_user_tier, UserTier::Tier0);
        Ok(())
    }

    #[test]
    fn environment_layer_accepts_prefixed_variable_names() -> crate::Result<()> {
        let config = load_from_sources(
            false,
            test_env(&[
                ("NOTEGATE_DATABASE_URL", "postgres://env"),
                ("NOTEGATE_AUTHGATE_URL", "https://auth.env"),
                ("NOTEGATE_PUBLIC_URL", "http://localhost:9191"),
                ("NOTEGATE_OAUTH_CLIENT_ID", "notegate-web"),
                ("NOTEGATE_MCP_OAUTH_CLIENT_ID", "notegate-mcp"),
                ("NOTEGATE_ENC_ROOT_KEY_ID", "env-enc"),
                (
                    "NOTEGATE_ENC_ROOT_SECRET",
                    "env-enc-root-secret-32-bytes-long",
                ),
                ("NOTEGATE_LOOKUP_ROOT_KEY_ID", "env-lookup"),
                (
                    "NOTEGATE_LOOKUP_ROOT_SECRET",
                    "env-lookup-root-secret-32-bytes-long",
                ),
                ("NOTEGATE_DB_MAX_CONNECTIONS", "7"),
                ("NOTEGATE_METRICS_ENABLED", "true"),
                ("NOTEGATE_DEFAULT_USER_TIER", "tier0"),
                ("NOTEGATE_S3__ENDPOINT", "https://s3.internal.env/"),
                ("NOTEGATE_S3__PUBLIC_ENDPOINT", "https://s3.public.env/"),
                ("NOTEGATE_S3__REGION", "us-east-1"),
                ("NOTEGATE_S3__BUCKET", "notegate"),
                ("NOTEGATE_S3__ACCESS_KEY", "notegate-access"),
                ("NOTEGATE_S3__SECRET_KEY", "notegate-secret"),
                ("NOTEGATE_S3__FORCE_PATH_STYLE", "true"),
                ("PATH", "/bin"),
                ("DATABASE_URL", "postgres://ignored"),
            ]),
        )?;

        assert_eq!(config.bind_addr.to_string(), super::DEFAULT_BIND_ADDR);
        assert_eq!(config.database_url, "postgres://env");
        assert_eq!(config.db_max_connections, 7);
        assert!(config.metrics_enabled);
        assert_eq!(config.oauth_client_id, "notegate-web");
        assert_eq!(config.mcp_oauth_client_id, "notegate-mcp");
        assert_eq!(
            config.oauth_redirect_url,
            "http://localhost:9191/auth/callback"
        );
        assert_eq!(config.resource_url, "http://localhost:9191/mcp");
        assert_eq!(config.default_user_tier, UserTier::Tier0);
        let s3 = &config.s3;
        assert_eq!(s3.endpoint, "https://s3.internal.env");
        assert_eq!(s3.public_endpoint.as_deref(), Some("https://s3.public.env"));
        assert_eq!(s3.region, "us-east-1");
        assert_eq!(s3.bucket, "notegate");
        assert_eq!(s3.access_key, "notegate-access");
        assert_eq!(s3.secret_key.expose_secret(), "notegate-secret");
        assert!(s3.force_path_style);
        assert_eq!(
            config.jwks_cache_ttl.as_secs(),
            super::DEFAULT_JWKS_CACHE_TTL_SECS
        );
        assert_eq!(
            config.browser_session_ttl.as_secs(),
            super::DEFAULT_BROWSER_SESSION_TTL_SECS
        );
        assert_eq!(
            config.browser_session_max_ttl.as_secs(),
            super::DEFAULT_BROWSER_SESSION_MAX_TTL_SECS
        );
        assert_eq!(config.limits, Limits::default());
        assert_eq!(config.http_rate_limits, HttpRateLimitsConfig::default());
        assert_eq!(config.search_body_cache, SearchBodyCacheConfig::default());
        Ok(())
    }

    #[test]
    fn environment_layer_accepts_nested_overrides() -> crate::Result<()> {
        let config = load_from_sources(
            false,
            test_env(&[
                ("NOTEGATE_DATABASE_URL", "postgres://env"),
                ("NOTEGATE_AUTHGATE_URL", "https://auth.env"),
                ("NOTEGATE_PUBLIC_URL", "http://localhost:9191"),
                ("NOTEGATE_OAUTH_CLIENT_ID", "notegate-web"),
                ("NOTEGATE_MCP_OAUTH_CLIENT_ID", "notegate-mcp"),
                ("NOTEGATE_ENC_ROOT_KEY_ID", "env-enc"),
                (
                    "NOTEGATE_ENC_ROOT_SECRET",
                    "env-enc-root-secret-32-bytes-long",
                ),
                ("NOTEGATE_LOOKUP_ROOT_KEY_ID", "env-lookup"),
                (
                    "NOTEGATE_LOOKUP_ROOT_SECRET",
                    "env-lookup-root-secret-32-bytes-long",
                ),
                ("NOTEGATE_LIMITS__FOLDER_MAX_CHILDREN", "3"),
                ("NOTEGATE_LIMITS__SPACE_MAX_NODES", "5"),
                ("NOTEGATE_LIMITS__SPACE_MAX_TEXT_BYTES", "1024"),
                ("NOTEGATE_LIMITS__SPACE_MAX_FILE_BYTES", "2048"),
                (
                    "NOTEGATE_HTTP_RATE_LIMITS__INGRESS__REQUESTS_PER_SECOND",
                    "210",
                ),
                ("NOTEGATE_HTTP_RATE_LIMITS__INGRESS__BURST", "230"),
                (
                    "NOTEGATE_HTTP_RATE_LIMITS__BROWSER_V1__REQUESTS_PER_SECOND",
                    "60",
                ),
                ("NOTEGATE_HTTP_RATE_LIMITS__BROWSER_V1__BURST", "70"),
                (
                    "NOTEGATE_HTTP_RATE_LIMITS__PUBLIC_V2__REQUESTS_PER_SECOND",
                    "80",
                ),
                ("NOTEGATE_HTTP_RATE_LIMITS__PUBLIC_V2__BURST", "90"),
                ("NOTEGATE_HTTP_RATE_LIMITS__MCP__REQUESTS_PER_SECOND", "17"),
                ("NOTEGATE_HTTP_RATE_LIMITS__MCP__BURST", "23"),
                (
                    "NOTEGATE_HTTP_RATE_LIMITS__MCP_V2__REQUESTS_PER_SECOND",
                    "19",
                ),
                ("NOTEGATE_HTTP_RATE_LIMITS__MCP_V2__BURST", "29"),
                (
                    "NOTEGATE_SEARCH_BODY_CACHE__MAX_CAPACITY_BYTES",
                    "268435456",
                ),
                ("NOTEGATE_SEARCH_BODY_CACHE__TTL_SECS", "3600"),
                ("NOTEGATE_SEARCH_BODY_CACHE__TTI_SECS", "600"),
            ]),
        )?;

        assert_eq!(config.limits.folder_max_children, 3);
        assert_eq!(config.limits.space_max_nodes, 5);
        assert_eq!(config.limits.space_max_text_bytes, 1024);
        assert_eq!(config.limits.space_max_file_bytes, 2048);
        assert_eq!(
            config.http_rate_limits,
            HttpRateLimitsConfig {
                ingress: HttpRateLimitConfig {
                    requests_per_second: 210,
                    burst: 230,
                },
                browser_v1: HttpRateLimitConfig {
                    requests_per_second: 60,
                    burst: 70,
                },
                public_v2: HttpRateLimitConfig {
                    requests_per_second: 80,
                    burst: 90,
                },
                mcp: HttpRateLimitConfig {
                    requests_per_second: 17,
                    burst: 23,
                },
                mcp_v2: HttpRateLimitConfig {
                    requests_per_second: 19,
                    burst: 29,
                },
            }
        );
        assert_eq!(
            config.search_body_cache,
            SearchBodyCacheConfig {
                max_capacity_bytes: 256 * 1024 * 1024,
                ttl: Duration::from_secs(3600),
                tti: Duration::from_secs(600),
            }
        );
        Ok(())
    }

    #[test]
    fn environment_layer_rejects_unknown_default_user_tier() {
        let result = load_from_sources(
            false,
            test_env(&[
                ("NOTEGATE_DATABASE_URL", "postgres://env"),
                ("NOTEGATE_AUTHGATE_URL", "https://auth.env"),
                ("NOTEGATE_PUBLIC_URL", "http://localhost:9191"),
                ("NOTEGATE_OAUTH_CLIENT_ID", "notegate-web"),
                ("NOTEGATE_MCP_OAUTH_CLIENT_ID", "notegate-mcp"),
                ("NOTEGATE_ENC_ROOT_KEY_ID", "env-enc"),
                (
                    "NOTEGATE_ENC_ROOT_SECRET",
                    "env-enc-root-secret-32-bytes-long",
                ),
                ("NOTEGATE_LOOKUP_ROOT_KEY_ID", "env-lookup"),
                (
                    "NOTEGATE_LOOKUP_ROOT_SECRET",
                    "env-lookup-root-secret-32-bytes-long",
                ),
                ("NOTEGATE_DEFAULT_USER_TIER", "enterprise"),
            ]),
        );

        assert!(result.is_err());
    }

    #[test]
    fn normalize_builds_valid_config() -> crate::Result<()> {
        let mut config = valid_config();
        config.validate().map_err(super::map_validation_error)?;
        config.normalize();
        assert_eq!(config.bind_addr.to_string(), "127.0.0.1:9191");
        assert_eq!(config.db_max_connections, 10);
        assert_eq!(config.jwks_cache_ttl.as_secs(), 300);
        assert_eq!(config.browser_session_ttl.as_secs(), 3600);
        assert!(!config.openapi_enabled);
        assert!(!config.metrics_enabled);
        assert!(!config.secure_cookies);
        Ok(())
    }

    #[test]
    fn validate_rejects_out_of_range_values() {
        let mut config = valid_config();
        config.db_max_connections = 0;
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.jwks_cache_ttl = Duration::from_secs(1);
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.enc_root_secret = SecretString::from("too-short".to_owned());
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.lookup_root_key_id = "_bad".to_owned();
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.browser_session_ttl = Duration::from_secs(1);
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.browser_session_max_ttl = Duration::from_secs(1);
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.browser_session_ttl = Duration::from_secs(86_400);
        config.browser_session_max_ttl = Duration::from_secs(3600);
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.limits.space_max_nodes = crate::limits::SPACE_MAX_NODES + 1;
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.limits.space_max_text_bytes = crate::limits::SPACE_MAX_TEXT_BYTES + 1;
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.limits.space_max_file_bytes = crate::limits::SPACE_MAX_FILE_BYTES + 1;
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.limits.folder_max_children = crate::limits::FOLDER_MAX_CHILDREN + 1;
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.http_rate_limits.mcp.requests_per_second = 0;
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.http_rate_limits.mcp_v2.requests_per_second = 0;
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.http_rate_limits.public_v2.burst = config.http_rate_limits.ingress.burst + 1;
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.search_body_cache.ttl = Duration::ZERO;
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.search_body_cache.max_capacity_bytes = 0;
        config.search_body_cache.ttl = Duration::ZERO;
        config.search_body_cache.tti = Duration::ZERO;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_rejects_reused_root_key_id_and_secret() {
        // Equal key-ids are rejected.
        let mut config = valid_config();
        config.lookup_root_key_id = config.enc_root_key_id.clone();
        assert!(config.validate().is_err());

        // Equal secrets are rejected.
        let mut config = valid_config();
        config.lookup_root_secret =
            SecretString::from("test-enc-root-secret-32-bytes-long".to_owned());
        assert!(
            config.lookup_root_secret.expose_secret() == config.enc_root_secret.expose_secret()
        );
        assert!(config.validate().is_err());

        // Distinct ids + secrets pass.
        let config = valid_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn env_example_uses_distinct_root_secret_placeholders() {
        let example = include_str!("../../../../.env.example");
        let secret_line = |key: &str| -> String {
            example
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .map(str::to_owned)
                .unwrap_or_default()
        };
        let enc = secret_line("NOTEGATE_ENC_ROOT_SECRET=");
        let lookup = secret_line("NOTEGATE_LOOKUP_ROOT_SECRET=");
        assert!(!enc.is_empty() && !lookup.is_empty());
        assert_ne!(
            enc, lookup,
            ".env.example ENC/LOOKUP root secrets must be distinct"
        );
    }

    #[test]
    fn validation_errors_do_not_echo_values() -> crate::Result<()> {
        let mut config = valid_config();
        config.authgate_url = "not a url with secret-token".to_owned();

        let err = match config.validate().map_err(super::map_validation_error) {
            Ok(()) => {
                return Err(crate::Error::validation(
                    "invalid URL should fail validation",
                ));
            }
            Err(err) => err,
        };
        let msg = err.to_string();
        assert!(msg.contains("authgate_url:http_url"));
        assert!(!msg.contains("secret-token"));
        Ok(())
    }
}
