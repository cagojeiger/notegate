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

const DEFAULT_BIND_ADDR: &str = "0.0.0.0:9191";
const DEFAULT_DB_MAX_CONNECTIONS: u32 = 10;
const DEFAULT_JWKS_CACHE_TTL_SECS: u64 = 300;
const DEFAULT_BROWSER_SESSION_TTL_SECS: u64 = 3600;
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
    pub oauth_client_id: String,
    /// Public MCP OAuth client id registered in authgate.
    pub mcp_oauth_client_id: String,
    /// Exact redirect URL registered in authgate.
    pub oauth_redirect_url: String,
    /// Resource/audience URL for REST and MCP, with trailing slash trimmed.
    pub resource_url: String,
    /// Shared JWKS cache TTL.
    #[serde(
        rename = "jwks_cache_ttl_secs",
        deserialize_with = "duration_from_secs"
    )]
    pub jwks_cache_ttl: Duration,
    /// Secret used to sign browser session cookies.
    pub browser_session_secret: SecretString,
    /// Browser session cookie TTL.
    #[serde(
        rename = "browser_session_ttl_secs",
        deserialize_with = "duration_from_secs"
    )]
    pub browser_session_ttl: Duration,
    /// Whether OpenAPI JSON and Swagger UI routes are exposed.
    pub openapi_enabled: bool,
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
        if validate_secret_min_32(&self.browser_session_secret).is_err() {
            errors.add("browser_session_secret", ValidationError::new("length"));
        }
        if validate_browser_session_ttl(&self.browser_session_ttl).is_err() {
            errors.add("browser_session_ttl", ValidationError::new("range"));
        }

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
        self.resource_url = trim_trailing_slashes(&self.resource_url);
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
        .set_default("openapi_enabled", DEFAULT_OPENAPI_ENABLED)
        .map_err(map_config_error)?;

    if include_files {
        builder = builder
            .add_source(File::new("config/default", FileFormat::Toml).required(false))
            .add_source(File::new("config/local", FileFormat::Toml).required(false));
    }

    let mut config = builder
        .add_source(environment.try_parsing(true))
        .build()
        .map_err(map_config_error)?
        .try_deserialize::<Config>()
        .map_err(map_config_error)?;

    config.validate().map_err(map_validation_error)?;
    config.normalize();
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

fn validate_secret_min_32(value: &SecretString) -> std::result::Result<(), ValidationError> {
    if value.expose_secret().len() >= 32 {
        Ok(())
    } else {
        Err(ValidationError::new("length"))
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
    use secrecy::SecretString;
    use validator::Validate;

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
            oauth_redirect_url: "http://localhost:9191/callback".to_owned(),
            resource_url: "http://localhost:9191/mcp".to_owned(),
            jwks_cache_ttl: Duration::from_secs(300),
            browser_session_secret: SecretString::from(
                "test-browser-session-secret-32-bytes".to_owned(),
            ),
            browser_session_ttl: Duration::from_secs(3600),
            openapi_enabled: false,
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
    fn environment_layer_accepts_prefixed_variable_names() -> crate::Result<()> {
        let config = load_from_sources(
            false,
            test_env(&[
                ("NOTEGATE_DATABASE_URL", "postgres://env"),
                ("NOTEGATE_AUTHGATE_URL", "https://auth.env"),
                ("NOTEGATE_PUBLIC_URL", "http://localhost:9191"),
                ("NOTEGATE_OAUTH_CLIENT_ID", "notegate-web"),
                ("NOTEGATE_MCP_OAUTH_CLIENT_ID", "notegate-mcp"),
                (
                    "NOTEGATE_OAUTH_REDIRECT_URL",
                    "http://localhost:9191/callback",
                ),
                ("NOTEGATE_RESOURCE_URL", "http://localhost:9191/mcp"),
                (
                    "NOTEGATE_BROWSER_SESSION_SECRET",
                    "env-browser-session-secret-32-bytes",
                ),
                ("NOTEGATE_DB_MAX_CONNECTIONS", "7"),
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
            config.jwks_cache_ttl.as_secs(),
            super::DEFAULT_JWKS_CACHE_TTL_SECS
        );
        assert_eq!(
            config.browser_session_ttl.as_secs(),
            super::DEFAULT_BROWSER_SESSION_TTL_SECS
        );
        Ok(())
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
        config.browser_session_secret = SecretString::from("too-short".to_owned());
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.browser_session_ttl = Duration::from_secs(1);
        assert!(config.validate().is_err());
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
