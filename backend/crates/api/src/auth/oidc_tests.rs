#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use openidconnect::core::CoreRsaPrivateSigningKey;
use openidconnect::{JsonWebKeyId, PrivateSigningKey};
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{RwLock, Semaphore};
use tokio::task::JoinHandle;

use super::{METADATA_CACHE_TTL, OidcProvider};
use crate::auth::oauth_exchange::exchange_code_for_userinfo;

const CLIENT_ID: &str = "notegate-web";
const NONCE: &str = "browser-login-nonce";
const SUBJECT: &str = "test-user";
const ACCESS_TOKEN: &str = "test-access-token";

struct MockProvider {
    issuer: String,
    state: Arc<MockState>,
    server: JoinHandle<()>,
}

struct MockState {
    issuer: String,
    keys: RwLock<Value>,
    id_token: RwLock<String>,
    userinfo_subject: RwLock<String>,
    discovery_requests: AtomicUsize,
    jwks_requests: AtomicUsize,
    token_requests: AtomicUsize,
    userinfo_requests: AtomicUsize,
    fail_jwks: AtomicBool,
    omit_refresh: AtomicBool,
    last_token_form: RwLock<std::collections::HashMap<String, String>>,
    pause_tokens: AtomicBool,
    tokens_started: Semaphore,
    tokens_released: Semaphore,
}

impl MockProvider {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock OIDC server");
        let issuer = format!("http://{}", listener.local_addr().expect("mock address"));
        let state = Arc::new(MockState {
            issuer: issuer.clone(),
            keys: RwLock::new(jwks(KEY_A, "key-a")),
            id_token: RwLock::new(sign_token(
                KEY_A,
                "key-a",
                &issuer,
                json!({}),
                Algorithm::RS256,
            )),
            userinfo_subject: RwLock::new(SUBJECT.to_owned()),
            discovery_requests: AtomicUsize::new(0),
            jwks_requests: AtomicUsize::new(0),
            token_requests: AtomicUsize::new(0),
            userinfo_requests: AtomicUsize::new(0),
            fail_jwks: AtomicBool::new(false),
            omit_refresh: AtomicBool::new(false),
            last_token_form: RwLock::new(Default::default()),
            pause_tokens: AtomicBool::new(false),
            tokens_started: Semaphore::new(0),
            tokens_released: Semaphore::new(0),
        });
        let router = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/keys", get(keys))
            .route("/token", post(token))
            .route("/userinfo", get(userinfo))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("mock OIDC server");
        });
        Self {
            issuer,
            state,
            server,
        }
    }

    fn oidc(&self) -> (Arc<OidcProvider>, reqwest::Client) {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("OIDC HTTP client");
        let oidc = OidcProvider::new(&config(&self.issuer), http.clone());
        (Arc::new(oidc), http)
    }

    async fn rotate(&self, kid: &str) {
        *self.state.keys.write().await = jwks(KEY_B, kid);
        self.set_token(KEY_B, kid, json!({}), Algorithm::RS256)
            .await;
    }

    async fn set_token(&self, key: &str, kid: &str, overrides: Value, alg: Algorithm) {
        *self.state.id_token.write().await = sign_token(key, kid, &self.issuer, overrides, alg);
    }

    async fn wait_for_tokens(&self, count: u32) {
        tokio::time::timeout(
            Duration::from_secs(5),
            self.state.tokens_started.acquire_many(count),
        )
        .await
        .expect("callbacks reached token endpoint")
        .expect("token request semaphore")
        .forget();
    }

    fn release_tokens(&self, count: usize) {
        self.state.tokens_released.add_permits(count);
    }

    fn counts(&self) -> (usize, usize, usize, usize) {
        (
            self.state.discovery_requests.load(Ordering::SeqCst),
            self.state.jwks_requests.load(Ordering::SeqCst),
            self.state.token_requests.load(Ordering::SeqCst),
            self.state.userinfo_requests.load(Ordering::SeqCst),
        )
    }
}

impl Drop for MockProvider {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn discovery(State(state): State<Arc<MockState>>) -> Json<Value> {
    state.discovery_requests.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "issuer": state.issuer,
        "authorization_endpoint": format!("{}/authorize", state.issuer),
        "token_endpoint": format!("{}/token", state.issuer),
        "userinfo_endpoint": format!("{}/userinfo", state.issuer),
        "jwks_uri": format!("{}/keys", state.issuer),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "token_endpoint_auth_methods_supported": ["none"],
    }))
}

async fn keys(State(state): State<Arc<MockState>>) -> Response {
    state.jwks_requests.fetch_add(1, Ordering::SeqCst);
    if state.fail_jwks.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Json(state.keys.read().await.clone()).into_response()
}

async fn token(
    State(state): State<Arc<MockState>>,
    axum::Form(form): axum::Form<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    *state.last_token_form.write().await = form;
    state.token_requests.fetch_add(1, Ordering::SeqCst);
    if state.pause_tokens.load(Ordering::SeqCst) {
        state.tokens_started.add_permits(1);
        state
            .tokens_released
            .acquire()
            .await
            .expect("release mock token response")
            .forget();
    }
    let mut response = json!({
        "access_token": ACCESS_TOKEN,
        "token_type": "Bearer",
        "expires_in": 300,
        "id_token": state.id_token.read().await.clone(),
    });
    if !state.omit_refresh.load(Ordering::SeqCst) {
        response["refresh_token"] = json!("test-refresh-token");
    }
    Json(response)
}

async fn userinfo(State(state): State<Arc<MockState>>) -> Json<Value> {
    state.userinfo_requests.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "sub": state.userinfo_subject.read().await.clone(),
        "email": "test@example.test",
        "name": "Test User",
    }))
}

fn jwks(key: &str, kid: &str) -> Value {
    let signing_key =
        CoreRsaPrivateSigningKey::from_pem(key, Some(JsonWebKeyId::new(kid.to_owned())))
            .expect("test RSA key");
    json!({ "keys": [signing_key.as_verification_key()] })
}

fn sign_token(
    key: &str,
    kid: &str,
    issuer: &str,
    overrides: Value,
    algorithm: Algorithm,
) -> String {
    let now = chrono::Utc::now().timestamp();
    let digest = Sha256::digest(ACCESS_TOKEN.as_bytes());
    let mut claims = json!({
        "iss": issuer,
        "aud": CLIENT_ID,
        "sub": SUBJECT,
        "iat": now,
        "auth_time": now,
        "exp": now + 300,
        "nonce": NONCE,
        "at_hash": URL_SAFE_NO_PAD.encode(&digest[..16]),
    });
    claims.as_object_mut().expect("claims object").extend(
        overrides
            .as_object()
            .expect("claim overrides object")
            .clone(),
    );
    let mut header = Header::new(algorithm);
    header.kid = Some(kid.to_owned());
    encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(key.as_bytes()).expect("test signing key"),
    )
    .expect("sign test ID token")
}

async fn login(
    oidc: &OidcProvider,
    http: &reqwest::Client,
) -> notegate_core::Result<crate::auth::oauth_exchange::UserInfo> {
    exchange_code_for_userinfo(oidc, http, "single-use-code", "test-pkce-verifier", NONCE).await
}

#[tokio::test]
async fn valid_cached_callback_does_not_fetch_metadata_or_jwks() {
    let mock = MockProvider::start().await;
    let (oidc, http) = mock.oidc();
    oidc.client().await.expect("warm discovery cache");

    let result = login(&oidc, &http).await.expect("cached ID token verifies");

    assert_eq!(result.sub, SUBJECT);
    assert_eq!(mock.counts(), (1, 1, 1, 1));
}

#[tokio::test]
async fn callback_refreshes_new_kid_and_same_kid_replacement_without_redeeming_code_twice() {
    for kid in ["key-b", "key-a"] {
        let mock = MockProvider::start().await;
        let (oidc, http) = mock.oidc();
        oidc.client().await.expect("warm old signing key");
        let fetched_at = oidc
            .cache
            .read()
            .await
            .metadata
            .as_ref()
            .expect("cached metadata")
            .fetched_at;
        mock.rotate(kid).await;

        let result = login(&oidc, &http).await.expect("rotated key verifies");

        assert_eq!(result.sub, SUBJECT, "kid {kid}");
        assert_eq!(mock.counts(), (1, 2, 1, 1), "kid {kid}");
        assert_eq!(
            oidc.cache
                .read()
                .await
                .metadata
                .as_ref()
                .expect("cached metadata")
                .fetched_at,
            fetched_at
        );
    }
}

#[tokio::test]
async fn a_successful_jwks_fetch_that_cannot_verify_the_token_is_not_retried_again() {
    let mock = MockProvider::start().await;
    let (oidc, http) = mock.oidc();
    oidc.client().await.expect("warm old signing key");
    // The issuer still publishes A, so refreshing cannot validate B.
    mock.set_token(KEY_B, "key-b", json!({}), Algorithm::RS256)
        .await;

    let error = login(&oidc, &http)
        .await
        .expect_err("unpublished key must fail");

    assert!(
        error
            .to_string()
            .contains("id_token verification failed after JWKS refresh")
    );
    assert_eq!(mock.counts(), (1, 2, 1, 0));
}

#[tokio::test]
async fn failed_key_refresh_keeps_old_keys_and_allows_a_later_attempt_to_recover() {
    let mock = MockProvider::start().await;
    let (oidc, http) = mock.oidc();
    oidc.client().await.expect("warm old signing key");
    mock.rotate("key-b").await;
    mock.state.fail_jwks.store(true, Ordering::SeqCst);

    assert!(login(&oidc, &http).await.is_err());
    assert_eq!(mock.counts(), (1, 2, 1, 0));

    mock.set_token(KEY_A, "key-a", json!({}), Algorithm::RS256)
        .await;
    login(&oidc, &http)
        .await
        .expect("old cache survives failed JWKS refresh");
    assert_eq!(mock.counts(), (1, 2, 2, 1));

    mock.state.fail_jwks.store(false, Ordering::SeqCst);
    mock.rotate("key-b").await;
    login(&oidc, &http)
        .await
        .expect("later callback can refresh again");
    assert_eq!(mock.counts(), (1, 3, 3, 2));
}

#[tokio::test]
async fn concurrent_callbacks_share_one_successful_key_refresh() {
    concurrent_callbacks_share_refresh(false).await;
}

#[tokio::test]
async fn concurrent_callbacks_share_one_failed_key_refresh() {
    concurrent_callbacks_share_refresh(true).await;
}

async fn concurrent_callbacks_share_refresh(fail: bool) {
    const CALLBACKS: usize = 8;
    let mock = MockProvider::start().await;
    let (oidc, http) = mock.oidc();
    oidc.client().await.expect("warm old signing key");
    mock.rotate("key-b").await;
    mock.state.fail_jwks.store(fail, Ordering::SeqCst);
    mock.state.pause_tokens.store(true, Ordering::SeqCst);
    let mut callbacks = Vec::new();
    for _ in 0..CALLBACKS {
        let oidc = oidc.clone();
        let http = http.clone();
        callbacks.push(tokio::spawn(async move { login(&oidc, &http).await }));
    }
    // All snapshots predate the refresh, even when the mock replies very quickly.
    mock.wait_for_tokens(CALLBACKS as u32).await;
    mock.release_tokens(CALLBACKS);
    for callback in callbacks {
        let result = tokio::time::timeout(Duration::from_secs(5), callback)
            .await
            .expect("callback completed")
            .expect("callback task");
        assert_eq!(result.is_err(), fail);
    }
    assert_eq!(
        mock.counts(),
        (1, 2, CALLBACKS, if fail { 0 } else { CALLBACKS })
    );
}

#[tokio::test]
async fn claim_and_userinfo_failures_never_trigger_an_unrelated_key_refresh() {
    for failure in Failure::ALL {
        let mock = MockProvider::start().await;
        let (oidc, http) = mock.oidc();
        oidc.client().await.expect("warm matching signing key");
        failure.install(&mock, KEY_A, "key-a").await;

        failure.assert_error(login(&oidc, &http).await);
        assert_eq!(
            mock.counts(),
            (1, 1, 1, failure.userinfo_requests()),
            "{failure:?}"
        );
    }
}

#[tokio::test]
async fn rotated_key_retry_preserves_all_claim_hash_and_subject_checks() {
    for failure in Failure::ALL {
        let mock = MockProvider::start().await;
        let (oidc, http) = mock.oidc();
        oidc.client().await.expect("warm old signing key");
        mock.rotate("key-b").await;
        failure.install(&mock, KEY_B, "key-b").await;

        failure.assert_error(login(&oidc, &http).await);
        // Issuer, audience and allowed algorithms are checked before key lookup.
        let jwks_requests = if matches!(
            failure,
            Failure::Issuer | Failure::Audience | Failure::Algorithm
        ) {
            1
        } else {
            2
        };
        assert_eq!(
            mock.counts(),
            (1, jwks_requests, 1, failure.userinfo_requests()),
            "{failure:?}"
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum Failure {
    Nonce,
    Issuer,
    Audience,
    Expiry,
    Algorithm,
    AccessTokenHash,
    UserinfoSubject,
}

impl Failure {
    const ALL: [Self; 7] = [
        Self::Nonce,
        Self::Issuer,
        Self::Audience,
        Self::Expiry,
        Self::Algorithm,
        Self::AccessTokenHash,
        Self::UserinfoSubject,
    ];

    async fn install(self, mock: &MockProvider, key: &str, kid: &str) {
        let overrides = match self {
            Self::Nonce => json!({"nonce": "another-login-nonce"}),
            Self::Issuer => json!({"iss": "https://unexpected-issuer.example.test"}),
            Self::Audience => json!({"aud": "another-client"}),
            Self::Expiry => json!({"exp": chrono::Utc::now().timestamp() - 3600}),
            Self::AccessTokenHash => json!({"at_hash": "not-the-access-token-hash"}),
            Self::Algorithm | Self::UserinfoSubject => json!({}),
        };
        let algorithm = if matches!(self, Self::Algorithm) {
            Algorithm::RS384
        } else {
            Algorithm::RS256
        };
        mock.set_token(key, kid, overrides, algorithm).await;
        if matches!(self, Self::UserinfoSubject) {
            *mock.state.userinfo_subject.write().await = "another-user".to_owned();
        }
    }

    fn userinfo_requests(self) -> usize {
        usize::from(matches!(self, Self::UserinfoSubject))
    }

    fn assert_error(self, result: notegate_core::Result<crate::auth::oauth_exchange::UserInfo>) {
        let error = result.expect_err("invalid claims must fail").to_string();
        let expected = match self {
            Self::Nonce => "Invalid nonce",
            Self::Issuer => "Invalid issuer",
            Self::Audience => "Invalid audiences",
            Self::Expiry => "Expired",
            Self::Algorithm => "Signature verification failed",
            Self::AccessTokenHash => "access token hash mismatch",
            Self::UserinfoSubject => "userinfo request failed: Failed to verify claims",
        };
        assert!(error.contains(expected), "{self:?}: {error}");
    }
}

#[tokio::test]
async fn callback_uses_a_concurrent_ttl_refresh_without_refetching_or_redeeming_code_twice() {
    let mock = MockProvider::start().await;
    let (oidc, http) = mock.oidc();
    oidc.client().await.expect("warm old signing key");
    mock.rotate("key-b").await;
    mock.state.pause_tokens.store(true, Ordering::SeqCst);
    let callback_oidc = oidc.clone();
    let callback = tokio::spawn(async move { login(&callback_oidc, &http).await });
    mock.wait_for_tokens(1).await;
    {
        let mut cache = oidc.cache.write().await;
        cache.metadata.as_mut().expect("warm metadata").fetched_at =
            std::time::Instant::now() - METADATA_CACHE_TTL - Duration::from_secs(1);
    }
    oidc.client()
        .await
        .expect("concurrent TTL discovery refresh");
    assert_eq!(mock.counts(), (2, 2, 1, 0));
    mock.release_tokens(1);

    let result = tokio::time::timeout(Duration::from_secs(5), callback)
        .await
        .expect("callback completed")
        .expect("callback task")
        .expect("fresh concurrent keys verify");

    assert_eq!(result.sub, SUBJECT);
    assert_eq!(mock.counts(), (2, 2, 1, 1));
}

fn config(issuer: &str) -> notegate_core::Config {
    notegate_core::Config {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9191),
        search_bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9192),
        search_service_url: None,
        process_mode: notegate_core::ProcessMode::All,
        database_url: "postgres://unused".to_owned(),
        db_max_connections: 1,
        read_database_url: None,
        read_db_max_connections: 1,
        background_jobs: notegate_core::BackgroundJobsConfig::default(),
        oidc: notegate_core::config::OidcLoginConfig {
            issuer: issuer.to_owned(),
            client_id: CLIENT_ID.to_owned(),
            client_secret: None,
            allow_signup: false,
        },
        notegate_public_url: "http://localhost:9191".to_owned(),

        cli_oauth_client_id: "notegate-cli-local".to_owned(),
        oauth_redirect_url: "http://localhost:9191/auth/callback".to_owned(),
        resource_url: "http://localhost:9191/mcp".to_owned(),

        enc_root_key_id: "test-enc".to_owned(),
        enc_root_secret: SecretString::from("test-enc-root-secret-32-bytes-long".to_owned()),
        lookup_root_key_id: "test-lookup".to_owned(),
        lookup_root_secret: SecretString::from("test-lookup-root-secret-32-bytes-long".to_owned()),

        browser_session_max_ttl: Duration::from_secs(30 * 86_400),
        metrics_enabled: false,
        web_dist_dir: None,
        s3: crate::state::test_s3_config(),
        default_user_tier: notegate_core::tier::UserTier::DEFAULT,
        limits: notegate_core::limits::Limits::default(),
        http_rate_limits: notegate_core::HttpRateLimitsConfig::default(),
        search_body_cache: notegate_core::SearchBodyCacheConfig::default(),
        secure_cookies: false,
    }
}

// Public test fixture; never used outside the local mock issuer.
const KEY_A: &str = r#"-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEAn4EPtAOCc9AlkeQHPzHStgAbgs7bTZLwUBZdR8/KuKPEHLd4
rHVTeT+O+XV2jRojdNhxJWTDvNd7nqQ0VEiZQHz/AJmSCpMaJMRBSFKrKb2wqVwG
U/NsYOYL+QtiWN2lbzcEe6XC0dApr5ydQLrHqkHHig3RBordaZ6Aj+oBHqFEHYpP
e7Tpe+OfVfHd1E6cS6M1FZcD1NNLYD5lFHpPI9bTwJlsde3uhGqC0ZCuEHg8lhzw
OHrtIQbS0FVbb9k3+tVTU4fg/3L/vniUFAKwuCLqKnS2BYwdq/mzSnbLY7h/qixo
R7jig3//kRhuaxwUkRz5iaiQkqgc5gHdrNP5zwIDAQABAoIBAG1lAvQfhBUSKPJK
Rn4dGbshj7zDSr2FjbQf4pIh/ZNtHk/jtavyO/HomZKV8V0NFExLNi7DUUvvLiW7
0PgNYq5MDEjJCtSd10xoHa4QpLvYEZXWO7DQPwCmRofkOutf+NqyDS0QnvFvp2d+
Lov6jn5C5yvUFgw6qWiLAPmzMFlkgxbtjFAWMJB0zBMy2BqjntOJ6KnqtYRMQUxw
TgXZDF4rhYVKtQVOpfg6hIlsaoPNrF7dofizJ099OOgDmCaEYqM++bUlEHxgrIVk
wZz+bg43dfJCocr9O5YX0iXaz3TOT5cpdtYbBX+C/5hwrqBWru4HbD3xz8cY1TnD
qQa0M8ECgYEA3Slxg/DwTXJcb6095RoXygQCAZ5RnAvZlno1yhHtnUex/fp7AZ/9
nRaO7HX/+SFfGQeutao2TDjDAWU4Vupk8rw9JR0AzZ0N2fvuIAmr/WCsmGpeNqQn
ev1T7IyEsnh8UMt+n5CafhkikzhEsrmndH6LxOrvRJlsPp6Zv8bUq0kCgYEAuKE2
dh+cTf6ERF4k4e/jy78GfPYUIaUyoSSJuBzp3Cubk3OCqs6grT8bR/cu0Dm1MZwW
mtdqDyI95HrUeq3MP15vMMON8lHTeZu2lmKvwqW7anV5UzhM1iZ7z4yMkuUwFWoB
vyY898EXvRD+hdqRxHlSqAZ192zB3pVFJ0s7pFcCgYAHw9W9eS8muPYv4ZhDu/fL
2vorDmD1JqFcHCxZTOnX1NWWAj5hXzmrU0hvWvFC0P4ixddHf5Nqd6+5E9G3k4E5
2IwZCnylu3bqCWNh8pT8T3Gf5FQsfPT5530T2BcsoPhUaeCnP499D+rb2mTnFYeg
mnTT1B/Ue8KGLFFfn16GKQKBgAiw5gxnbocpXPaO6/OKxFFZ+6c0OjxfN2PogWce
TU/k6ZzmShdaRKwDFXisxRJeNQ5Rx6qgS0jNFtbDhW8E8WFmQ5urCOqIOYk28EBi
At4JySm4v+5P7yYBh8B8YD2l9j57z/s8hJAxEbn/q8uHP2ddQqvQKgtsni+pHSk9
XGBfAoGBANz4qr10DdM8DHhPrAb2YItvPVz/VwkBd1Vqj8zCpyIEKe/07oKOvjWQ
SgkLDH9x2hBgY01SbP43CvPk0V72invu2TGkI/FXwXWJLLG7tDSgw4YyfhrYrHmg
1Vre3XB9HH8MYBVB6UIexaAq4xSeoemRKTBesZro7OKjKT8/GmiO
-----END RSA PRIVATE KEY-----"#;

// Public test fixture; never used outside the local mock issuer.
const KEY_B: &str = r#"-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEAsRMj0YYjy7du6v1gWyKSTJx3YjBzZTG0XotRP0IaObw0k+68
30dXadjL5jVhSWNdcg9OyMyTGWfdNqfdrS6ppBqlQNgjZJdloIqL9zOLBZrDm7G4
+qN4KeZ4/5TyEilq2zOHHGFEzXpOq/UxqVnm3J4fhjqCNaS2nKd7HVVXGBQQ+4+F
dVT+MyJXemw5maz2F/h324TQi6XoUPEwUddxBwLQFSOlzWnHYMc4/lcyZJ8MpTXC
MPe/YJFNtb9CaikKUdf8x4mzwH7usSf8s2d6R4dQITzKrjrEJ0u3w3eGkBBapoMV
FBGPjP3Haz5FsVtHc5VEN3FZVIDF6HrbJH1C4QIDAQABAoIBAHSS3izM+3nc7Bel
8S5uRxRKmcm5je6b11u6qiVUFkHWJmMRc6QmqmSThkCq+b4/vUAe1cYZ7+l02Exo
HOcrZiEULaDP6hUKGqyjKVv3wdlRtt8kFFxlC/HBufzAiNDuFVvzw0oquwnvMCXC
yQvtlK+/JY/PqvM32cSt+b4o9apySsHqAtdsoHHohK82jsQqIfCi1v8XYV/xRBJB
cQMCaA0Ls3tFpmJv3JdikyyQxio4kZ5tswghC63znCp1iL+qDq1wjjKzjick9MDb
Qzb95X09QQP201l1FPWN7Kbhj4ybg6PJGz/VHQcvILcBCoYIc0UY/OMSBt9VN9yD
wr1WlbECgYEA37difsTMcLmUEN57sicFe1q4lxH6eqnUBjmoKBflx4oMIIyRnfjF
Jwsu9yIiBkJfBCP85nl2tZdcV0wfZLf6amxB/KMtdfW6r8eoTDzE472OYxSIg1F5
dI4qn2nBI0Dou0g58xj+Kv0iLaym0pxtyJkSg/rxZGwKb9a+x5WAs50CgYEAyqC0
NcZs2BRIiT5kEOF6+MeUvarbKh1mangKHKcTdXRrvoJ+Z5izm7FifBixo/79MYpt
0VofW0IzYKtAI9KZDq2JcozEbZ+lt/ZPH5QEXO4T39QbDoAG8BbOmEP7l+6m+7QO
PiQ0WSNjDnwk3W7Zihgg31DH7hyxsxQCapKLcxUCgYAwERXPiPcoDSd8DGFlYK7z
1wUsKEe6DT0p7T9tBd1v5wA+ChXLbETn46Y+oQ3QbHg/yn+vAU/5KkFD3G4uVL0w
Gnx/DIxa+OYYmHxXjQL8r6ClNycxl9LRsS4FPFKsAWk/u///dFI/6E1spNjfDY8k
94ab5tHwsqn3Z5tsBHo3nQKBgFUmxbSXh2Qi2fy6+GhTqU7k6G/wXhvLsR9rBKzX
1YiVfTXZNu+oL0ptd/q4keZeIN7x0oaY/fZm0pp8PP8Q4HtXmBxIZb+/yG+Pld6q
YE8BSd7VDu3ABapdm0JHx3Iou4mpOBcLNeiDw3vx1bgsfkTXMPFHzE0XR+H+tak9
nlalAoGBALAmAF7WBGdOt43Rj8hPaKOM/ahj+6z3CNwVreToNsVBHoyNmiO8q7MC
+tRo4jgdrzk1pzs66OIHfbx5P1mXKPtgPZhvI5omAY8WqXEgeNqSL1Ksp6LZ2ql/
ouZns5xwKc9+aRL+GWoAGNzwzcjE8cP52sBy/r0rYXTs/sZo5kgV
-----END RSA PRIVATE KEY-----"#;

#[tokio::test]
async fn standalone_oidc_callback_preserves_existing_owner_without_upstream_refresh()
-> Result<(), Box<dyn std::error::Error>> {
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, header};
    use tower::ServiceExt as _;
    let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
        return Ok(());
    };
    let mock = MockProvider::start().await;
    mock.state.omit_refresh.store(true, Ordering::SeqCst);
    let mut state = crate::auth::bearer_tests::standalone_state(&db)?;
    let mut config = (*state.config).clone();
    config.oidc.issuer = mock.issuer.clone();
    config.oidc.client_secret = None;
    state.config = Arc::new(config);
    state.oidc = Arc::new(OidcProvider::new(&state.config, state.http.clone()));
    // This simulates AuthGate remaining the same standards-based upstream provider.
    let owner = state
        .accounts
        .upsert_user_by_identity(&crate::identity::ResolveAttrs {
            issuer: mock.issuer.clone(),
            sub: SUBJECT.to_owned(),
            email: "test@example.test".to_owned(),
            name: "Existing owner".to_owned(),
        })
        .await?
        .0
        .id;
    sqlx::query("INSERT INTO audit_events (owner_user_id, actor_account_id, source, op_type, resource_type, resource_id) VALUES ($1, $1, 'rest', 'test.before_oidc', 'account', $1)")
        .bind(owner).execute(&db.pool).await?;
    let router = Router::new()
        .route("/auth/login", get(crate::auth::oauth::login))
        .route("/auth/callback", get(crate::auth::oauth::callback))
        .with_state(state.clone());
    let login = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/auth/login?next=%2F")
                .body(Body::empty())?,
        )
        .await?;
    let location = url::Url::parse(login.headers()[header::LOCATION].to_str()?)?;
    let query: std::collections::HashMap<_, _> = location.query_pairs().into_owned().collect();
    assert_eq!(query["max_age"], "0");
    assert!(!query["scope"].contains("offline_access"));
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()?
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let second = router
        .clone()
        .oneshot(Request::builder().uri("/auth/login").body(Body::empty())?)
        .await?;
    let second_cookie = second.headers()[header::SET_COOKIE]
        .to_str()?
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert_ne!(cookie.split('=').next(), second_cookie.split('=').next());
    let callback = format!("/auth/callback?code=code&state={}", query["state"]);
    let wrong = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&callback)
                .header(header::COOKIE, &second_cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    let payload: Value =
        sqlx::query_scalar("SELECT private_payload FROM oidc_login_flows WHERE id=$1")
            .bind(uuid::Uuid::parse_str(&query["state"])?)
            .fetch_one(&db.pool)
            .await?;
    assert!(!payload.to_string().contains(&query["nonce"]));
    mock.set_token(
        KEY_A,
        "key-a",
        json!({"nonce":query["nonce"],"auth_time":chrono::Utc::now().timestamp()}),
        Algorithm::RS256,
    )
    .await;
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&callback)
                .header(header::COOKIE, &cookie)
                .header(header::USER_AGENT, "Macintosh Chrome/130 Safari/537")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/");
    let session_cookie = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("notegate_browser_session="))
        .ok_or("no local session")?
        .split(';')
        .next()
        .unwrap();
    let session = session_cookie
        .strip_prefix("notegate_browser_session=")
        .unwrap();
    let caller = crate::auth::session::verify_browser_session(&state, session).await?;
    assert_eq!(caller.account_id(), owner);
    let preserved: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE actor_account_id=$1 AND op_type='test.before_oidc'",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(preserved, 1);
    assert_eq!(
        state
            .accounts
            .find_user_id_by_identity(&mock.issuer, SUBJECT)
            .await?,
        Some(owner)
    );

    let form = mock.state.last_token_form.read().await;
    assert!(!form.contains_key("client_secret"));
    assert_eq!(
        notegate_service::oauth::pkce_challenge(&form["code_verifier"]),
        query["code_challenge"]
    );
    drop(form);
    let (id, secret) = notegate_db::browser_session_repo::parse_token(session).unwrap();
    let stored = state
        .browser_sessions
        .find_by_token(
            id,
            &state
                .security
                .browser_session_hash(&id.to_string(), secret)?,
        )
        .await?
        .unwrap();
    assert_eq!(stored.oidc_issuer.as_deref(), Some(mock.issuer.as_str()));
    let environment: Option<String> =
        sqlx::query_scalar("SELECT client_environment FROM browser_sessions WHERE id=$1")
            .bind(id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(environment.as_deref(), Some("Chrome on macOS"));
    let audit_environment: Option<String> = sqlx::query_scalar("SELECT metadata->>'client_environment' FROM audit_events WHERE resource_id=$1 AND op_type='session.login'").bind(id).fetch_one(&db.pool).await?;
    assert_eq!(audit_environment, environment);
    assert!(stored.idle_expires_at > chrono::Utc::now());
    assert!(stored.idle_expires_at <= stored.expires_at);
    let before = mock.counts();
    let replay = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&callback)
                .header(header::COOKIE, &cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    assert_eq!(mock.counts(), before);
    sqlx::query(
        "UPDATE browser_sessions SET idle_expires_at=now()-interval '1 second' WHERE id=$1",
    )
    .bind(id)
    .execute(&db.pool)
    .await?;
    assert!(
        crate::auth::session::verify_browser_session(&state, session)
            .await
            .is_err()
    );
    assert_eq!(mock.counts(), before);
    // A valid signed identity from a different issuer cannot silently create a duplicate account.
    let mut denied_state = state.clone();
    let mut denied_config = (*state.config).clone();
    denied_config.oidc.allow_signup = false;
    denied_state.config = Arc::new(denied_config);
    let denied = crate::auth::oauth::complete_login(
        &denied_state,
        axum_extra::extract::CookieJar::new(),
        crate::auth::oauth_exchange::UserInfo {
            sub: "unlinked-subject".to_owned(),
            email: Some("test@example.test".to_owned()),
            name: Some("Duplicate".to_owned()),
        },
        None,
        None,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let _ = to_bytes(denied.into_body(), 10_000).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn standalone_confidential_oidc_requires_recent_auth_time_and_not_a_refresh_token() {
    let mock = MockProvider::start().await;
    mock.state.omit_refresh.store(true, Ordering::SeqCst);
    let mut config = config(&mock.issuer);
    config.oidc = notegate_core::config::OidcLoginConfig {
        issuer: mock.issuer.clone(),
        client_id: CLIENT_ID.to_owned(),
        client_secret: Some(SecretString::from("test-client-secret".to_owned())),
        allow_signup: false,
    };
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");
    let oidc = OidcProvider::new(&config, http.clone());
    mock.set_token(KEY_A, "key-a", json!({"auth_time": null}), Algorithm::RS256)
        .await;
    assert!(
        exchange_code_for_userinfo(&oidc, &http, "code", "verifier", NONCE)
            .await
            .is_err()
    );
    mock.set_token(
        KEY_A,
        "key-a",
        json!({"auth_time":chrono::Utc::now().timestamp()-120}),
        Algorithm::RS256,
    )
    .await;
    assert!(
        exchange_code_for_userinfo(&oidc, &http, "code", "verifier", NONCE)
            .await
            .is_err()
    );
    mock.set_token(
        KEY_A,
        "key-a",
        json!({"auth_time":chrono::Utc::now().timestamp()}),
        Algorithm::RS256,
    )
    .await;
    let result = exchange_code_for_userinfo(&oidc, &http, "code", "verifier", NONCE)
        .await
        .expect("fresh confidential OIDC");
    assert_eq!(result.sub, SUBJECT);
    assert_eq!(
        mock.state.last_token_form.read().await["client_secret"],
        "test-client-secret"
    );
}
