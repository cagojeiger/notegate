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
use crate::limits::Limits;
use crate::tier::UserTier;

const DEFAULT_BIND_ADDR: &str = "0.0.0.0:9191";
const DEFAULT_DB_MAX_CONNECTIONS: u32 = 10;
const DEFAULT_JWKS_CACHE_TTL_SECS: u64 = 300;
const DEFAULT_BROWSER_SESSION_TTL_SECS: u64 = 3600;
const DEFAULT_BROWSER_SESSION_MAX_TTL_SECS: u64 = 30 * 86_400;
const DEFAULT_OPENAPI_ENABLED: bool = false;

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
    /// Resource/audience URL for REST and MCP, with trailing slash trimmed.
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
    /// Optional directory containing the built web dashboard. When set, unknown
    /// non-API routes fall back to this directory's `index.html`.
    pub web_dist_dir: Option<String>,
    /// Tier assigned to newly created users.
    #[serde(default = "default_user_tier", deserialize_with = "user_tier_from_str")]
    pub default_user_tier: UserTier,
    /// Runtime-overridable capacity limits. Defaults match `docs/spec/performance-limits.md`.
    #[serde(default)]
    pub limits: Limits,
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
        validate_limits(&self.limits, &mut errors);

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
        self.secure_cookies = secure_cookies_for_redirect(&self.oauth_redirect_url);
    }
}

fn load_from_sources(include_files: bool, environment: Environment) -> Result<Config> {
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

    use super::{Config, load_from_sources};

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
            web_dist_dir: None,
            default_user_tier: UserTier::DEFAULT,
            limits: Limits::default(),
            secure_cookies: false,
        }
    }

    fn test_env(vars: &[(&str, &str)]) -> Environment {
        Environment::with_prefix("NOTEGATE").source(Some(
            vars.iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect::<HashMap<_, _>>(),
        ))
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
                ("NOTEGATE_DEFAULT_USER_TIER", "tier0"),
                ("PATH", "/bin"),
                ("DATABASE_URL", "postgres://ignored"),
            ]),
        )?;

        assert_eq!(config.bind_addr.to_string(), super::DEFAULT_BIND_ADDR);
        assert_eq!(config.database_url, "postgres://env");
        assert_eq!(config.db_max_connections, 7);
        assert_eq!(config.oauth_client_id, "notegate-web");
        assert_eq!(config.mcp_oauth_client_id, "notegate-mcp");
        assert_eq!(
            config.oauth_redirect_url,
            "http://localhost:9191/auth/callback"
        );
        assert_eq!(config.resource_url, "http://localhost:9191/mcp");
        assert_eq!(config.default_user_tier, UserTier::Tier0);
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
        Ok(())
    }

    #[test]
    fn environment_layer_accepts_nested_limit_overrides() -> crate::Result<()> {
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
            ]),
        )?;

        assert_eq!(config.limits.folder_max_children, 3);
        assert_eq!(config.limits.space_max_nodes, 5);
        assert_eq!(config.limits.space_max_text_bytes, 1024);
        assert_eq!(config.limits.space_max_file_bytes, 2048);
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
