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
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::header::{CONTENT_TYPE, WWW_AUTHENTICATE};
use axum::http::{Request, StatusCode};
use chrono::Utc;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use notegate_db::{AccountRepo, test_support::TestDb};
use notegate_model::account::{Account, AccountKind};
use notegate_model::{Caller, CallerIdentity, Channel, User};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::auth::jwt::JwtAuthority;
use crate::identity::{CallerResolver, IdentityError, ResolveAttrs};
use crate::state::AppState;

use crate::auth::bearer::{AuthError, verify_bearer};
use crate::auth::session::{
    BROWSER_SESSION_COOKIE, create_browser_session, verify_browser_session,
};

const KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQCx8TUdJX0WeXTQ
I4bxI8S08Y4xr3k/hcyGmydJXiVt+hQXK4CM9Rz+4W2SYfazdWg0p1h0eyL883Jy
9LmxfQs44T1mifs7CZlm8ihxmZj3unsFQAA7hd9fGHwwNbVQrMGzAb7tNV6REoBs
800pSMl6Bta0VgStT/taUXwKsJfl5sN/KOS4ZQf5thFGdF3XSlf89MhDrXn0V0np
H3CA0iTSBE1EexYR2VX4DGE8CZhc9YU29ikO2I2UKLdqcKnyROVbMsop4l1YEfOv
fG4HJQZctx8IihWwF35Je2lLrYTamS3wPw6K1zFwT3/wNlxcHtn5MiPnji4Bjddo
9OZ4o9x3AgMBAAECggEAOsPhmiAU2PTAjrKE8KMy5dz2bFM6lC9wVa3swg6dBt51
fxdnS2Xxrv0szhCbRDYMdYMks8cszWPq0qsenk6hA6ZjPDdqaFtptXVYxPeIbJvB
4AB8cyvpkoLIFLXQDPYYvDDh6H3dHsUA87pAK9e1bh7PDlxwC/qjlHbfo7ohWBOZ
YzpsNeAhP3COpnhrkUTRoeBKV18T8p320VJ5fCVbK0w+vGEgw/8gWql3POjBUbb+
/N2dKXDLePXB94HjS6YLz0/Zvb9oMsDDiyOoC/1jXYXLHdKEbOPgW1KVjwmQp2ro
gA6mqK4fUSQ89pvqDzHpC3UGoSjSRvgwgoOJ/E18HQKBgQDvzovIWlpbIF9n4FGX
uq+mZa0fhcjyfe8p1YuDTAUJYuEx4CyoJDXuEil8yDvR1rYmPpqbGDArQlBtw67j
37m4+Cm0iRUHjlUUdwHHJggytRWeIq7AqAaPepjxdZAjV/6k1zIA2eGa8pK141rS
eBS22nreobqmhNWJ0hyicpO6mwKBgQC99Tr5b4aB3voVKG2cAG/ps2hrk70RKwcZ
yVd2xtN3iAGvvlG9UozpI7Unkm69jyHwwJTTVxYXD5Na1BbulUBNbJo7Ro1tzAtx
KvgZB6q2Li9HT84FzvZ29tQfQr9zxdnnunpptBip9oBCEK3yDBDmZXzzkwjKp7cY
zF85O4OlVQKBgDHPuG9UfUJCdi7QhII8z/GDWzOaCYR9LimFZuZN6xnpBRfkFcKT
SvR5p055FRvgOpO1G04t9wt1SdmS9Qf2V9CZE6ihdNHN+dQ3aBIizz8hKC1hzOTN
whcZgx1cqyT8STOaU5Ojrl4OFvVbFWl0cfENbspB09B09Rocn8AKhq8TAoGAJdwo
ouptfpj4cxsZrYwQwh115GsPtcpDogoVGqFKKHq9C0/9bqRzXUw2oOp4k+NhOmDH
yM+EoZgDIIlBANBSfpv0qXfIXGfcp/OOez6h8amG1sm7IEE9sjxDzu84xVRbt+nc
2BCDEe0FZyV35dQt0h3MJ6fYiruerJyfJgMMm/kCgYBnqQ5mEiA76yh/208g1nfM
WNYy7n/b2QYI1CcDUtrxjmDVGSbdQ1MG04Az3PhLBDh4UE/yOXb3slpLECmfjcK/
lq0mdqBAHuT8W8E2jRw9CejdITWxllSS0L8xhhSv5JMJ+3CUmpbsWP1X6ByQmF/E
EmW0T9kajxWyy7ochOgNdA==
-----END PRIVATE KEY-----"#;

#[derive(Debug, Serialize)]
struct TestClaims {
    sub: String,
    email: String,
    name: String,
    iss: String,
    aud: Value,
    exp: usize,
}

#[derive(Clone)]
struct TestResolver {
    mode: ResolverMode,
}

#[derive(Clone)]
enum ResolverMode {
    Registered(bool),
    Missing,
}

impl CallerResolver for TestResolver {
    fn resolve_browser(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve(attrs, Channel::Browser) })
    }

    fn resolve_browser_session_user(
        &self,
        _user_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move {
            self.resolve(
                ResolveAttrs {
                    sub: "session-user".to_owned(),
                    email: String::new(),
                    name: String::new(),
                },
                Channel::Browser,
            )
        })
    }

    fn resolve_api(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve(attrs, Channel::Api) })
    }

    fn resolve_mcp(
        &self,
        attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move { self.resolve(attrs, Channel::Mcp) })
    }

    fn resolve_api_key(
        &self,
        _token: String,
        _channel: Channel,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        // The bearer test harness only exercises the JWT/cookie paths; an
        // An unrecognized notegate API key resolves to nothing.
        Box::pin(async { Err(IdentityError::NotRegistered) })
    }
}

impl TestResolver {
    fn resolve(&self, attrs: ResolveAttrs, channel: Channel) -> Result<Caller, IdentityError> {
        match self.mode {
            ResolverMode::Missing => Err(IdentityError::NotRegistered),
            ResolverMode::Registered(active) if !active => Err(IdentityError::Inactive),
            ResolverMode::Registered(_active) => Ok(test_caller(attrs, channel)),
        }
    }
}

fn test_caller(attrs: ResolveAttrs, channel: Channel) -> Caller {
    let now = Utc::now();
    let account = Account {
        id: Uuid::nil(),
        kind: AccountKind::User,
        display_name: attrs.name,
        is_active: true,
        deleted_at: None,
        deleted_by: None,
        created_at: now,
        updated_at: now,
    };
    let user = User {
        id: Uuid::nil(),
        email: Some(attrs.email),
        tier: "tier0".to_owned(),
        anonymized_at: None,
    };
    Caller {
        account,
        identity: CallerIdentity::User(user),
        channel,
    }
}

fn state(mode: ResolverMode) -> Result<AppState, Box<dyn std::error::Error>> {
    state_with_resource(mode, "https://api.example.test")
}

fn state_with_resource(
    mode: ResolverMode,
    resource_url: &str,
) -> Result<AppState, Box<dyn std::error::Error>> {
    let pool =
        PgPoolOptions::new().connect_lazy("postgres://notegate:notegate@localhost/notegate")?;
    state_with_pool(mode, resource_url, pool)
}

fn state_with_pool(
    mode: ResolverMode,
    resource_url: &str,
    pool: PgPool,
) -> Result<AppState, Box<dyn std::error::Error>> {
    let config = Arc::new(notegate_core::Config {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9191),
        database_url: "postgres://notegate:notegate@localhost/notegate".to_owned(),
        db_max_connections: 1,
        authgate_url: "https://auth.example.test".to_owned(),
        notegate_public_url: "http://localhost:9191".to_owned(),
        oauth_client_id: "notegate-web".to_owned(),
        mcp_oauth_client_id: "notegate-mcp".to_owned(),
        oauth_redirect_url: "http://localhost:9191/auth/callback".to_owned(),
        resource_url: resource_url.to_owned(),
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
        browser_session_max_ttl: Duration::from_secs(15 * 86_400),
        openapi_enabled: false,
        web_dist_dir: None,
        default_user_tier: notegate_core::tier::UserTier::DEFAULT,
        limits: notegate_core::limits::Limits::default(),
        secure_cookies: false,
    });
    let jwt = Arc::new(JwtAuthority::from_jwks(&config, aliri_jwks()?));
    let oidc = Arc::new(crate::auth::oidc::OidcProvider::new(
        &config,
        reqwest::Client::new(),
    ));
    Ok(AppState::new(
        pool,
        config.clone(),
        jwt,
        oidc,
        Arc::new(TestResolver { mode }),
        reqwest::Client::new(),
        notegate_core::security::PiiCrypto::from_root_secrets(
            config.enc_root_key_id.clone(),
            &config.enc_root_secret,
            config.lookup_root_key_id.clone(),
            &config.lookup_root_secret,
        )
        .expect("derive test crypto"),
    ))
}

async fn valid_browser_session(
    db: &TestDb,
    state: &AppState,
) -> Result<String, Box<dyn std::error::Error>> {
    let (account, _user) = AccountRepo::with_crypto_and_default_user_tier(
        db.pool.clone(),
        state.security.clone(),
        state.config.default_user_tier,
    )
    .upsert_user_by_sub(&ResolveAttrs {
        sub: "session-user".to_owned(),
        email: "session@example.test".to_owned(),
        name: "Session User".to_owned(),
    })
    .await?;
    Ok(create_browser_session(state, account.id, "refresh-token").await?)
}

fn token(
    sub: &str,
    iss: &str,
    aud: Value,
    exp: usize,
    kid: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.to_owned());
    let claims = TestClaims {
        sub: sub.to_owned(),
        email: "user@example.test".to_owned(),
        name: "User".to_owned(),
        iss: iss.to_owned(),
        aud,
        exp,
    };
    Ok(encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(KEY.as_bytes())?,
    )?)
}

fn future_exp() -> usize {
    epoch_secs() + 3600
}

fn epoch_secs() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as usize)
        .unwrap_or(0)
}

#[tokio::test]
async fn verify_accepts_valid_token() -> Result<(), Box<dyn std::error::Error>> {
    let state = state(ResolverMode::Registered(true))?;
    let token = token(
        "sub-1",
        "https://auth.example.test",
        json!("https://api.example.test"),
        future_exp(),
        "kid-1",
    )?;
    let caller = verify_bearer(&state, &token).await?;
    assert_eq!(
        caller.user().and_then(|user| user.email.as_deref()),
        Some("user@example.test")
    );
    Ok(())
}

#[tokio::test]
async fn verify_rejects_invalid_claims_without_panic() -> Result<(), Box<dyn std::error::Error>> {
    let state = state(ResolverMode::Registered(true))?;
    let cases = [
        token(
            "sub-1",
            "https://auth.example.test",
            json!("https://api.example.test"),
            epoch_secs().saturating_sub(3600),
            "kid-1",
        )?,
        token(
            "sub-1",
            "https://other.example.test",
            json!("https://api.example.test"),
            future_exp(),
            "kid-1",
        )?,
        token(
            "sub-1",
            "https://auth.example.test",
            json!("https://other.example.test"),
            future_exp(),
            "kid-1",
        )?,
        token(
            "sub-1",
            "https://auth.example.test",
            json!("https://api.example.test"),
            future_exp(),
            "unknown",
        )?,
        "not-a-jwt".to_owned(),
        alg_none_token(),
    ];
    for (idx, candidate) in cases.into_iter().enumerate() {
        let err = verify_bearer(&state, &candidate).await.err();
        assert!(
            matches!(err, Some(AuthError::InvalidToken)),
            "case {idx}: {err:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn verify_accepts_aud_array_and_trailing_slash() -> Result<(), Box<dyn std::error::Error>> {
    let state = state(ResolverMode::Registered(true))?;
    let token = token(
        "sub-1",
        "https://auth.example.test",
        json!(["other", "https://api.example.test/"]),
        future_exp(),
        "kid-1",
    )?;
    let caller = verify_bearer(&state, &token).await?;
    assert_eq!(
        caller.user().and_then(|user| user.email.as_deref()),
        Some("user@example.test")
    );
    Ok(())
}

#[tokio::test]
async fn verify_maps_registered_state_errors() -> Result<(), Box<dyn std::error::Error>> {
    let valid = token(
        "sub-1",
        "https://auth.example.test",
        json!("https://api.example.test"),
        future_exp(),
        "kid-1",
    )?;
    let missing = state(ResolverMode::Missing)?;
    let missing_err = verify_bearer(&missing, &valid).await.err();
    assert!(matches!(missing_err, Some(AuthError::NotRegistered)));

    let inactive = state(ResolverMode::Registered(false))?;
    let inactive_err = verify_bearer(&inactive, &valid).await.err();
    assert!(matches!(inactive_err, Some(AuthError::Inactive)));
    Ok(())
}

#[tokio::test]
async fn api_routes_require_bearer_before_handler() -> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    let response = app
        .oneshot(Request::builder().uri("/api/v1/me").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn api_routes_accept_valid_bearer() -> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    let valid = token(
        "sub-1",
        "https://auth.example.test",
        json!("https://api.example.test"),
        future_exp(),
        "kid-1",
    )?;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/me")
                .header("authorization", format!("Bearer {valid}"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn api_routes_reject_unknown_browser_session() -> Result<(), Box<dyn std::error::Error>> {
    let state = state(ResolverMode::Registered(true))?;
    let app = crate::routes::app(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/me")
                .header("cookie", format!("{BROWSER_SESSION_COOKIE}=unknown"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn api_routes_accept_valid_browser_session() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test",
        db.pool.clone(),
    )?;
    let session = valid_browser_session(&db, &state).await?;
    let app = crate::routes::app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/me")
                .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn api_cookie_mutation_requires_same_origin_header() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test",
        db.pool.clone(),
    )?;
    let session = valid_browser_session(&db, &state).await?;
    let app = crate::routes::app(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/missing")
                .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/missing")
                .header("origin", "http://localhost:9191")
                .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_json_error(response, "not_found", "api route not found").await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn bearer_mutation_does_not_require_browser_origin() -> Result<(), Box<dyn std::error::Error>>
{
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    let valid = token(
        "sub-1",
        "https://auth.example.test",
        json!("https://api.example.test"),
        future_exp(),
        "kid-1",
    )?;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/missing")
                .header("authorization", format!("Bearer {valid}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_json_error(response, "not_found", "api route not found").await?;
    Ok(())
}

#[tokio::test]
async fn browser_session_resolves_browser_channel() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test",
        db.pool.clone(),
    )?;
    let session = valid_browser_session(&db, &state).await?;

    let caller = verify_browser_session(&state, &session).await?;

    assert_eq!(caller.channel, Channel::Browser);
    assert!(caller.user().is_some());
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn browser_session_rejects_malformed_token() -> Result<(), Box<dyn std::error::Error>> {
    let state = state(ResolverMode::Registered(true))?;
    let err = verify_browser_session(&state, "unknown").await.err();

    assert!(matches!(err, Some(AuthError::InvalidToken)));
    Ok(())
}

#[tokio::test]
async fn mcp_routes_reject_cookie_without_bearer() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test/mcp",
        db.pool.clone(),
    )?;
    let session = valid_browser_session(&db, &state).await?;
    let app = crate::routes::app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let challenge = response
        .headers()
        .get(WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(challenge.contains("resource_metadata="));
    assert!(challenge.contains("/.well-known/oauth-protected-resource/mcp"));
    assert!(challenge.contains("scope=\"openid offline_access\""));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn unknown_api_routes_still_require_bearer() -> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/missing")
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

async fn assert_json_error(
    response: axum::response::Response,
    kind: &str,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json; charset=utf-8")
    );
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let value: Value = serde_json::from_slice(&body)?;
    assert_eq!(value["error"], kind);
    assert_eq!(value["kind"], kind);
    assert_eq!(value["message"], message);
    Ok(())
}

#[tokio::test]
async fn public_routes_do_not_require_bearer() -> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state_with_resource(
        ResolverMode::Registered(true),
        "https://api.example.test/mcp",
    )?);
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/health").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    for path in ["/auth/success"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
    }

    let logout_get = app
        .clone()
        .oneshot(Request::builder().uri("/auth/logout").body(Body::empty())?)
        .await?;
    assert_eq!(logout_get.status(), StatusCode::METHOD_NOT_ALLOWED);

    let logout_post_without_origin = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/logout")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(logout_post_without_origin.status(), StatusCode::FORBIDDEN);

    let logout_post_cross_origin = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/logout")
                .header("origin", "https://evil.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(logout_post_cross_origin.status(), StatusCode::FORBIDDEN);

    let logout_post_same_origin = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/logout")
                .header("origin", "http://localhost:9191")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(logout_post_same_origin.status(), StatusCode::SEE_OTHER);

    for path in [
        "/.well-known/oauth-authorization-server",
        "/.well-known/oauth-protected-resource",
        "/.well-known/oauth-protected-resource/mcp",
        "/.well-known/oauth-protected-resource/mcp/tools",
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json; charset=utf-8")
        );
    }
    Ok(())
}

fn aliri_jwks() -> Result<aliri::Jwks, Box<dyn std::error::Error>> {
    use base64::Engine as _;

    let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(
        "sfE1HSV9Fnl00COG8SPEtPGOMa95P4XMhpsnSV4lbfoUFyuAjPUc_uFtkmH2s3VoNKdYdHsi_PNycvS5sX0LOOE9Zon7OwmZZvIocZmY97p7BUAAO4XfXxh8MDW1UKzBswG-7TVekRKAbPNNKUjJegbWtFYErU_7WlF8CrCX5ebDfyjkuGUH-bYRRnRd10pX_PTIQ6159FdJ6R9wgNIk0gRNRHsWEdlV-AxhPAmYXPWFNvYpDtiNlCi3anCp8kTlWzLKKeJdWBHzr3xuByUGXLcfCIoVsBd-SXtpS62E2pkt8D8OitcxcE9_8DZcXB7Z-TIj544uAY3XaPTmeKPcdw",
    )?;
    let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode("AQAB")?;
    let rsa = aliri::jwa::Rsa::from_public_components(n, e)?;
    let key = aliri::Jwk::from(rsa)
        .with_algorithm(aliri::jwa::Algorithm::RS256)
        .with_key_id(aliri::jwk::KeyId::from_static("kid-1"));
    let mut jwks = aliri::Jwks::default();
    jwks.add_key(key);
    Ok(jwks)
}

fn alg_none_token() -> String {
    let header = base64_url_json(&json!({"alg":"none","kid":"kid-1"}));
    let claims = base64_url_json(&json!({
        "sub":"sub-1",
        "email":"user@example.test",
        "name":"User",
        "iss":"https://auth.example.test",
        "aud":"https://api.example.test",
        "exp": future_exp()
    }));
    format!("{header}.{claims}.")
}

fn base64_url_json(value: &Value) -> String {
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        value.to_string().as_bytes(),
    )
}
