//! Public-client OAuth endpoints. Only registered CIMD redirects receive codes.
mod clients;
pub(crate) use clients::ClientRegistry;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use axum_extra::extract::CookieJar;
use chrono::{Duration, Utc};
use notegate_db::oauth_repo::Authorization;
use notegate_model::Caller;
use notegate_service::oauth::{CODE_SECONDS, FLOW_SECONDS, TokenOutcome, random_secret};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::oauth_flow::hardened_cookie;
use super::origin::has_trusted_browser_origin;
use super::page::escape_html;
use super::session::{BROWSER_SESSION_COOKIE, verify_browser_session};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/approve", get(consent).post(approve))
        .route("/oauth/token", post(token))
        .route("/oauth/revoke", post(revoke))
        .route("/oauth/device/authorize", post(device_authorize))
        .route("/oauth/device", get(device_page).post(device_approve))
        .route("/oauth/connections", get(connections))
        .route("/oauth/connections/revoke", post(revoke_connection))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024))
        .layer(axum::middleware::from_fn(private_response))
}

#[derive(Deserialize)]
struct AuthorizeRequest {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    resource: String,
    scope: String,
    state: Option<String>,
    code_challenge: String,
    code_challenge_method: String,
}

async fn authorize(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(request): Query<AuthorizeRequest>,
) -> Response {
    match prepare_authorization(&state, request).await {
        Ok((id, secret)) => (
            jar.add(hardened_cookie(
                auth_cookie(id),
                secret,
                FLOW_SECONDS,
                state.config.secure_cookies,
            )),
            Redirect::to(&format!("/oauth/approve?id={id}")),
        )
            .into_response(),
        Err(error) => oauth_error(error),
    }
}

async fn prepare_authorization(
    state: &AppState,
    request: AuthorizeRequest,
) -> Result<(Uuid, String), &'static str> {
    if request.response_type != "code" {
        return Err("unsupported_response_type");
    }
    if request.resource != state.config.resource_url {
        return Err("invalid_target");
    }
    let scope = validate_scope(&request.scope, "mcp")?;
    if request.state.as_ref().is_some_and(|s| s.len() > 1024)
        || request.code_challenge_method != "S256"
        || request.code_challenge.len() != 43
        || !request
            .code_challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("invalid_request");
    }
    let client = state.oauth_clients.get(&request.client_id).await?;
    if !client.allows_redirect(&request.redirect_uri) {
        return Err("invalid_request");
    }
    let id = Uuid::new_v4();
    let secret = random_secret();
    let private = state
        .security
        .encrypt_oauth_state(
            &id.to_string(),
            &serde_json::to_string(&request.state).map_err(|_| "server_error")?,
        )
        .map_err(|_| "server_error")?;
    let auth = Authorization {
        id,
        client_id: client.client_id,
        client_name: client.client_name,
        resource: request.resource,
        redirect_uri: request.redirect_uri,
        scope,
        code_challenge: request.code_challenge,
        cookie_hash: state
            .security
            .oauth_hash("authorization-cookie", &secret)
            .map_err(|_| "server_error")?,
        private_payload: serde_json::to_value(private).map_err(|_| "server_error")?,
        user_id: None,
        code_hash: None,
        expires_at: Utc::now() + Duration::seconds(FLOW_SECONDS),
        consumed_at: None,
    };
    state
        .oauth
        .store
        .insert_authorization(&auth)
        .await
        .map_err(|_| "server_error")?;
    Ok((id, secret))
}

#[derive(Deserialize)]
struct ConsentQuery {
    id: Uuid,
}
#[derive(Deserialize)]
struct ConsentForm {
    id: Uuid,
    decision: String,
}

async fn consent(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<ConsentQuery>,
) -> Response {
    let next = format!("/oauth/approve?id={}", query.id);
    let Some(caller) = recent_user(&state, &jar).await else {
        return login_redirect(&next);
    };
    let auth = match bound_authorization(&state, &jar, query.id).await {
        Ok(auth) => auth,
        Err(error) => return oauth_error(error),
    };
    let name = escape_html(&auth.client_name);
    let client = escape_html(&auth.client_id);
    let resource = escape_html(&auth.resource);
    let scope = escape_html(&auth.scope);
    let account = escape_html(&caller.account.display_name);
    page(
        "Connect application",
        &format!(
            "<p>Signed in as <strong>{account}</strong>.</p><p><strong>{name}</strong> requests access to your NoteGate documents in Spaces with MCP &amp; API access enabled, subject to existing permissions.</p><p>Client: <code>{client}</code><br>Redirect: <code>{}</code><br>Resource: <code>{resource}</code><br>Scopes: <code>{scope}</code></p><p>If the redirect is localhost or a loopback IP, only continue if you started this login in an application on this device.</p><p>Offline access lets this application reconnect until its grant expires or you revoke it.</p><form method=post action=/oauth/approve><input type=hidden name=id value=\"{}\"><button name=decision value=approve>Allow access</button> <button name=decision value=deny>Deny</button></form>",
            escape_html(&auth.redirect_uri),
            auth.id
        ),
    )
}

async fn bound_authorization(
    state: &AppState,
    jar: &CookieJar,
    id: Uuid,
) -> Result<Authorization, &'static str> {
    let secret = jar.get(&auth_cookie(id)).ok_or("invalid_request")?.value();
    let hash = state
        .security
        .oauth_hash("authorization-cookie", secret)
        .map_err(|_| "server_error")?;
    state
        .oauth
        .store
        .find_authorization(id, &hash, Utc::now())
        .await
        .map_err(|_| "server_error")?
        .ok_or("invalid_request")
}

async fn approve(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<ConsentForm>,
) -> Response {
    if !has_trusted_browser_origin(&headers, &state) {
        return oauth_error("invalid_request");
    }
    let Some(caller) = recent_user(&state, &jar).await else {
        return oauth_error("login_required");
    };
    let auth = match bound_authorization(&state, &jar, form.id).await {
        Ok(auth) => auth,
        Err(error) => return oauth_error(error),
    };
    let mut redirect = match url::Url::parse(&auth.redirect_uri) {
        Ok(url) => url,
        Err(_) => return oauth_error("server_error"),
    };
    let result = async {
        if form.decision == "approve" {
            let code = random_secret();
            let hash = state.security.oauth_hash("code", &code)?;
            if !state
                .oauth
                .store
                .approve_authorization(
                    auth.id,
                    &auth.cookie_hash,
                    caller.account_id(),
                    &hash,
                    Utc::now(),
                    Utc::now() + Duration::seconds(CODE_SECONDS),
                )
                .await?
            {
                return Ok::<_, notegate_core::Error>(None);
            }
            Ok(Some(("code", code)))
        } else if form.decision == "deny" {
            state
                .oauth
                .store
                .deny_authorization(auth.id, &auth.cookie_hash)
                .await?;
            Ok(Some(("error", "access_denied".to_owned())))
        } else {
            Ok(None)
        }
    }
    .await;
    let pair = match result {
        Ok(Some(pair)) => pair,
        Ok(None) => return oauth_error("invalid_request"),
        Err(_) => return oauth_error("server_error"),
    };
    let original_state = match authorization_state(&state, &auth) {
        Ok(value) => value,
        Err(_) => return oauth_error("server_error"),
    };
    {
        let mut query = redirect.query_pairs_mut();
        query.append_pair(pair.0, &pair.1);
        if let Some(value) = original_state {
            query.append_pair("state", &value);
        }
        query.append_pair("iss", &state.config.notegate_public_url);
    }
    let jar = jar.add(hardened_cookie(
        auth_cookie(auth.id),
        String::new(),
        0,
        state.config.secure_cookies,
    ));
    (jar, Redirect::to(redirect.as_str())).into_response()
}

fn authorization_state(
    state: &AppState,
    auth: &Authorization,
) -> notegate_core::Result<Option<String>> {
    let encrypted = serde_json::from_value(auth.private_payload.clone())
        .map_err(|_| notegate_core::Error::internal("invalid OAuth state"))?;
    let plaintext = state
        .security
        .decrypt_oauth_state(&auth.id.to_string(), &encrypted)?;
    serde_json::from_str(&plaintext)
        .map_err(|_| notegate_core::Error::internal("invalid OAuth state"))
}

#[derive(Deserialize)]
struct TokenForm {
    grant_type: String,
    client_id: String,
    resource: Option<String>,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
    device_code: Option<String>,
    scope: Option<String>,
    client_secret: Option<String>,
}

async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    form: Result<Form<TokenForm>, axum::extract::rejection::FormRejection>,
) -> Response {
    let Form(form) = match form {
        Ok(form) => form,
        Err(_) => return oauth_error("invalid_request"),
    };
    if let Some(response) = invalid_client_auth(&headers, form.client_secret.is_some()) {
        return response;
    }
    let resource = match token_resource(&state, &form.client_id, form.resource.as_deref()) {
        Ok(resource) => resource,
        Err(error) => return oauth_error(error),
    };
    // Refresh may repeat the original scope. Scope expansion/downscoping is not advertised.
    if form.scope.is_some() && form.grant_type != "refresh_token" {
        return oauth_error("invalid_scope");
    }
    let requested_scope = match form.scope.as_deref() {
        Some(scope) => match validate_scope(
            scope,
            if form.client_id == state.config.cli_oauth_client_id {
                "cli"
            } else {
                "mcp"
            },
        ) {
            Ok(scope) => Some(scope),
            Err(error) => return oauth_error(error),
        },
        None => None,
    };
    let now = Utc::now();
    let result = match form.grant_type.as_str() {
        "authorization_code" => match (form.code, form.redirect_uri, form.code_verifier) {
            (Some(code), Some(redirect), Some(verifier)) => {
                state
                    .oauth
                    .exchange_code(&code, &form.client_id, &resource, &redirect, &verifier, now)
                    .await
            }
            _ => return oauth_error("invalid_request"),
        },
        "refresh_token" => match form.refresh_token {
            Some(token) => {
                state
                    .oauth
                    .refresh_with_scope(
                        &token,
                        &form.client_id,
                        &resource,
                        requested_scope.as_deref(),
                        now,
                    )
                    .await
            }
            None => return oauth_error("invalid_request"),
        },
        "urn:ietf:params:oauth:grant-type:device_code"
            if form.client_id == state.config.cli_oauth_client_id =>
        {
            match form.device_code {
                Some(code) => {
                    state
                        .oauth
                        .poll_device(&code, &form.client_id, &resource, now)
                        .await
                }
                None => return oauth_error("invalid_request"),
            }
        }
        _ => return oauth_error("unsupported_grant_type"),
    };
    match result {
        Ok(TokenOutcome::Issued(tokens)) => no_store(Json(tokens).into_response()),
        Ok(TokenOutcome::Rejected(error)) => oauth_error(error),
        Err(error) => {
            tracing::error!(event="oauth.token_failed",%error);
            oauth_error("server_error")
        }
    }
}

fn token_resource(
    state: &AppState,
    client: &str,
    resource: Option<&str>,
) -> Result<String, &'static str> {
    if client.is_empty() || client.len() > 2048 {
        return Err("invalid_client");
    }
    let cli = format!("{}/cli", state.config.notegate_public_url);
    if client == state.config.cli_oauth_client_id {
        if resource.is_some_and(|r| r != cli) {
            return Err("invalid_target");
        }
        Ok(cli)
    } else if resource == Some(state.config.resource_url.as_str()) {
        Ok(state.config.resource_url.clone())
    } else {
        Err("invalid_target")
    }
}

#[derive(Deserialize)]
struct RevokeForm {
    token: String,
    client_id: String,
    client_secret: Option<String>,
}
async fn revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    form: Result<Form<RevokeForm>, axum::extract::rejection::FormRejection>,
) -> Response {
    let Form(form) = match form {
        Ok(form) => form,
        Err(_) => return oauth_error("invalid_request"),
    };
    if let Some(response) = invalid_client_auth(&headers, form.client_secret.is_some()) {
        return response;
    }
    match state
        .oauth
        .revoke(&form.token, &form.client_id, Utc::now())
        .await
    {
        Ok(()) => no_store(StatusCode::OK.into_response()),
        Err(_) => oauth_error("server_error"),
    }
}

#[derive(Deserialize)]
struct DeviceForm {
    client_id: String,
    scope: String,
    resource: Option<String>,
}
async fn device_authorize(
    State(state): State<AppState>,
    form: Result<Form<DeviceForm>, axum::extract::rejection::FormRejection>,
) -> Response {
    let Form(form) = match form {
        Ok(form) => form,
        Err(_) => return oauth_error("invalid_request"),
    };
    if form.client_id != state.config.cli_oauth_client_id {
        return oauth_error("invalid_client");
    }
    let resource = match token_resource(&state, &form.client_id, form.resource.as_deref()) {
        Ok(r) => r,
        Err(e) => return oauth_error(e),
    };
    let scope = match validate_scope(&form.scope, "cli") {
        Ok(s) => s,
        Err(e) => return oauth_error(e),
    };
    match state
        .oauth
        .device(
            &form.client_id,
            &resource,
            &scope,
            &state.config.notegate_public_url,
            Utc::now(),
        )
        .await
    {
        Ok(device) => no_store(Json(device).into_response()),
        Err(_) => oauth_error("server_error"),
    }
}

#[derive(Deserialize)]
struct DeviceQuery {
    user_code: Option<String>,
}
#[derive(Deserialize)]
struct DeviceApproval {
    user_code: String,
    decision: String,
}
async fn device_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<DeviceQuery>,
) -> Response {
    let code = query
        .user_code
        .filter(|s| s.len() <= 32)
        .unwrap_or_default();
    let next = format!(
        "/oauth/device?{}",
        url::form_urlencoded::Serializer::new(String::new())
            .append_pair("user_code", &code)
            .finish()
    );
    let Some(caller) = recent_user(&state, &jar).await else {
        return login_redirect(&next);
    };
    page(
        "Connect NoteGate CLI",
        &format!(
            "<p>Signed in as <strong>{}</strong>.</p><p>Only approve a code displayed by a NoteGate CLI login you started. This permits reading and changing documents in enabled Spaces and offline access.</p><form method=post action=/oauth/device><label>Code shown in your terminal <input name=user_code maxlength=32 required autocomplete=off value=\"{}\"></label><button name=decision value=approve>Connect CLI</button> <button name=decision value=deny>Deny</button></form>",
            escape_html(&caller.account.display_name),
            escape_html(&code)
        ),
    )
}
async fn device_approve(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<DeviceApproval>,
) -> Response {
    if !has_trusted_browser_origin(&headers, &state)
        || !["approve", "deny"].contains(&form.decision.as_str())
    {
        return oauth_error("invalid_request");
    }
    let Some(caller) = recent_user(&state, &jar).await else {
        return oauth_error("login_required");
    };
    match state
        .oauth
        .approve_device(
            &form.user_code,
            caller.account_id(),
            form.decision == "approve",
            Utc::now(),
        )
        .await
    {
        Ok(true) => page(
            "Request completed",
            "<p>Return to your terminal. You can close this tab.</p>",
        ),
        Ok(false) => page(
            "Code unavailable",
            "<p>The code expired, was already used, or is incorrect. Start a new CLI login.</p>",
        ),
        Err(_) => oauth_error("server_error"),
    }
}

async fn connections(State(state): State<AppState>, jar: CookieJar) -> Response {
    let Some(caller) = session_user(&state, &jar).await else {
        return login_redirect("/oauth/connections");
    };
    match state
        .oauth
        .store
        .list_grants(caller.account_id(), Utc::now())
        .await
    {
        Ok(grants) => {
            let mut body = String::from(
                "<p>Revoking a connection immediately disables its access and refresh tokens.</p><ul>",
            );
            for grant in grants {
                body.push_str(&format!("<li><strong>{}</strong> <code>{}</code><br>{}<form method=post action=/oauth/connections/revoke><input type=hidden name=id value=\"{}\"><button>Revoke connection</button></form></li>",escape_html(&grant.client_name),escape_html(&grant.client_id),escape_html(&grant.scope),grant.id));
            }
            body.push_str("</ul><p><a href=/ >Back to NoteGate</a></p>");
            page("Connected applications", &body)
        }
        Err(_) => oauth_error("server_error"),
    }
}
async fn revoke_connection(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<ConsentQuery>,
) -> Response {
    if !has_trusted_browser_origin(&headers, &state) {
        return oauth_error("invalid_request");
    }
    let Some(caller) = session_user(&state, &jar).await else {
        return oauth_error("login_required");
    };
    match state
        .oauth
        .store
        .revoke_owned_grant(form.id, caller.account_id(), Utc::now())
        .await
    {
        Ok(()) => Redirect::to("/oauth/connections").into_response(),
        Err(_) => oauth_error("server_error"),
    }
}

async fn session_user(state: &AppState, jar: &CookieJar) -> Option<Caller> {
    verify_browser_session(state, jar.get(BROWSER_SESSION_COOKIE)?.value())
        .await
        .ok()
}
async fn recent_user(state: &AppState, jar: &CookieJar) -> Option<Caller> {
    let token = jar.get(BROWSER_SESSION_COOKIE)?.value();
    let caller = verify_browser_session(state, token).await.ok()?;
    let (id, secret) = notegate_db::browser_session_repo::parse_token(token)?;
    let hash = state
        .security
        .browser_session_hash(&id.to_string(), secret)
        .ok()?;
    let session = state
        .browser_sessions
        .find_live_by_token(id, &hash)
        .await
        .ok()??;
    (session.local_session && session.created_at > Utc::now() - Duration::minutes(5))
        .then_some(caller)
}
fn validate_scope(scope: &str, required: &str) -> Result<String, &'static str> {
    let scopes: Vec<_> = scope.split_whitespace().collect();
    if !scopes.contains(&required)
        || scopes
            .iter()
            .any(|s| *s != required && *s != "offline_access")
    {
        return Err("invalid_scope");
    }
    Ok(if scopes.contains(&"offline_access") {
        format!("{required} offline_access")
    } else {
        required.to_owned()
    })
}
fn auth_cookie(id: Uuid) -> String {
    format!("notegate_authorization_{id}")
}
fn login_redirect(next: &str) -> Response {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("next", next)
        .finish();
    Redirect::to(&format!("/auth/login?{query}")).into_response()
}
fn oauth_error(error: &'static str) -> Response {
    tracing::debug!(event = "oauth.rejected", reason = error);
    let status = if error == "temporarily_unavailable" {
        StatusCode::SERVICE_UNAVAILABLE
    } else if error == "server_error" {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::BAD_REQUEST
    };
    no_store((status, Json(json!({"error":error}))).into_response())
}
fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
        .headers_mut()
        .insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));
    response
}
fn page(title: &str, body: &str) -> Response {
    let mut response=no_store(Html(format!("<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content=\"width=device-width, initial-scale=1\"><title>{}</title></head><body><main><h1>{}</h1>{body}</main></body></html>",escape_html(title),escape_html(title))).into_response());
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        header::HeaderValue::from_static(
            "default-src 'none'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
        ),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        header::HeaderValue::from_static("same-origin"),
    );
    response
}

async fn private_response(
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    let mut response = no_store(next.run(request).await);
    // Fetch serializes navigation POST Origin as null under no-referrer.
    // Keep same-origin CSRF checks while suppressing cross-origin Referer.
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        header::HeaderValue::from_static("same-origin"),
    );
    response.headers_mut().insert(
        header::X_FRAME_OPTIONS,
        header::HeaderValue::from_static("DENY"),
    );
    response
}

fn invalid_client_auth(headers: &HeaderMap, secret: bool) -> Option<Response> {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let challenge = if value.to_str().is_ok_and(|v| v.starts_with("Basic ")) {
            "Basic realm=\"oauth\""
        } else {
            "Bearer realm=\"oauth\""
        };
        return Some(no_store(
            (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, challenge)],
                Json(json!({"error":"invalid_client"})),
            )
                .into_response(),
        ));
    }
    secret.then(|| oauth_error("invalid_client"))
}
