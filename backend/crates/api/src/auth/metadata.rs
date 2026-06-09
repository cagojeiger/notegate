use axum::Json;
use axum::http::HeaderValue;
use notegate_core::Config;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedResourceUrl {
    pub route_path: String,
    pub full_url: String,
}

pub fn protected_resource_metadata_url(resource_url: &str) -> ProtectedResourceUrl {
    let trimmed = resource_url.trim_end_matches('/');
    let (origin, resource_path) = split_origin_and_path(trimmed);
    let path = resource_path.trim_matches('/');
    let route_path = if path.is_empty() {
        "/.well-known/oauth-protected-resource".to_owned()
    } else {
        format!("/.well-known/oauth-protected-resource/{path}")
    };
    let full_url = format!("{origin}{route_path}");
    ProtectedResourceUrl {
        route_path,
        full_url,
    }
}

fn split_origin_and_path(value: &str) -> (&str, &str) {
    let Some((scheme, rest)) = value.split_once("://") else {
        return (value, "");
    };
    if let Some((host, path)) = rest.split_once('/') {
        let origin_len = scheme.len() + 3 + host.len();
        (&value[..origin_len], path)
    } else {
        (value, "")
    }
}

pub fn challenge_header(meta_url: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("Bearer resource_metadata=\"{meta_url}\""))
        .unwrap_or_else(|_error| HeaderValue::from_static("Bearer"))
}

pub fn scoped_challenge_header(meta_url: &str) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "Bearer resource_metadata=\"{meta_url}\", scope=\"openid offline_access\""
    ))
    .unwrap_or_else(|_error| HeaderValue::from_static("Bearer"))
}

#[derive(Debug, Serialize)]
pub struct AuthorizationServerMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    revocation_endpoint: String,
    device_authorization_endpoint: String,
    response_types_supported: Vec<&'static str>,
    grant_types_supported: Vec<&'static str>,
    token_endpoint_auth_methods_supported: Vec<&'static str>,
    code_challenge_methods_supported: Vec<&'static str>,
    scopes_supported: Vec<&'static str>,
    client_id_metadata_document_supported: bool,
}

#[derive(Debug, Serialize)]
pub struct ProtectedResourceMetadata {
    resource: String,
    authorization_servers: Vec<String>,
    scopes_supported: Vec<&'static str>,
    bearer_methods_supported: Vec<&'static str>,
    /// Static public OAuth client id registered for MCP clients in authgate.
    /// This is an extension field; generic clients may ignore it.
    mcp_client_id: String,
}

pub async fn authorization_server_metadata(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
) -> Json<AuthorizationServerMetadata> {
    Json(authorization_server_metadata_for_config(&state.config))
}

pub fn authorization_server_metadata_for_config(config: &Config) -> AuthorizationServerMetadata {
    let issuer = config.authgate_url.trim_end_matches('/').to_owned();
    AuthorizationServerMetadata {
        issuer: issuer.clone(),
        authorization_endpoint: format!("{issuer}/authorize"),
        token_endpoint: format!("{issuer}/oauth/token"),
        revocation_endpoint: format!("{issuer}/oauth/revoke"),
        device_authorization_endpoint: format!("{issuer}/oauth/device/authorize"),
        response_types_supported: vec!["code"],
        grant_types_supported: vec![
            "authorization_code",
            "refresh_token",
            "urn:ietf:params:oauth:grant-type:device_code",
        ],
        token_endpoint_auth_methods_supported: vec![
            "none",
            "client_secret_basic",
            "client_secret_post",
        ],
        code_challenge_methods_supported: vec!["S256"],
        scopes_supported: vec!["openid", "profile", "email", "offline_access"],
        client_id_metadata_document_supported: true,
    }
}

pub async fn protected_resource_metadata(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
) -> Json<ProtectedResourceMetadata> {
    Json(protected_resource_metadata_for_config(&state.config))
}

pub fn protected_resource_metadata_for_config(config: &Config) -> ProtectedResourceMetadata {
    ProtectedResourceMetadata {
        resource: config.resource_url.clone(),
        authorization_servers: vec![config.authgate_url.clone()],
        scopes_supported: vec!["openid", "profile", "email", "offline_access"],
        bearer_methods_supported: vec!["header"],
        mcp_client_id: config.mcp_oauth_client_id.clone(),
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
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    use secrecy::SecretString;

    use super::{
        authorization_server_metadata_for_config, challenge_header,
        protected_resource_metadata_url, scoped_challenge_header,
    };

    fn config() -> notegate_core::Config {
        notegate_core::Config {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9191),
            database_url: "postgres://example".to_owned(),
            db_max_connections: 10,
            authgate_url: "https://auth.example.test".to_owned(),
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
            openapi_enabled: false,
            limits: notegate_core::limits::Limits::default(),
            secure_cookies: false,
        }
    }

    #[test]
    fn metadata_url_for_root_resource() {
        let got = protected_resource_metadata_url("http://localhost:9191");
        assert_eq!(got.route_path, "/.well-known/oauth-protected-resource");
        assert_eq!(
            got.full_url,
            "http://localhost:9191/.well-known/oauth-protected-resource"
        );
    }

    #[test]
    fn metadata_url_for_mcp_resource() {
        let got = protected_resource_metadata_url("http://localhost:9191/mcp");
        assert_eq!(got.route_path, "/.well-known/oauth-protected-resource/mcp");
        assert_eq!(
            got.full_url,
            "http://localhost:9191/.well-known/oauth-protected-resource/mcp"
        );
    }

    #[test]
    fn authorization_server_metadata_matches_authgate_endpoints() {
        let metadata = authorization_server_metadata_for_config(&config());
        assert_eq!(metadata.issuer, "https://auth.example.test");
        assert_eq!(
            metadata.authorization_endpoint,
            "https://auth.example.test/authorize"
        );
        assert_eq!(
            metadata.token_endpoint,
            "https://auth.example.test/oauth/token"
        );
        assert_eq!(
            metadata.revocation_endpoint,
            "https://auth.example.test/oauth/revoke"
        );
        assert_eq!(
            metadata.device_authorization_endpoint,
            "https://auth.example.test/oauth/device/authorize"
        );
        assert_eq!(metadata.response_types_supported, vec!["code"]);
        assert_eq!(
            metadata.grant_types_supported,
            vec![
                "authorization_code",
                "refresh_token",
                "urn:ietf:params:oauth:grant-type:device_code"
            ]
        );
        assert_eq!(
            metadata.token_endpoint_auth_methods_supported,
            vec!["none", "client_secret_basic", "client_secret_post"]
        );
        assert_eq!(metadata.code_challenge_methods_supported, vec!["S256"]);
        assert!(metadata.client_id_metadata_document_supported);
    }

    #[test]
    fn bearer_challenge_includes_resource_metadata() {
        let header =
            challenge_header("http://localhost:9191/.well-known/oauth-protected-resource/mcp");
        let value = header.to_str().unwrap_or_default();
        assert!(value.starts_with("Bearer "));
        assert!(value.contains("resource_metadata="));
    }

    #[test]
    fn scoped_bearer_challenge_includes_resource_metadata_and_scope_hint() {
        let header = scoped_challenge_header(
            "http://localhost:9191/.well-known/oauth-protected-resource/mcp",
        );
        let value = header.to_str().unwrap_or_default();
        assert!(value.starts_with("Bearer "));
        assert!(value.contains("resource_metadata="));
        assert!(value.contains("scope=\"openid offline_access\""));
    }
}
