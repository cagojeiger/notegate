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
use axum::http::header::{
    ACCEPT, CACHE_CONTROL, CONTENT_TYPE, LOCATION, SET_COOKIE, WWW_AUTHENTICATE,
};
use axum::http::{Request, StatusCode, header};
use chrono::Utc;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use notegate_db::{AccountRepo, browser_session_repo::format_token, test_support::TestDb};
use notegate_model::account::{Account, AccountKind};
use notegate_model::{Agent, Caller, CallerIdentity, Channel, User};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::identity::{CallerResolver, IdentityError, ResolveAttrs};
use crate::state::AppState;

use crate::auth::bearer::{AuthError, verify_bearer_command, verify_bearer_mcp};
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
const TEST_API_KEY: &str = "ngk_v2_00000000-0000-0000-0000-000000000000_test-secret";
const TEST_LEGACY_API_KEY: &str = "ngk_v1_00000000-0000-0000-0000-000000000000_test-secret";
const MCP_INITIALIZE_REQUEST: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;

fn cli_request() -> axum::http::request::Builder {
    Request::builder()
        .method("POST")
        .uri("/cli")
        .header(CONTENT_TYPE, "application/json")
        .header("x-notegate-cli-version", env!("CARGO_PKG_VERSION"))
        .header(
            "x-notegate-command-protocol",
            notegate_command::COMMAND_PROTOCOL_VERSION,
        )
}

fn cli_body(tool: &str, input: Value) -> Body {
    Body::from(json!({"tool": tool, "input": input}).to_string())
}

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
                    issuer: "https://auth.example.test".to_owned(),
                    sub: "session-user".to_owned(),
                    email: String::new(),
                    name: String::new(),
                },
                Channel::Browser,
            )
        })
    }

    fn resolve_agent_api_key(
        &self,
        token: String,
        channel: Channel,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async move {
            if token != TEST_API_KEY {
                return Err(IdentityError::NotRegistered);
            }
            match self.mode {
                ResolverMode::Missing => Err(IdentityError::NotRegistered),
                ResolverMode::Registered(false) => Err(IdentityError::Inactive),
                ResolverMode::Registered(true) => Ok(test_agent_caller(channel)),
            }
        })
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

fn test_agent_caller(channel: Channel) -> Caller {
    let now = Utc::now();
    let account_id = Uuid::nil();
    Caller {
        account: Account {
            id: account_id,
            kind: AccountKind::Agent,
            display_name: "API Agent".to_owned(),
            is_active: true,
            deleted_at: None,
            deleted_by: None,
            created_at: now,
            updated_at: now,
        },
        identity: CallerIdentity::Agent(Agent {
            id: account_id,
            name: "api-agent".to_owned(),
            owner_user_id: Uuid::new_v4(),
        }),
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
        search_bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9192),
        search_service_url: None,
        process_mode: notegate_core::ProcessMode::All,
        database_url: "postgres://notegate:notegate@localhost/notegate".to_owned(),
        db_max_connections: 1,
        read_database_url: None,
        read_db_max_connections: 1,
        background_jobs: notegate_core::BackgroundJobsConfig::default(),
        oidc: notegate_core::config::OidcLoginConfig {
            issuer: "https://auth.example.test".to_owned(),
            client_id: "notegate-web".to_owned(),
            client_secret: None,
            allow_signup: false,
        },
        notegate_public_url: "http://localhost:9191".to_owned(),

        cli_oauth_client_id: "notegate-cli-local".to_owned(),
        oauth_redirect_url: "http://localhost:9191/auth/callback".to_owned(),
        resource_url: resource_url.to_owned(),

        enc_root_key_id: "test-enc".to_owned(),
        enc_root_secret: secrecy::SecretString::from(
            "test-enc-root-secret-32-bytes-long".to_owned(),
        ),
        lookup_root_key_id: "test-lookup".to_owned(),
        lookup_root_secret: secrecy::SecretString::from(
            "test-lookup-root-secret-32-bytes-long".to_owned(),
        ),

        browser_session_max_ttl: Duration::from_secs(30 * 86_400),
        metrics_enabled: false,
        web_dist_dir: None,
        s3: crate::state::test_s3_config(),
        default_user_tier: notegate_core::tier::UserTier::DEFAULT,
        limits: notegate_core::limits::Limits::default(),
        http_rate_limits: notegate_core::HttpRateLimitsConfig::default(),
        search_body_cache: notegate_core::SearchBodyCacheConfig::default(),
        secure_cookies: false,
    });
    let oidc = Arc::new(crate::auth::oidc::OidcProvider::new(
        &config,
        reqwest::Client::new(),
    ));
    Ok(AppState::new(
        pool,
        config.clone(),
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
    .upsert_user_by_identity(&ResolveAttrs {
        issuer: "https://auth.example.test".to_owned(),
        sub: "session-user".to_owned(),
        email: "session@example.test".to_owned(),
        name: "Session User".to_owned(),
    })
    .await?;
    Ok(create_browser_session(state, account.id, None).await?)
}

async fn local_access(
    state: &AppState,
    user_id: Uuid,
    resource: &str,
    scope: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(local_tokens(state, user_id, resource, scope)
        .await?
        .access_token)
}

async fn local_tokens(
    state: &AppState,
    user_id: Uuid,
    resource: &str,
    scope: &str,
) -> Result<notegate_service::oauth::TokenResponse, Box<dyn std::error::Error>> {
    let now = Utc::now();
    let code = Uuid::new_v4().to_string();
    let id = Uuid::new_v4();
    let cookie_hash = state
        .security
        .oauth_hash("authorization-cookie", "test-cookie")?;
    let expires_at = now + chrono::Duration::minutes(5);
    state
        .oauth
        .store
        .insert_authorization(&notegate_db::oauth_repo::Authorization {
            id,
            client_id: "test-client".to_owned(),
            client_name: "Test client".to_owned(),
            resource: resource.to_owned(),
            redirect_uri: "http://127.0.0.1/callback".to_owned(),
            scope: scope.to_owned(),
            code_challenge: notegate_service::oauth::pkce_challenge(
                "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGH",
            ),
            cookie_hash: cookie_hash.clone(),
            private_payload: json!({}),
            user_id: None,
            code_hash: None,
            expires_at,
            consumed_at: None,
        })
        .await?;
    assert!(
        state
            .oauth
            .store
            .approve_authorization(
                id,
                &cookie_hash,
                user_id,
                &state.security.oauth_hash("code", &code)?,
                now,
                expires_at
            )
            .await?
    );
    let tokens = state
        .oauth
        .exchange_code(
            &code,
            "test-client",
            resource,
            "http://127.0.0.1/callback",
            "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGH",
            now,
            None,
        )
        .await?;
    match tokens {
        notegate_service::oauth::TokenOutcome::Issued(tokens) => Ok(tokens),
        _ => Err("local grant was not issued".into()),
    }
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

#[tokio::test]
async fn session_management_is_owner_scoped_and_revokes_browser_cli_and_mcp_access()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = standalone_state(&db)?;
    let current = valid_browser_session(&db, &state).await?;
    let owner = verify_browser_session(&state, &current).await?.account_id();
    let remote = create_browser_session(&state, owner, Some("Android Chrome/130")).await?;
    let (remote_id, remote_secret) =
        notegate_db::browser_session_repo::parse_token(&remote).unwrap();
    let remote_hash = state
        .security
        .browser_session_hash(&remote_id.to_string(), remote_secret)?;
    let stranger = state
        .accounts
        .upsert_user_by_identity(&ResolveAttrs {
            issuer: state.oidc.issuer().into(),
            sub: "management-stranger".into(),
            email: "stranger@example.test".into(),
            name: "Other user".into(),
        })
        .await?
        .0
        .id;
    let stranger_session = create_browser_session(&state, stranger, None).await?;
    let stranger_id = notegate_db::browser_session_repo::parse_token(&stranger_session)
        .unwrap()
        .0;
    let cli_resource = format!("{}/cli", state.config.notegate_public_url);
    let cli = local_tokens(&state, owner, &cli_resource, "cli offline_access").await?;
    let mcp = local_tokens(
        &state,
        owner,
        &state.config.resource_url,
        "mcp offline_access",
    )
    .await?;
    let stranger_mcp = local_tokens(
        &state,
        stranger,
        &state.config.resource_url,
        "mcp offline_access",
    )
    .await?;
    let router = crate::routes::app(state.clone());
    let request = |method: &str, path: &str| {
        Request::builder()
            .method(method)
            .uri(path)
            .header(
                header::COOKIE,
                format!("{BROWSER_SESSION_COOKIE}={current}"),
            )
            .header(header::ORIGIN, "https://api.example.test")
            .body(Body::empty())
    };

    let response = router
        .clone()
        .oneshot(request("GET", "/api/v1/me/sessions")?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), 32_000).await?)?;
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions.iter().filter(|s| s["current"] == true).count(), 1);
    assert!(
        sessions
            .iter()
            .any(|s| s["client_environment"] == "Chrome on Android")
    );
    for session in sessions {
        assert!(session.get("token_hash").is_none());
        assert!(session.get("token_prefix").is_none());
    }
    for (path, method) in [
        ("/api/v1/me/sessions", "GET"),
        ("/api/v1/me/oauth-grants", "GET"),
        ("/api/v1/me/sessions/others", "DELETE"),
    ] {
        let denied = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", mcp.access_token),
                    )
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    }
    let rejected = router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/v1/me/sessions")
                .header(
                    header::COOKIE,
                    format!("{BROWSER_SESSION_COOKIE}={current}"),
                )
                .header(header::ORIGIN, "https://untrusted.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    let rejected = router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/v1/me/oauth-grants")
                .header(
                    header::COOKIE,
                    format!("{BROWSER_SESSION_COOKIE}={current}"),
                )
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

    let foreign = format!("/api/v1/me/sessions/{stranger_id}");
    assert_eq!(
        router
            .clone()
            .oneshot(request("DELETE", &foreign)?)
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    verify_browser_session(&state, &stranger_session).await?;
    let path = format!("/api/v1/me/sessions/{remote_id}");
    for _ in 0..2 {
        assert_eq!(
            router
                .clone()
                .oneshot(request("DELETE", &path)?)
                .await?
                .status(),
            StatusCode::NO_CONTENT
        );
    }
    assert!(verify_browser_session(&state, &remote).await.is_err());
    assert!(
        !state
            .browser_sessions
            .renew_idle_expiry(remote_id, &remote_hash, state.oidc.issuer())
            .await?
    );
    let revokes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE resource_id=$1 AND op_type='session.revoke'",
    )
    .bind(remote_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(revokes, 1);
    let another = create_browser_session(&state, owner, None).await?;
    assert_eq!(
        router
            .clone()
            .oneshot(request("DELETE", "/api/v1/me/sessions/others")?)
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(verify_browser_session(&state, &another).await.is_err());
    verify_browser_session(&state, &current).await?;

    let response = router
        .clone()
        .oneshot(request("GET", "/api/v1/me/oauth-grants")?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), 32_000).await?)?;
    let grants = body["grants"].as_array().unwrap();
    assert_eq!(grants.len(), 2);
    let cli_id = grants.iter().find(|g| g["kind"] == "cli").unwrap()["id"]
        .as_str()
        .unwrap();
    assert!(
        grants
            .iter()
            .all(|g| g["offline_access"] == true && g.get("access_token").is_none())
    );
    let stranger_grant = state.oauth.store.list_grants(stranger, Utc::now()).await?[0].id;
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "DELETE",
                &format!("/api/v1/me/oauth-grants/{stranger_grant}")
            )?)
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "DELETE",
                &format!("/api/v1/me/oauth-grants/{cli_id}")
            )?)
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        state
            .oauth
            .access_user(&cli.access_token, &cli_resource, "cli", Utc::now())
            .await?,
        None
    );
    assert!(matches!(
        state
            .oauth
            .refresh(
                cli.refresh_token.as_deref().unwrap(),
                "test-client",
                &cli_resource,
                Utc::now()
            )
            .await?,
        notegate_service::oauth::TokenOutcome::Rejected("invalid_grant")
    ));
    assert_eq!(
        state
            .oauth
            .access_user(
                &mcp.access_token,
                &state.config.resource_url,
                "mcp",
                Utc::now()
            )
            .await?,
        Some(owner)
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request("DELETE", "/api/v1/me/oauth-grants")?)
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        state
            .oauth
            .access_user(
                &mcp.access_token,
                &state.config.resource_url,
                "mcp",
                Utc::now()
            )
            .await?,
        None
    );
    assert!(matches!(
        state
            .oauth
            .refresh(
                mcp.refresh_token.as_deref().unwrap(),
                "test-client",
                &state.config.resource_url,
                Utc::now()
            )
            .await?,
        notegate_service::oauth::TokenOutcome::Rejected("invalid_grant")
    ));
    assert_eq!(
        state
            .oauth
            .access_user(
                &stranger_mcp.access_token,
                &state.config.resource_url,
                "mcp",
                Utc::now()
            )
            .await?,
        Some(stranger)
    );
    verify_browser_session(&state, &current).await?;
    let response = router
        .clone()
        .oneshot(request("DELETE", "/api/v1/me/sessions")?)
        .await?;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .any(|h| h.to_str().unwrap().contains("Max-Age=0"))
    );
    assert!(verify_browser_session(&state, &current).await.is_err());
    verify_browser_session(&state, &stranger_session).await?;
    db.cleanup().await;
    Ok(())
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
async fn upstream_jwts_and_malformed_tokens_never_authorize_requests()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test",
        db.pool.clone(),
    )?;
    let cases = [
        token(
            "sub-1",
            "https://auth.example.test",
            json!("https://api.example.test"),
            future_exp(),
            "kid-1",
        )?,
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
    for candidate in cases {
        assert!(matches!(
            verify_bearer_mcp(&state, &candidate).await,
            Err(AuthError::InvalidToken)
        ));
        assert!(matches!(
            verify_bearer_command(&state, &candidate).await,
            Err(AuthError::InvalidToken)
        ));
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn v1_routes_require_browser_session_before_handler() -> Result<(), Box<dyn std::error::Error>>
{
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    let response = app
        .oneshot(Request::builder().uri("/api/v1/me").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!response.headers().contains_key(WWW_AUTHENTICATE));
    Ok(())
}

#[tokio::test]
async fn v1_routes_reject_oauth_bearer() -> Result<(), Box<dyn std::error::Error>> {
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

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn v1_routes_reject_api_keys() -> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/me")
                .header("authorization", format!("Bearer {TEST_API_KEY}"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn openapi_docs_redirect_unauthenticated_browser_to_login()
-> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);

    for path in [
        "/swagger-ui/v2",
        "/swagger-ui/v2/",
        "/swagger-ui/v2/swagger-ui-bundle.js",
        "/swagger-ui/v2/swagger-ui.css",
        "/swagger-ui/v2/swagger-initializer.js",
        "/openapi/v2.json",
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty())?)
            .await?;

        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT, "{path}");
        assert_eq!(
            response.headers().get(CACHE_CONTROL),
            Some(&"private, no-store".parse()?),
            "{path}"
        );
        let location = response
            .headers()
            .get(LOCATION)
            .ok_or("missing login redirect location")?
            .to_str()?;
        let location = url::Url::parse(&format!("http://localhost{location}"))?;
        assert_eq!(location.path(), "/auth/login", "{path}");
        assert_eq!(
            location
                .query_pairs()
                .find(|(key, _)| key == "next")
                .map(|(_, value)| value.into_owned())
                .as_deref(),
            Some(path),
            "{path}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn openapi_docs_do_not_accept_agent_api_keys() -> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    for path in ["/swagger-ui/v2/swagger-ui-bundle.js", "/openapi/v2.json"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", format!("Bearer {TEST_API_KEY}"))
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT, "{path}");
        assert_eq!(
            response.headers().get(CACHE_CONTROL),
            Some(&"private, no-store".parse()?),
            "{path}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn openapi_docs_accept_valid_browser_session() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test",
        db.pool.clone(),
    )?;
    assert!(state.config.web_dist_dir.is_none());
    let session = valid_browser_session(&db, &state).await?;
    let app = crate::routes::app(state);

    for (path, content_type, expected_body) in [
        ("/swagger-ui/v2/", "text/html", "swagger-ui-bundle.js"),
        (
            "/swagger-ui/v2/index.html",
            "text/html",
            "swagger-ui-bundle.js",
        ),
        (
            "/swagger-ui/v2/swagger-ui-bundle.js",
            "text/javascript",
            "SwaggerUIBundle",
        ),
        (
            "/swagger-ui/v2/swagger-ui-standalone-preset.js",
            "text/javascript",
            "SwaggerUIStandalonePreset",
        ),
        ("/swagger-ui/v2/swagger-ui.css", "text/css", ".swagger-ui"),
        (
            "/swagger-ui/v2/swagger-initializer.js",
            "text/javascript",
            "/openapi/v2.json",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
                    .body(Body::empty())?,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            response.headers().get(CACHE_CONTROL),
            Some(&"private, no-store".parse()?),
            "{path}"
        );
        assert!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .ok_or("missing docs content type")?
                .to_str()?
                .starts_with(content_type),
            "{path}"
        );
        let body = to_bytes(response.into_body(), 8 * 1024 * 1024).await?;
        assert!(
            std::str::from_utf8(&body)?.contains(expected_body),
            "{path}"
        );
    }

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/swagger-ui/v2")
                .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get(LOCATION),
        Some(&"/swagger-ui/v2/".parse()?)
    );
    assert_eq!(
        response.headers().get(CACHE_CONTROL),
        Some(&"private, no-store".parse()?)
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/openapi/v2.json")
                .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .ok_or("missing OpenAPI content type")?
        .to_str()?;
    assert_eq!(content_type.split(';').next(), Some("application/json"));
    assert_eq!(
        response.headers().get(CACHE_CONTROL),
        Some(&"private, no-store".parse()?)
    );
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    let schema: Value = serde_json::from_slice(&body)?;
    assert_eq!(schema["openapi"], "3.1.0");
    assert_eq!(
        schema["paths"]["/api/v2/me"]["get"]["operationId"],
        "get_me"
    );
    assert_eq!(
        schema["components"]["securitySchemes"]["api_key"]["scheme"],
        "bearer"
    );

    let response = app
        .oneshot(
            Request::builder()
                .uri("/swagger-ui/v2/missing.css")
                .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers().get(CACHE_CONTROL),
        Some(&"private, no-store".parse()?)
    );
    db.cleanup().await;
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
async fn v2_api_key_requests_do_not_require_browser_origin()
-> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v2/missing")
                .header("authorization", format!("Bearer {TEST_API_KEY}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_json_error(response, "not_found", "api route not found").await?;
    Ok(())
}

#[tokio::test]
async fn v2_exposes_agent_resources_but_not_management_routes()
-> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v2/me")
                .header("authorization", format!("Bearer {TEST_API_KEY}"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    let body = to_bytes(response.into_body(), 16 * 1024).await?;
    let body: Value = serde_json::from_slice(&body)?;
    assert_eq!(body["account"]["kind"], "agent");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v2/spaces/not-a-uuid")
                .header("authorization", format!("Bearer {TEST_API_KEY}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response
            .headers()
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/agents")
                .header("authorization", format!("Bearer {TEST_API_KEY}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response
            .headers()
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    assert_json_error(response, "not_found", "api route not found").await?;
    Ok(())
}

#[tokio::test]
async fn v2_routes_require_api_keys() -> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    let response = app
        .oneshot(Request::builder().uri("/api/v2/me").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response
            .headers()
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    assert_eq!(
        response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer realm=\"notegate-public-api\"")
    );
    Ok(())
}

#[tokio::test]
async fn command_routes_require_bearer_auth() -> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    let response = app
        .oneshot(cli_request().body(cli_body("me", json!({})))?)
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response
            .headers()
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    assert_eq!(
        response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer realm=\"notegate-command-api\"")
    );
    Ok(())
}

#[tokio::test]
async fn command_routes_accept_agent_api_keys() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test",
        db.pool.clone(),
    )?;
    let app = crate::routes::app(state);
    let response = app
        .oneshot(
            cli_request()
                .header("authorization", format!("Bearer {TEST_API_KEY}"))
                .body(cli_body("me", json!({})))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    let body = to_bytes(response.into_body(), 16 * 1024).await?;
    let body: Value = serde_json::from_slice(&body)?;
    assert_eq!(body["account"]["kind"], "agent");
    assert!(
        body["server_version"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn command_routes_accept_cli_oauth_for_registered_users()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test",
        db.pool.clone(),
    )?;

    let _session = valid_browser_session(&db, &state).await?;
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
        .fetch_one(&db.pool)
        .await?;
    let oauth = local_access(&state, user_id, "http://localhost:9191/cli", "cli").await?;
    let app = crate::routes::app(state);

    let response = app
        .oneshot(
            cli_request()
                .header("authorization", format!("Bearer {oauth}"))
                .body(cli_body("me", json!({})))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 16 * 1024).await?;
    let body: Value = serde_json::from_slice(&body)?;
    assert_eq!(body["account"]["kind"], "user");
    assert!(body["user"].is_object());
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn issuer_scoped_identity_resolves_one_user_across_auth_channels()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let mut state = state_with_pool(
        ResolverMode::Missing,
        "https://api.example.test",
        db.pool.clone(),
    )?;
    let issuer = state.config.oidc.issuer.clone();
    let attrs = ResolveAttrs {
        issuer: issuer.clone(),
        sub: "legacy-channel-user".to_owned(),
        email: "legacy-channel@example.test".to_owned(),
        name: "Legacy Channel User".to_owned(),
    };
    let (account, _) = state.accounts.upsert_user_by_identity(&attrs).await?;
    state.resolver = Arc::new(notegate_service::identity::Resolver::new(
        state.accounts.clone(),
        notegate_db::ApiKeyRepo::with_lookup_key(
            db.pool.clone(),
            state.security.lookup_key_id(),
            state.security.version(),
        ),
        state.security.clone(),
    ));
    let mcp_token = local_access(&state, account.id, &state.config.resource_url, "mcp").await?;
    assert_eq!(
        verify_bearer_mcp(&state, &mcp_token).await?.account_id(),
        account.id
    );
    let cli_token = local_access(&state, account.id, "http://localhost:9191/cli", "cli").await?;
    let session = create_browser_session(&state, account.id, None).await?;
    let app = crate::routes::app(state.clone());
    for request in [
        cli_request()
            .header("authorization", format!("Bearer {cli_token}"))
            .body(cli_body("me", json!({})))?,
        Request::builder()
            .uri("/api/v1/me")
            .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
            .body(Body::empty())?,
    ] {
        let response = app.clone().oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 16 * 1024).await?)?;
        assert_eq!(body["account"]["id"], json!(account.id));
    }
    assert_eq!(
        state
            .accounts
            .find_user_id_by_identity("https://other.example.test", &attrs.sub)
            .await?,
        None
    );
    assert_eq!(
        state.accounts.upsert_user_by_identity(&attrs).await?.0.id,
        account.id
    );
    state
        .accounts
        .soft_delete_user(account.id, account.id)
        .await?;
    assert!(matches!(
        verify_bearer_mcp(&state, &mcp_token).await,
        Err(AuthError::InvalidToken)
    ));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn command_unregistered_user_error_is_transport_neutral()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Missing,
        "https://api.example.test",
        db.pool.clone(),
    )?;
    let _session = valid_browser_session(&db, &state).await?;
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
        .fetch_one(&db.pool)
        .await?;
    let oauth = local_access(&state, user_id, "http://localhost:9191/cli", "cli").await?;
    let app = crate::routes::app(state);

    let response = app
        .oneshot(
            cli_request()
                .header("authorization", format!("Bearer {oauth}"))
                .body(cli_body("me", json!({})))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(response.into_body(), 16 * 1024).await?;
    let body: Value = serde_json::from_slice(&body)?;
    assert_eq!(body["error"], "not_registered");
    assert_eq!(
        body["message"],
        "This account is authenticated but not registered in NoteGate yet. Open login_url once, then retry the request."
    );
    assert_eq!(body["login_url"], "http://localhost:9191/auth/login");
    assert!(!body["message"].as_str().unwrap_or_default().contains("MCP"));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn command_routes_reject_legacy_keys_and_mcp_audience_bearers()
-> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    let oauth = token(
        "sub-1",
        "https://auth.example.test",
        json!("https://api.example.test"),
        future_exp(),
        "kid-1",
    )?;

    for bearer in [
        TEST_LEGACY_API_KEY.to_owned(),
        "ngk_malformed".to_owned(),
        oauth,
    ] {
        let response = app
            .clone()
            .oneshot(
                cli_request()
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(cli_body("me", json!({})))?,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get(CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("private, no-store")
        );
        assert_eq!(
            response
                .headers()
                .get(WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer realm=\"notegate-command-api\"")
        );
    }
    Ok(())
}

#[tokio::test]
async fn command_api_key_shape_does_not_depend_on_jwks() -> Result<(), Box<dyn std::error::Error>> {
    let state = state(ResolverMode::Registered(true))?;
    let app = crate::routes::app(state);

    let response = app
        .oneshot(
            cli_request()
                .header("authorization", "Bearer ngk_malformed")
                .body(cli_body("me", json!({})))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn command_routes_preserve_recovery_errors_and_no_store()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test",
        db.pool.clone(),
    )?;
    let app = crate::routes::app(state);
    let response = app
        .oneshot(
            cli_request()
                .header("authorization", format!("Bearer {TEST_API_KEY}"))
                .body(cli_body(
                    "read",
                    json!({
                        "purpose": "read a text node",
                        "op": "read"
                    }),
                ))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response
            .headers()
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    let body = to_bytes(response.into_body(), 16 * 1024).await?;
    let body: Value = serde_json::from_slice(&body)?;
    assert_eq!(body["error"], "required_field_missing");
    assert_eq!(body["kind"], "invalid_input");
    assert_eq!(body["data"]["next_action"]["kind"], "add_fields");
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn v2_routes_reject_legacy_agent_keys() -> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state(ResolverMode::Registered(true))?);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/me")
                .header("authorization", format!("Bearer {TEST_LEGACY_API_KEY}"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn v2_routes_reject_oauth_bearer() -> Result<(), Box<dyn std::error::Error>> {
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
                .uri("/api/v2/me")
                .header("authorization", format!("Bearer {valid}"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn v2_routes_reject_browser_sessions() -> Result<(), Box<dyn std::error::Error>> {
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
                .uri("/api/v2/me")
                .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    db.cleanup().await;
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
        .clone()
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
    assert!(challenge.contains("scope=\"mcp offline_access\""));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/v2")
                .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer realm=\"notegate-agent-mcp-v2\"")
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn user_mcp_rejects_agent_api_keys() -> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state_with_resource(
        ResolverMode::Registered(true),
        "https://api.example.test/mcp",
    )?);
    for token in [TEST_API_KEY, TEST_LEGACY_API_KEY] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("authorization", format!("Bearer {token}"))
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
    }
    Ok(())
}

#[tokio::test]
async fn user_mcp_accepts_oauth_bearer() -> Result<(), Box<dyn std::error::Error>> {
    let resource_url = "https://api.example.test/mcp";
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        resource_url,
        db.pool.clone(),
    )?;
    let _session = valid_browser_session(&db, &state).await?;
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
        .fetch_one(&db.pool)
        .await?;
    let oauth = local_access(&state, user_id, resource_url, "mcp").await?;
    let app = crate::routes::app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("host", "api.example.test")
                .header("authorization", format!("Bearer {oauth}"))
                .header(ACCEPT, "application/json, text/event-stream")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(MCP_INITIALIZE_REQUEST))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn user_mcp_rejects_cli_audience_bearer() -> Result<(), Box<dyn std::error::Error>> {
    let resource_url = "https://api.example.test/mcp";
    let app = crate::routes::app(state_with_resource(
        ResolverMode::Registered(true),
        resource_url,
    )?);
    let oauth = token(
        "sub-1",
        "https://auth.example.test",
        json!("notegate-cli-local"),
        future_exp(),
        "kid-1",
    )?;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("authorization", format!("Bearer {oauth}"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn agent_mcp_v2_accepts_v2_and_rejects_legacy_keys() -> Result<(), Box<dyn std::error::Error>>
{
    let app = crate::routes::app(state_with_resource(
        ResolverMode::Registered(true),
        "https://api.example.test/mcp",
    )?);

    let accepted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/v2")
                .header("host", "api.example.test")
                .header("authorization", format!("Bearer {TEST_API_KEY}"))
                .header(ACCEPT, "application/json, text/event-stream")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(MCP_INITIALIZE_REQUEST))?,
        )
        .await?;
    assert_eq!(accepted.status(), StatusCode::OK);

    let legacy = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/v2")
                .header("host", "api.example.test")
                .header("authorization", format!("Bearer {TEST_LEGACY_API_KEY}"))
                .header(ACCEPT, "application/json, text/event-stream")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(MCP_INITIALIZE_REQUEST))?,
        )
        .await?;
    assert_eq!(legacy.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn agent_mcp_v2_rejects_oauth_bearer() -> Result<(), Box<dyn std::error::Error>> {
    let app = crate::routes::app(state_with_resource(
        ResolverMode::Registered(true),
        "https://api.example.test/mcp",
    )?);
    let oauth = token(
        "sub-1",
        "https://auth.example.test",
        json!("https://api.example.test/mcp"),
        future_exp(),
        "kid-1",
    )?;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/v2")
                .header("authorization", format!("Bearer {oauth}"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer realm=\"notegate-agent-mcp-v2\"")
    );
    Ok(())
}

#[tokio::test]
async fn unknown_v1_routes_still_require_browser_session() -> Result<(), Box<dyn std::error::Error>>
{
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

    for (path, content_type, cache_control) in [
        (
            "/auth/success",
            "text/html; charset=utf-8",
            Some("no-store"),
        ),
        ("/auth/ui.css", "text/css; charset=utf-8", Some("no-cache")),
        (
            "/auth/ui.js",
            "text/javascript; charset=utf-8",
            Some("no-cache"),
        ),
        (
            "/auth/login-complete.js",
            "text/javascript; charset=utf-8",
            Some("private, no-store"),
        ),
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
            Some(content_type)
        );
        assert_eq!(
            response
                .headers()
                .get(CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            cache_control
        );
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

    let agent_metadata = app
        .oneshot(
            Request::builder()
                .uri("/.well-known/oauth-protected-resource/mcp/v2")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(agent_metadata.status(), StatusCode::NOT_FOUND);
    Ok(())
}

#[tokio::test]
async fn logout_does_not_expire_cookie_when_local_revoke_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test/mcp",
        db.pool.clone(),
    )?;
    db.pool.close().await;
    let session = format_token(Uuid::new_v4(), "secret");
    let app = crate::routes::app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/logout")
                .header("origin", "http://localhost:9191")
                .header("cookie", format!("{BROWSER_SESSION_COOKIE}={session}"))
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        response.headers().get(SET_COOKIE).is_none(),
        "local logout failure must not clear the browser cookie"
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn session_marker_migration_preserves_authentication_decisions()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup_before(60).await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test/mcp",
        db.pool.clone(),
    )?;
    let (owner, _) = AccountRepo::with_crypto(db.pool.clone(), state.security.clone())
        .upsert_user_by_identity(&ResolveAttrs {
            issuer: state.oidc.issuer().to_owned(),
            sub: "session-migration".to_owned(),
            email: "session@example.test".to_owned(),
            name: "Session User".to_owned(),
        })
        .await?;
    let mut tokens = Vec::new();
    for case in [
        "current",
        "unbound",
        "wrong_issuer",
        "revoked",
        "stale",
        "expired",
    ] {
        let id = Uuid::new_v4();
        let issuer = match case {
            "unbound" => None,
            "wrong_issuer" => Some("https://other.example.test"),
            _ => Some(state.oidc.issuer()),
        };
        let hash = state
            .security
            .browser_session_hash(&id.to_string(), "retained-secret")?;
        sqlx::query(
            "INSERT INTO browser_sessions \
             (id,user_id,token_prefix,token_hash,hash_key_id,hash_version,local_session,oidc_issuer,validated_until,expires_at,revoked_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8, \
               CASE WHEN $9='expired' THEN now()-interval '2 hours' WHEN $9='stale' THEN now()-interval '1 hour' ELSE now()+interval '1 hour' END, \
               CASE WHEN $9='expired' THEN now()-interval '1 hour' ELSE now()+interval '7 days' END, \
               CASE WHEN $9='revoked' THEN now() ELSE NULL END)",
        )
        .bind(id).bind(owner.id).bind(notegate_db::browser_session_repo::token_prefix(id))
        .bind(hash).bind(state.security.lookup_key_id()).bind(state.security.version())
        .bind(issuer.is_some()).bind(issuer).bind(case).execute(&db.pool).await?;
        tokens.push((case, format_token(id, "retained-secret")));
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM browser_sessions WHERE oidc_issuer=$1 \
         AND revoked_at IS NULL AND validated_until>now() AND expires_at>now()",
        )
        .bind(state.oidc.issuer())
        .fetch_one(&db.pool)
        .await?,
        1
    );
    db.apply_migration(60).await?;
    db.apply_migration(61).await?;
    for (case, token) in &tokens {
        match verify_browser_session(&state, token).await {
            Ok(caller) => {
                assert_eq!(*case, "current");
                assert_eq!(caller.account_id(), owner.id);
            }
            Err(error) => {
                assert_ne!(*case, "current", "{error:?}");
                assert!(matches!(error, AuthError::InvalidToken));
            }
        }
    }
    let fresh = create_browser_session(&state, owner.id, None).await?;
    assert_eq!(
        verify_browser_session(&state, &fresh).await?.account_id(),
        owner.id
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM browser_sessions")
            .fetch_one(&db.pool)
            .await?,
        7
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn legacy_browser_sessions_are_rejected_without_deleting_account_or_history()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test/mcp",
        db.pool.clone(),
    )?;
    let session = valid_browser_session(&db, &state).await?;
    let (id, _) = notegate_db::browser_session_repo::parse_token(&session).ok_or("bad session")?;
    sqlx::query("UPDATE browser_sessions SET oidc_issuer=NULL WHERE id=$1")
        .bind(id)
        .execute(&db.pool)
        .await?;
    assert!(matches!(
        verify_browser_session(&state, &session).await,
        Err(AuthError::InvalidToken)
    ));
    let rows: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM users), (SELECT count(*) FROM browser_sessions), (SELECT count(*) FROM audit_events WHERE op_type='session.login')")
        .fetch_one(&db.pool).await?;
    assert_eq!(rows, (1, 1, 1));
    db.cleanup().await;
    Ok(())
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

pub(super) fn standalone_state(db: &TestDb) -> Result<AppState, Box<dyn std::error::Error>> {
    let mut state = state_with_pool(
        ResolverMode::Missing,
        "http://localhost:9191/mcp",
        db.pool.clone(),
    )?;
    let mut config = (*state.config).clone();
    config.oidc = notegate_core::config::OidcLoginConfig {
        issuer: "https://accounts.google.com".to_owned(),
        client_id: "notegate-web".to_owned(),
        client_secret: Some(secrecy::SecretString::from("test-google-secret".to_owned())),
        allow_signup: false,
    };
    state.config = Arc::new(config);
    state.oidc = Arc::new(crate::auth::oidc::OidcProvider::new(
        &state.config,
        state.http.clone(),
    ));
    state.resolver = Arc::new(notegate_service::identity::Resolver::new(
        state.accounts.clone(),
        notegate_db::ApiKeyRepo::with_lookup_key(
            db.pool.clone(),
            state.security.lookup_key_id(),
            state.security.version(),
        ),
        state.security.clone(),
    ));
    Ok(state)
}

#[path = "standalone_tests.rs"]
mod standalone;

#[tokio::test]
async fn browser_activity_requires_live_cookie_and_same_origin_without_reauthenticating()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_pool(
        ResolverMode::Registered(true),
        "https://api.example.test",
        db.pool.clone(),
    )?;
    let token = valid_browser_session(&db, &state).await?;
    let (id, _) =
        notegate_db::browser_session_repo::parse_token(&token).ok_or("invalid fixture")?;
    let (initial_idle, absolute): (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as("SELECT idle_expires_at,expires_at FROM browser_sessions WHERE id=$1")
            .bind(id)
            .fetch_one(&db.pool)
            .await?;
    assert!(
        initial_idle
            > chrono::Utc::now() + chrono::Duration::days(14) - chrono::Duration::seconds(5)
    );
    assert!(initial_idle <= absolute);
    sqlx::query("UPDATE browser_sessions SET idle_expires_at=now()+interval '1 hour' WHERE id=$1")
        .bind(id)
        .execute(&db.pool)
        .await?;
    let before: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(t) FROM browser_sessions t WHERE id=$1")
            .bind(id)
            .fetch_one(&db.pool)
            .await?;
    let router = crate::routes::app(state.clone());
    for (origin, credential, authorization, status) in [
        (None, true, false, StatusCode::FORBIDDEN),
        (
            Some("https://attacker.test"),
            true,
            false,
            StatusCode::FORBIDDEN,
        ),
        (
            Some("https://api.example.test"),
            false,
            false,
            StatusCode::UNAUTHORIZED,
        ),
        (
            Some("https://api.example.test"),
            true,
            true,
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/auth/session/activity");
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        if credential {
            request = request.header(header::COOKIE, format!("{BROWSER_SESSION_COOKIE}={token}"));
        }
        if authorization {
            request = request.header(header::AUTHORIZATION, "Bearer rejected");
        }
        let response = router.clone().oneshot(request.body(Body::empty())?).await?;
        assert_eq!(response.status(), status);
    }
    assert_eq!(
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT to_jsonb(t) FROM browser_sessions t WHERE id=$1"
        )
        .bind(id)
        .fetch_one(&db.pool)
        .await?,
        before
    );
    verify_browser_session(&state, &token).await?;
    assert_eq!(
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT to_jsonb(idle_expires_at) FROM browser_sessions WHERE id=$1"
        )
        .bind(id)
        .fetch_one(&db.pool)
        .await?,
        before["idle_expires_at"]
    );
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/session/activity")
                .header(header::ORIGIN, "https://api.example.test")
                .header(header::COOKIE, format!("{BROWSER_SESSION_COOKIE}={token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    let after: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(t) FROM browser_sessions t WHERE id=$1")
            .bind(id)
            .fetch_one(&db.pool)
            .await?;
    for field in [
        "id",
        "user_id",
        "token_hash",
        "hash_key_id",
        "hash_version",
        "oidc_issuer",
        "created_at",
        "expires_at",
    ] {
        assert_eq!(after[field], before[field], "{field}");
    }
    assert_ne!(after["idle_expires_at"], before["idle_expires_at"]);
    for reason in ["idle", "absolute", "issuer", "revoked"] {
        sqlx::query("UPDATE browser_sessions SET \
          idle_expires_at=CASE WHEN $2 IN ('idle','absolute') THEN now() ELSE now()+interval '1 hour' END, \
          expires_at=CASE WHEN $2='absolute' THEN now() ELSE now()+interval '1 day' END, \
          oidc_issuer=CASE WHEN $2='issuer' THEN 'https://other.test' ELSE $3 END, \
          revoked_at=CASE WHEN $2='revoked' THEN now() ELSE NULL END WHERE id=$1")
          .bind(id).bind(reason).bind(state.oidc.issuer()).execute(&db.pool).await?;
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/session/activity")
                    .header(header::ORIGIN, "https://api.example.test")
                    .header(header::COOKIE, format!("{BROWSER_SESSION_COOKIE}={token}"))
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{reason}");
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn activity_uses_browser_capacity_without_consuming_login_or_logout_capacity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = state(ResolverMode::Registered(true))?;
    let mut config = (*state.config).clone();
    config.http_rate_limits.browser_v1 = notegate_core::HttpRateLimitConfig {
        requests_per_second: 1,
        burst: 1,
    };
    state.config = Arc::new(config);
    let router = crate::routes::app(state);
    for attempt in 0..31 {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/session/activity")
                    .header(header::ORIGIN, "https://api.example.test")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(
            response.status(),
            if attempt == 0 {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::TOO_MANY_REQUESTS
            }
        );
    }
    let read = router
        .clone()
        .oneshot(Request::builder().uri("/api/v1/me").body(Body::empty())?)
        .await?;
    assert_eq!(read.status(), StatusCode::TOO_MANY_REQUESTS);
    let login_page = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/auth/success")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(login_page.status(), StatusCode::OK);
    let logout = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/logout")
                .header(header::ORIGIN, "https://api.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(logout.status(), StatusCode::SEE_OTHER);
    Ok(())
}
