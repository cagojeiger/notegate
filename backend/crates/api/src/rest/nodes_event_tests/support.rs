#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::Extension;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use notegate_core::Config;
use notegate_core::security::PiiCrypto;
use notegate_db::{AccountRepo, SpaceRepo, test_support::TestDb};
use notegate_model::{Caller, CallerIdentity, Channel, ResolveAttrs};
use notegate_service::spaces::CreateSpace;
use secrecy::SecretString;
use serde_json::Value;
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::auth::jwt::JwtAuthority;
use crate::auth::oidc::OidcProvider;
use crate::identity::{CallerResolver, IdentityError};

#[derive(Clone)]
struct UnusedResolver;

impl CallerResolver for UnusedResolver {
    fn resolve_browser(
        &self,
        _attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async { Err(IdentityError::NotRegistered) })
    }

    fn resolve_browser_session_user(
        &self,
        _user_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async { Err(IdentityError::NotRegistered) })
    }

    fn resolve_api(
        &self,
        _attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async { Err(IdentityError::NotRegistered) })
    }

    fn resolve_mcp(
        &self,
        _attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async { Err(IdentityError::NotRegistered) })
    }

    fn resolve_api_key(
        &self,
        _token: String,
        _channel: Channel,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async { Err(IdentityError::NotRegistered) })
    }
}

fn test_config() -> Arc<Config> {
    Arc::new(Config {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9191),
        database_url: "postgres://notegate:notegate@localhost/notegate".to_owned(),
        db_max_connections: 1,
        authgate_url: "https://auth.example.test".to_owned(),
        notegate_public_url: "http://localhost:9191".to_owned(),
        oauth_client_id: "notegate-web".to_owned(),
        mcp_oauth_client_id: "notegate-mcp".to_owned(),
        oauth_redirect_url: "http://localhost:9191/auth/callback".to_owned(),
        resource_url: "https://api.example.test".to_owned(),
        jwks_cache_ttl: Duration::from_secs(300),
        enc_root_key_id: "test-enc".to_owned(),
        enc_root_secret: SecretString::from("test-enc-root-secret-32-bytes-long".to_owned()),
        lookup_root_key_id: "test-lookup".to_owned(),
        lookup_root_secret: SecretString::from("test-lookup-root-secret-32-bytes-long".to_owned()),
        lookup_verify_0_key_id: None,
        lookup_verify_0_secret: None,
        browser_session_ttl: Duration::from_secs(3600),
        browser_session_max_ttl: Duration::from_secs(30 * 86_400),
        openapi_enabled: false,
        web_dist_dir: None,
        default_user_tier: notegate_core::tier::UserTier::DEFAULT,
        limits: notegate_core::limits::Limits::default(),
        secure_cookies: false,
    })
}

pub(super) fn state(db: &TestDb) -> crate::state::AppState {
    let config = test_config();
    let security = PiiCrypto::from_root_secrets(
        config.enc_root_key_id.clone(),
        &config.enc_root_secret,
        config.lookup_root_key_id.clone(),
        &config.lookup_root_secret,
    )
    .expect("derive test crypto");
    notegate_service::cursor::configure_signing_key(security.session_signing_key())
        .expect("configure cursor signing key");
    let jwt = Arc::new(JwtAuthority::from_jwks(&config, aliri::Jwks::default()));
    let oidc = Arc::new(OidcProvider::new(&config, reqwest::Client::new()));
    crate::state::AppState::new(
        db.pool.clone(),
        config,
        jwt,
        oidc,
        Arc::new(UnusedResolver),
        reqwest::Client::new(),
        security,
    )
}

pub(super) async fn caller_and_space(
    state: &crate::state::AppState,
) -> Result<(Caller, Uuid, Uuid), Box<dyn std::error::Error>> {
    let (account, user) = AccountRepo::with_crypto_and_default_user_tier(
        state.db.clone(),
        state.security.clone(),
        state.config.default_user_tier,
    )
    .upsert_user_by_sub(&ResolveAttrs {
        sub: "rest-events-owner".to_owned(),
        email: "rest-events@example.test".to_owned(),
        name: "REST Events Owner".to_owned(),
    })
    .await?;
    let space = SpaceRepo::new(state.db.clone())
        .create_space(
            account.id,
            &CreateSpace {
                name: "rest-events".to_owned(),
            },
        )
        .await?;
    let root = SpaceRepo::new(state.db.clone())
        .root_node_id(space.id)
        .await?
        .expect("root node");
    Ok((
        Caller {
            account,
            identity: CallerIdentity::User(user),
            channel: Channel::Browser,
        },
        space.id,
        root,
    ))
}

pub(super) fn rest_app(state: crate::state::AppState, caller: Caller) -> Router {
    Router::new()
        .merge(super::super::routes())
        .merge(crate::rest::text::routes())
        .merge(crate::rest::files::routes())
        .layer(Extension(caller))
        .with_state(state)
}

pub(super) async fn json_request(
    app: Router,
    method: &str,
    uri: String,
    body: Value,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))?,
        )
        .await?;
    decode_response(response).await
}

pub(super) async fn get_json(
    app: Router,
    uri: String,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty())?)
        .await?;
    decode_response(response).await
}

pub(super) async fn empty_request(
    app: Router,
    method: &str,
    uri: String,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())?,
        )
        .await?;
    decode_response(response).await
}

pub(super) async fn upload_file(
    app: Router,
    space_id: Uuid,
    parent_id: Uuid,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    let boundary = "notegate-test-boundary";
    let body = format!(
        "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"parent_node_id\"\r\n\r\n\
             {parent_id}\r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"name\"\r\n\r\n\
             asset.txt\r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"media_type\"\r\n\r\n\
             text/plain\r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"asset.txt\"\r\n\
             Content-Type: text/plain\r\n\r\n\
             asset\r\n\
             --{boundary}--\r\n"
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/spaces/{space_id}/files"))
                .header(
                    CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))?,
        )
        .await?;
    decode_response(response).await
}

async fn decode_response(
    response: Response,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok((status, value))
}
