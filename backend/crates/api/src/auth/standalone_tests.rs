#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
use super::standalone_state;
use crate::auth::{
    authorization_server,
    session::{BROWSER_SESSION_COOKIE, create_browser_session},
};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode, header},
    response::Response,
};
use chrono::{Duration, Utc};
use notegate_db::{oauth_repo::Authorization, test_support::TestDb};
use notegate_model::ResolveAttrs;
use notegate_service::oauth::pkce_challenge;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

async fn body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap()
}
fn app(state: crate::state::AppState) -> Router {
    authorization_server::routes().with_state(state)
}
async fn post(
    app: &Router,
    path: &str,
    form: &str,
    cookie: Option<&str>,
    origin: Option<&str>,
) -> Response {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    if let Some(origin) = origin {
        request = request.header(header::ORIGIN, origin);
    }
    app.clone()
        .oneshot(request.body(Body::from(form.to_owned())).unwrap())
        .await
        .unwrap()
}
async fn owner(state: &crate::state::AppState) -> Uuid {
    state
        .accounts
        .upsert_user_by_identity(&ResolveAttrs {
            issuer: state.oidc.issuer().to_owned(),
            sub: "google-user".to_owned(),
            email: "user@test".to_owned(),
            name: "User".to_owned(),
        })
        .await
        .unwrap()
        .0
        .id
}

#[tokio::test]
async fn device_http_flow_rejects_csrf_and_uses_own_tokens_without_authgate()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = standalone_state(&db)?;
    let user = owner(&state).await;
    let session = create_browser_session(&state, user, None).await?;
    let cookie = format!("{BROWSER_SESSION_COOKIE}={session}");
    let router = app(state.clone());
    let device = body(
        post(
            &router,
            "/oauth/device/authorize",
            "client_id=notegate-cli-local&scope=cli+offline_access",
            None,
            None,
        )
        .await,
    )
    .await;
    assert!(device["device_code"].is_string());
    let approval = format!(
        "user_code={}&decision=approve",
        device["user_code"].as_str().unwrap()
    );
    assert_eq!(
        post(
            &router,
            "/oauth/device",
            &approval,
            Some(&cookie),
            Some("https://evil.test")
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post(
            &router,
            "/oauth/device",
            &approval,
            Some(&cookie),
            Some("http://localhost:9191")
        )
        .await
        .status(),
        StatusCode::OK
    );
    let poll = format!(
        "grant_type=urn:ietf:params:oauth:grant-type:device_code&client_id=notegate-cli-local&device_code={}",
        device["device_code"].as_str().unwrap()
    );
    let response = post(&router, "/oauth/token", &poll, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let tokens = body(response).await;
    let access = tokens["access_token"].as_str().unwrap();
    let refresh = tokens["refresh_token"].as_str().unwrap();
    assert_eq!(
        crate::auth::bearer::verify_bearer_command(&state, access)
            .await?
            .account_id(),
        user
    );
    assert!(
        crate::auth::bearer::verify_bearer_mcp(&state, access)
            .await
            .is_err()
    );
    let first = post(
        &router,
        "/oauth/token",
        &format!("grant_type=refresh_token&client_id=notegate-cli-local&scope=offline_access+cli&refresh_token={refresh}"),
        None,
        None,
    )
    .await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(
        post(
            &router,
            "/oauth/token",
            &format!(
                "grant_type=refresh_token&client_id=notegate-cli-local&refresh_token={refresh}"
            ),
            None,
            None
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let rotated = body(first).await;
    assert!(
        crate::auth::bearer::verify_bearer_command(
            &state,
            rotated["access_token"].as_str().unwrap()
        )
        .await
        .is_err()
    );
    let metadata = body(
        crate::auth::metadata::authorization_server_metadata(State(state))
            .await
            .into_response(),
    )
    .await;
    assert_eq!(metadata["issuer"], "http://localhost:9191");
    assert_eq!(metadata["client_id_metadata_document_supported"], true);
    assert_eq!(
        metadata["authorization_response_iss_parameter_supported"],
        true
    );
    assert_eq!(
        metadata["token_endpoint_auth_methods_supported"],
        json!(["none"])
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn authorization_consent_is_bound_single_use_and_explicit()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = standalone_state(&db)?;
    let user = owner(&state).await;
    let session = create_browser_session(&state, user, None).await?;
    let id = Uuid::new_v4();
    let flow_secret = "flow-browser-secret";
    let verifier = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGH";
    let encrypted = state.security.encrypt_oauth_state(
        &id.to_string(),
        &serde_json::to_string(&Some("client-state"))?,
    )?;
    let auth = Authorization {
        id,
        client_id: "https://client.test/metadata".to_owned(),
        client_name: "Untrusted <script>".to_owned(),
        resource: state.config.resource_url.clone(),
        redirect_uri: "https://client.test:8443/callback?flow=browser".to_owned(),
        scope: "mcp offline_access".to_owned(),
        code_challenge: pkce_challenge(verifier),
        cookie_hash: state
            .security
            .oauth_hash("authorization-cookie", flow_secret)?,
        private_payload: serde_json::to_value(encrypted)?,
        user_id: None,
        code_hash: None,
        expires_at: Utc::now() + Duration::minutes(5),
        consumed_at: None,
    };
    state.oauth.store.insert_authorization(&auth).await?;
    let cookie =
        format!("{BROWSER_SESSION_COOKIE}={session}; notegate_authorization_{id}={flow_secret}");
    let router = app(state.clone());
    let get = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/oauth/approve?id={id}"))
                .header(header::COOKIE, &cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(get.status(), StatusCode::OK);
    assert_eq!(
        get.headers()[header::CONTENT_SECURITY_POLICY].to_str()?,
        "default-src 'none'; style-src 'self'; script-src 'self'; form-action 'self' https://client.test:8443; frame-ancestors 'none'; base-uri 'none'"
    );
    let html = String::from_utf8(to_bytes(get.into_body(), 100_000).await?.to_vec())?;
    assert!(html.contains("Untrusted &lt;script&gt;"));
    assert!(html.contains(&auth.redirect_uri));
    let pending: i64 =
        sqlx::query_scalar("SELECT count(*) FROM oauth_authorizations WHERE code_hash IS NOT NULL")
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(pending, 0);
    let form = format!("id={id}&decision=approve");
    assert_eq!(
        post(&router, "/oauth/approve", &form, Some(&cookie), None)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let missing = format!("{BROWSER_SESSION_COOKIE}={session}");
    assert_eq!(
        post(
            &router,
            "/oauth/approve",
            &form,
            Some(&missing),
            Some("http://localhost:9191")
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let response = post(
        &router,
        "/oauth/approve",
        &form,
        Some(&cookie),
        Some("http://localhost:9191"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let redirect = url::Url::parse(response.headers()[header::LOCATION].to_str()?)?;
    let query: std::collections::HashMap<_, _> = redirect.query_pairs().into_owned().collect();
    assert_eq!(query["state"], "client-state");
    assert_eq!(query["iss"], "http://localhost:9191");
    assert_eq!(
        post(
            &router,
            "/oauth/approve",
            &form,
            Some(&cookie),
            Some("http://localhost:9191")
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let params = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("client_id", &auth.client_id)
        .append_pair("resource", &auth.resource)
        .append_pair("redirect_uri", &auth.redirect_uri)
        .append_pair("code", &query["code"])
        .append_pair("code_verifier", verifier)
        .finish();
    let tokens = body(post(&router, "/oauth/token", &params, None, None).await).await;
    let access = tokens["access_token"].as_str().ok_or("no access token")?;
    assert_eq!(
        crate::auth::bearer::verify_bearer_mcp(&state, access)
            .await?
            .account_id(),
        user
    );
    assert!(
        crate::auth::bearer::verify_bearer_command(&state, access)
            .await
            .is_err()
    );
    let connections = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/oauth/connections")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(connections.status(), StatusCode::OK);
    let grant = state
        .oauth
        .store
        .list_grants(user, Utc::now())
        .await?
        .pop()
        .ok_or("no grant")?;
    let other = state
        .accounts
        .upsert_user_by_identity(&ResolveAttrs {
            issuer: state.oidc.issuer().to_owned(),
            sub: "other-subject".to_owned(),
            email: "other@test".to_owned(),
            name: "Other".to_owned(),
        })
        .await?
        .0
        .id;
    let other_cookie = format!(
        "{BROWSER_SESSION_COOKIE}={}",
        create_browser_session(&state, other, None).await?
    );
    post(
        &router,
        "/oauth/connections/revoke",
        &format!("id={}", grant.id),
        Some(&other_cookie),
        Some("http://localhost:9191"),
    )
    .await;
    assert!(
        crate::auth::bearer::verify_bearer_mcp(&state, access)
            .await
            .is_ok()
    );
    assert!(
        state
            .oauth
            .store
            .list_grants(other, Utc::now())
            .await?
            .is_empty()
    );
    assert_eq!(
        post(
            &router,
            "/oauth/connections/revoke",
            &format!("id={}", grant.id),
            Some(&cookie),
            Some("http://localhost:9191")
        )
        .await
        .status(),
        StatusCode::SEE_OTHER
    );
    assert!(
        crate::auth::bearer::verify_bearer_mcp(&state, access)
            .await
            .is_err()
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn local_sessions_reject_old_issuer_and_expire_without_upstream_refresh()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = standalone_state(&db)?;
    let user = owner(&state).await;
    let session = create_browser_session(&state, user, None).await?;
    assert_eq!(
        crate::auth::session::verify_browser_session(&state, &session)
            .await?
            .account_id(),
        user
    );
    sqlx::query("UPDATE browser_sessions SET oidc_issuer='https://other.test' WHERE user_id=$1")
        .bind(user)
        .execute(&db.pool)
        .await?;
    assert!(
        crate::auth::session::verify_browser_session(&state, &session)
            .await
            .is_err()
    );
    sqlx::query("UPDATE browser_sessions SET oidc_issuer=$2,idle_expires_at=now()-interval '1 second' WHERE user_id=$1").bind(user).bind(state.oidc.issuer()).execute(&db.pool).await?;
    assert!(
        crate::auth::session::verify_browser_session(&state, &session)
            .await
            .is_err()
    );
    db.cleanup().await;
    Ok(())
}

use axum::response::IntoResponse as _;

#[tokio::test]
async fn malformed_oauth_requests_return_private_protocol_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let router = app(standalone_state(&db)?);
    for path in ["/oauth/token", "/oauth/revoke", "/oauth/device/authorize"] {
        let response = post(&router, path, "unexpected=value", None, None).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(body(response).await["error"], "invalid_request");
    }
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/oauth/token")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(header::AUTHORIZATION, "Basic dGVzdDpwYXNzd29yZA==")
                .body(Body::from(
                    "grant_type=refresh_token&client_id=client&refresh_token=token",
                ))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
    assert_eq!(body(response).await["error"], "invalid_client");
    db.cleanup().await;
    Ok(())
}
