#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::future::join_all;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use openidconnect::core::CoreRsaPrivateSigningKey;
use openidconnect::{JsonWebKeyId, PrivateSigningKey};
use serde_json::{Value, json};
use tokio::sync::{RwLock, Semaphore};
use tokio::task::JoinHandle;

use super::{JwtAuthority, ON_DEMAND_REFRESH_COOLDOWN};
use crate::auth::bearer::AuthError;

const ISSUER: &str = "https://auth.example.test";
const AUDIENCE: &str = "https://notes.example.test/mcp";

struct TestClock(Mutex<Instant>);

impl TestClock {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(Instant::now())))
    }

    fn now(&self) -> Instant {
        *self.0.lock().expect("test clock")
    }

    fn advance(&self, duration: Duration) {
        let mut now = self.0.lock().expect("test clock");
        *now += duration;
    }
}

struct MockJwks {
    url: String,
    state: Arc<MockState>,
    server: JoinHandle<()>,
}

struct MockState {
    keys: RwLock<Value>,
    requests: AtomicUsize,
    fail: AtomicBool,
    pause: AtomicBool,
    started: Semaphore,
    released: Semaphore,
}

impl MockJwks {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind JWKS server");
        let url = format!("http://{}/keys", listener.local_addr().expect("mock address"));
        let state = Arc::new(MockState {
            keys: RwLock::new(jwks(KEY_A, "key-a")),
            requests: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
            pause: AtomicBool::new(false),
            started: Semaphore::new(0),
            released: Semaphore::new(0),
        });
        let router = Router::new()
            .route("/keys", get(keys))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.expect("JWKS server");
        });
        Self { url, state, server }
    }

    fn authority(&self) -> Arc<JwtAuthority> {
        Arc::new(JwtAuthority::from_url(&config(), self.url.clone()))
    }

    fn count(&self) -> usize {
        self.state.requests.load(Ordering::SeqCst)
    }

    async fn rotate(&self) {
        *self.state.keys.write().await = jwks(KEY_B, "key-b");
    }

    async fn wait_for_blocked_fetch(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.state.started.acquire())
            .await
            .expect("JWKS fetch started")
            .expect("JWKS started semaphore")
            .forget();
    }

    fn release_fetch(&self) {
        self.state.pause.store(false, Ordering::SeqCst);
        self.state.released.add_permits(1);
    }
}

impl Drop for MockJwks {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn keys(State(state): State<Arc<MockState>>) -> Response {
    state.requests.fetch_add(1, Ordering::SeqCst);
    if state.pause.load(Ordering::SeqCst) {
        state.started.add_permits(1);
        state
            .released
            .acquire()
            .await
            .expect("release JWKS fetch")
            .forget();
    }
    if state.fail.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Json(state.keys.read().await.clone()).into_response()
}

async fn verify(authority: &JwtAuthority, token: &str, clock: &TestClock) -> Result<(), AuthError> {
    authority
        .verify_claims_with_clock(token, || clock.now())
        .await
        .map(|_claims| ())
}

async fn warm(authority: &JwtAuthority, clock: &TestClock) {
    verify(authority, &token(KEY_A, "key-a"), clock)
        .await
        .expect("initial trusted key");
}

#[tokio::test]
async fn initial_fetch_and_all_unknown_kids_share_one_cooldown() {
    let mock = MockJwks::start().await;
    let authority = mock.authority();
    let clock = TestClock::new();
    for kid in ["unknown-a", "unknown-a", "unknown-b"] {
        assert!(matches!(
            verify(&authority, &token(KEY_A, kid), &clock).await,
            Err(AuthError::InvalidToken)
        ));
    }
    assert_eq!(mock.count(), 1, "initial fetch already consumed the budget");

    clock.advance(ON_DEMAND_REFRESH_COOLDOWN);
    for kid in ["unknown-b", "unknown-b", "unknown-c"] {
        assert!(matches!(
            verify(&authority, &token(KEY_A, kid), &clock).await,
            Err(AuthError::InvalidToken)
        ));
    }
    warm(&authority, &clock).await;
    assert_eq!(mock.count(), 2, "key IDs do not create separate budgets");
}

#[tokio::test]
async fn concurrent_unknown_kids_share_one_remote_refresh() {
    let mock = MockJwks::start().await;
    let authority = mock.authority();
    let clock = TestClock::new();
    warm(&authority, &clock).await;
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN);
    let tokens: Vec<_> = (0..20)
        .map(|index| token(KEY_A, &format!("unknown-{index}")))
        .collect();
    let results = join_all(tokens.iter().map(|token| verify(&authority, token, &clock))).await;
    assert!(
        results
            .into_iter()
            .all(|result| matches!(result, Err(AuthError::InvalidToken)))
    );
    assert_eq!(mock.count(), 2);
}

#[tokio::test]
async fn rotated_key_recovers_after_cooldown_and_concurrent_callers_reuse_it() {
    let mock = MockJwks::start().await;
    let authority = mock.authority();
    let clock = TestClock::new();
    warm(&authority, &clock).await;
    mock.rotate().await;
    let rotated = token(KEY_B, "key-b");
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN - Duration::from_nanos(1));
    assert!(matches!(
        verify(&authority, &rotated, &clock).await,
        Err(AuthError::InvalidToken)
    ));
    assert_eq!(mock.count(), 1);
    clock.advance(Duration::from_nanos(1));

    let results = join_all((0..20).map(|_index| verify(&authority, &rotated, &clock))).await;
    assert!(results.into_iter().all(|result| result.is_ok()));
    assert_eq!(mock.count(), 2);
}

#[tokio::test]
async fn failed_refresh_is_throttled_preserves_cached_keys_and_can_recover() {
    let mock = MockJwks::start().await;
    let authority = mock.authority();
    let clock = TestClock::new();
    warm(&authority, &clock).await;
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN);
    mock.state.fail.store(true, Ordering::SeqCst);
    let rotated = token(KEY_B, "key-b");
    assert!(matches!(
        verify(&authority, &rotated, &clock).await,
        Err(AuthError::InvalidToken)
    ));
    for kid in ["key-b", "unknown", "other-unknown"] {
        assert!(matches!(
            verify(&authority, &token(KEY_A, kid), &clock).await,
            Err(AuthError::InvalidToken)
        ));
    }
    warm(&authority, &clock).await;
    assert_eq!(mock.count(), 2);
    mock.state.fail.store(false, Ordering::SeqCst);
    mock.rotate().await;
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN);
    verify(&authority, &rotated, &clock)
        .await
        .expect("recover after failed refresh");
    assert_eq!(mock.count(), 3);
}

#[tokio::test]
async fn failed_initial_fetch_is_throttled_and_retries_after_cooldown() {
    let mock = MockJwks::start().await;
    let authority = mock.authority();
    let clock = TestClock::new();
    mock.state.fail.store(true, Ordering::SeqCst);
    let valid = token(KEY_A, "key-a");
    assert!(matches!(
        verify(&authority, &valid, &clock).await,
        Err(AuthError::Internal)
    ));
    let results = join_all((0..20).map(|_index| verify(&authority, &valid, &clock))).await;
    assert!(
        results
            .into_iter()
            .all(|result| matches!(result, Err(AuthError::Unavailable)))
    );
    assert_eq!(mock.count(), 1);
    mock.state.fail.store(false, Ordering::SeqCst);
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN);
    warm(&authority, &clock).await;
    assert_eq!(mock.count(), 2);
}

#[tokio::test]
async fn cached_valid_tokens_do_not_wait_for_a_blocked_unknown_key_refresh() {
    let mock = MockJwks::start().await;
    let authority = mock.authority();
    let clock = TestClock::new();
    warm(&authority, &clock).await;
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN);
    mock.state.pause.store(true, Ordering::SeqCst);
    let refresh = tokio::spawn({
        let authority = authority.clone();
        let clock = clock.clone();
        async move { verify(&authority, &token(KEY_A, "unknown"), &clock).await }
    });
    mock.wait_for_blocked_fetch().await;
    tokio::time::timeout(Duration::from_secs(5), warm(&authority, &clock))
        .await
        .expect("cached valid token must bypass refresh gate");
    mock.release_fetch();
    assert!(matches!(
        refresh.await.expect("refresh task"),
        Err(AuthError::InvalidToken)
    ));
    assert_eq!(mock.count(), 2);
}

#[tokio::test]
async fn cooldown_starts_again_when_a_slow_refresh_finishes() {
    let mock = MockJwks::start().await;
    let authority = mock.authority();
    let clock = TestClock::new();
    warm(&authority, &clock).await;
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN);
    mock.state.pause.store(true, Ordering::SeqCst);
    let refresh = tokio::spawn({
        let authority = authority.clone();
        let clock = clock.clone();
        async move { verify(&authority, &token(KEY_A, "unknown"), &clock).await }
    });
    mock.wait_for_blocked_fetch().await;
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN * 2);
    mock.release_fetch();
    assert!(matches!(
        refresh.await.expect("refresh task"),
        Err(AuthError::InvalidToken)
    ));
    assert!(matches!(
        verify(&authority, &token(KEY_A, "other"), &clock).await,
        Err(AuthError::InvalidToken)
    ));
    assert_eq!(mock.count(), 2);
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN);
    assert!(matches!(
        verify(&authority, &token(KEY_A, "other"), &clock).await,
        Err(AuthError::InvalidToken)
    ));
    assert_eq!(mock.count(), 3);
}

#[tokio::test]
async fn cancelled_initial_fetch_keeps_its_cooldown() {
    let mock = MockJwks::start().await;
    let authority = mock.authority();
    let clock = TestClock::new();
    mock.state.pause.store(true, Ordering::SeqCst);
    let initial = tokio::spawn({
        let authority = authority.clone();
        let clock = clock.clone();
        async move { verify(&authority, &token(KEY_A, "key-a"), &clock).await }
    });
    mock.wait_for_blocked_fetch().await;
    initial.abort();
    assert!(
        initial
            .await
            .expect_err("initial fetch cancelled")
            .is_cancelled()
    );
    mock.release_fetch();
    assert!(matches!(
        verify(&authority, &token(KEY_A, "key-a"), &clock).await,
        Err(AuthError::Unavailable)
    ));
    assert_eq!(mock.count(), 1);
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN);
    warm(&authority, &clock).await;
    assert_eq!(mock.count(), 2);
}

#[tokio::test]
async fn cancelled_unknown_key_refresh_keeps_its_cooldown() {
    let mock = MockJwks::start().await;
    let authority = mock.authority();
    let clock = TestClock::new();
    warm(&authority, &clock).await;
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN);
    mock.state.pause.store(true, Ordering::SeqCst);
    let refresh = tokio::spawn({
        let authority = authority.clone();
        let clock = clock.clone();
        async move { verify(&authority, &token(KEY_A, "unknown"), &clock).await }
    });
    mock.wait_for_blocked_fetch().await;
    refresh.abort();
    assert!(
        refresh
            .await
            .expect_err("refresh cancelled")
            .is_cancelled()
    );
    mock.release_fetch();
    warm(&authority, &clock).await;
    assert!(matches!(
        verify(&authority, &token(KEY_A, "other"), &clock).await,
        Err(AuthError::InvalidToken)
    ));
    assert_eq!(mock.count(), 2);
    clock.advance(ON_DEMAND_REFRESH_COOLDOWN);
    assert!(matches!(
        verify(&authority, &token(KEY_A, "other"), &clock).await,
        Err(AuthError::InvalidToken)
    ));
    assert_eq!(mock.count(), 3);
}

fn jwks(key: &str, kid: &str) -> Value {
    let signing_key =
        CoreRsaPrivateSigningKey::from_pem(key, Some(JsonWebKeyId::new(kid.to_owned())))
            .expect("test RSA key");
    json!({"keys": [signing_key.as_verification_key()]})
}

fn token(key: &str, kid: &str) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.to_owned());
    encode(
        &header,
        &json!({
            "iss": ISSUER,
            "aud": AUDIENCE,
            "sub": "test-user",
            "exp": chrono::Utc::now().timestamp() + 300,
        }),
        &EncodingKey::from_rsa_pem(key.as_bytes()).expect("test signing key"),
    )
    .expect("sign test token")
}

fn config() -> notegate_core::Config {
    notegate_core::Config {
        bind_addr: "127.0.0.1:9191".parse().expect("test address"),
        search_bind_addr: "127.0.0.1:9192".parse().expect("test search address"),
        search_service_url: None,
        process_mode: notegate_core::ProcessMode::All,
        database_url: "postgres://unused".to_owned(),
        db_max_connections: 1,
        read_database_url: None,
        read_db_max_connections: 1,
        background_jobs: notegate_core::BackgroundJobsConfig::default(),
        authgate_url: ISSUER.to_owned(),
        notegate_public_url: "http://localhost:9191".to_owned(),
        oauth_client_id: "notegate-web".to_owned(),
        mcp_oauth_client_id: "notegate-mcp".to_owned(),
        cli_oauth_client_id: "notegate-cli-local".to_owned(),
        oauth_redirect_url: "http://localhost:9191/auth/callback".to_owned(),
        resource_url: "http://localhost:9191/mcp".to_owned(),
        jwks_cache_ttl: Duration::from_secs(300),
        enc_root_key_id: "test-enc".to_owned(),
        enc_root_secret: secrecy::SecretString::from(
            "test-enc-root-secret-32-bytes-long".to_owned(),
        ),
        lookup_root_key_id: "test-lookup".to_owned(),
        lookup_root_secret: secrecy::SecretString::from(
            "test-lookup-root-secret-32-bytes-long".to_owned(),
        ),
        lookup_verify_0_key_id: None,
        lookup_verify_0_secret: None,
        browser_session_ttl: Duration::from_secs(3600),
        browser_session_max_ttl: Duration::from_secs(30 * 86_400),
        openapi_enabled: false,
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
