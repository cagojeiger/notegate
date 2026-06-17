use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use serde::Deserialize;
use subtle::ConstantTimeEq;

use crate::auth::oauth_exchange::exchange_code_for_userinfo;
use crate::auth::oauth_flow::{
    LOGIN_NEXT_COOKIE, LOGIN_NONCE_COOKIE, LOGIN_STATE_COOKIE, LOGIN_VERIFIER_COOKIE,
    clear_flow_cookies, flow_cookie, hardened_cookie, new_login_flow,
};
use crate::auth::origin::has_trusted_browser_origin;
use crate::auth::page::html_page;
use crate::auth::session::{
    BROWSER_SESSION_COOKIE, create_browser_session, revoke_browser_session_for_logout,
};
use crate::error::ApiError;
use crate::identity::{IdentityError, ResolveAttrs};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginQuery {
    next: Option<String>,
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<LoginQuery>,
) -> Response {
    let login_flow = match new_login_flow(&state.oidc).await {
        Ok(flow) => flow,
        Err(error) => {
            tracing::error!(event = "oauth.login_flow_failed", %error);
            return html_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Login failed",
                "internal error",
            );
        }
    };
    let next = sanitize_next(query.next.as_deref());
    let mut jar = jar
        .add(flow_cookie(
            LOGIN_STATE_COOKIE,
            login_flow.csrf_state,
            state.config.secure_cookies,
        ))
        .add(flow_cookie(
            LOGIN_VERIFIER_COOKIE,
            login_flow.pkce_verifier,
            state.config.secure_cookies,
        ))
        .add(flow_cookie(
            LOGIN_NONCE_COOKIE,
            login_flow.nonce,
            state.config.secure_cookies,
        ));
    if let Some(next) = next {
        jar = jar.add(flow_cookie(
            LOGIN_NEXT_COOKIE,
            next,
            state.config.secure_cookies,
        ));
    }
    (jar, Redirect::temporary(&login_flow.redirect_url)).into_response()
}

pub async fn callback(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let cookie_state = jar
        .get(LOGIN_STATE_COOKIE)
        .map(|cookie| cookie.value().to_owned());
    let cookie_verifier = jar
        .get(LOGIN_VERIFIER_COOKIE)
        .map(|cookie| cookie.value().to_owned());
    let cookie_nonce = jar
        .get(LOGIN_NONCE_COOKIE)
        .map(|cookie| cookie.value().to_owned());
    let next = jar
        .get(LOGIN_NEXT_COOKIE)
        .and_then(|cookie| sanitize_next(Some(cookie.value())));
    let jar = clear_flow_cookies(jar, state.config.secure_cookies);

    if query.error.is_some() {
        return (
            jar,
            html_page(
                StatusCode::BAD_REQUEST,
                "Login error",
                "authorization failed",
            ),
        )
            .into_response();
    }

    let (Some(code), Some(query_state)) = (query.code.as_deref(), query.state.as_deref()) else {
        return (
            jar,
            html_page(
                StatusCode::BAD_REQUEST,
                "Login error",
                "missing code or state",
            ),
        )
            .into_response();
    };

    let Some(cookie_state) = cookie_state else {
        return (
            jar,
            html_page(StatusCode::BAD_REQUEST, "Login error", "state mismatch"),
        )
            .into_response();
    };
    if cookie_state
        .as_bytes()
        .ct_eq(query_state.as_bytes())
        .unwrap_u8()
        != 1
    {
        return (
            jar,
            html_page(StatusCode::BAD_REQUEST, "Login error", "state mismatch"),
        )
            .into_response();
    }

    let Some(verifier) = cookie_verifier.filter(|value| !value.is_empty()) else {
        return (
            jar,
            html_page(StatusCode::BAD_REQUEST, "Login error", "missing verifier"),
        )
            .into_response();
    };
    let Some(nonce) = cookie_nonce.filter(|value| !value.is_empty()) else {
        return (
            jar,
            html_page(StatusCode::BAD_REQUEST, "Login error", "missing nonce"),
        )
            .into_response();
    };

    let login_userinfo =
        match exchange_code_for_userinfo(&state.oidc, &state.http, code, &verifier, &nonce).await {
            Ok(userinfo) => userinfo,
            Err(error) => {
                tracing::warn!(event = "oauth.exchange_failed", %error);
                return (
                    jar,
                    html_page(
                        StatusCode::BAD_GATEWAY,
                        "Login error",
                        "authorization exchange failed",
                    ),
                )
                    .into_response();
            }
        };
    let attrs = ResolveAttrs {
        sub: login_userinfo.userinfo.sub,
        email: login_userinfo.userinfo.email.unwrap_or_default(),
        name: login_userinfo.userinfo.name.unwrap_or_default(),
    };
    match state.resolver.resolve_browser(attrs).await {
        Ok(caller) => {
            let session = match create_browser_session(
                &state,
                caller.account_id(),
                &login_userinfo.refresh_token,
            )
            .await
            {
                Ok(session) => session,
                Err(error) => {
                    tracing::error!(event = "oauth.session_failed", %error);
                    return (
                        jar,
                        html_page(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Login error",
                            "internal error",
                        ),
                    )
                        .into_response();
                }
            };
            (
                jar.add(browser_session_cookie(&state, session)),
                Redirect::to(next.as_deref().unwrap_or("/auth/success")),
            )
                .into_response()
        }
        Err(IdentityError::Inactive) => (
            jar,
            html_page(StatusCode::FORBIDDEN, "Login forbidden", "user is inactive"),
        )
            .into_response(),
        Err(IdentityError::InvalidInput) => (
            jar,
            html_page(
                StatusCode::BAD_REQUEST,
                "Login error",
                "identity attributes exceed notegate limits",
            ),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(event = "oauth.resolve_failed", %error);
            (
                jar,
                html_page(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Login error",
                    "internal error",
                ),
            )
                .into_response()
        }
    }
}

pub async fn success() -> Response {
    Html(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Login complete</title>
</head>
<body>
  <h1>Login complete</h1>
  <p>Login complete. You can close this tab and return to Notegate.</p>
  <script>
    if (window.opener) {
      window.opener.postMessage({ type: "notegate:login-complete" }, "*");
      window.close();
    }
  </script>
</body>
</html>"#,
    )
    .into_response()
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar, headers: HeaderMap) -> Response {
    if !has_trusted_browser_origin(&headers, &state) {
        return ApiError::forbidden("logout requires same-origin Origin or Referer")
            .into_response();
    }

    let refresh_token = revoke_browser_session_for_logout(
        &state,
        jar.get(BROWSER_SESSION_COOKIE).map(|cookie| cookie.value()),
    )
    .await;
    if let Some(refresh_token) = refresh_token {
        revoke_authgate_refresh_token(&state, &refresh_token).await;
    }
    (
        jar.add(expired_browser_session_cookie(state.config.secure_cookies)),
        Redirect::to("/"),
    )
        .into_response()
}

fn sanitize_next(value: Option<&str>) -> Option<String> {
    let value = value?;
    if value.starts_with('/') && !value.starts_with("//") && !value.contains('\\') {
        Some(value.to_owned())
    } else {
        None
    }
}
fn browser_session_cookie(state: &AppState, value: String) -> Cookie<'static> {
    hardened_cookie(
        BROWSER_SESSION_COOKIE,
        value,
        state.config.browser_session_max_ttl.as_secs() as i64,
        state.config.secure_cookies,
    )
}

fn expired_browser_session_cookie(secure: bool) -> Cookie<'static> {
    hardened_cookie(BROWSER_SESSION_COOKIE, String::new(), 0, secure)
}

async fn revoke_authgate_refresh_token(state: &AppState, refresh_token: &str) {
    let revoke_url = format!("{}/oauth/revoke", state.config.authgate_url);
    let form = [
        ("token", refresh_token),
        ("token_type_hint", "refresh_token"),
        ("client_id", state.config.oauth_client_id.as_str()),
    ];
    match state.http.post(revoke_url).form(&form).send().await {
        Ok(response) if response.status().is_success() => {}
        Ok(response) => {
            tracing::warn!(
                event = "oauth.refresh_token_revoke_failed",
                status = response.status().as_u16()
            );
        }
        Err(error) => {
            tracing::warn!(event = "oauth.refresh_token_revoke_failed", %error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_next;

    #[test]
    fn sanitize_next_allows_relative_paths_only() {
        assert_eq!(sanitize_next(Some("/")).as_deref(), Some("/"));
        assert_eq!(
            sanitize_next(Some("/dashboard?x=1")).as_deref(),
            Some("/dashboard?x=1")
        );
        assert_eq!(sanitize_next(Some("https://evil.test")), None);
        assert_eq!(sanitize_next(Some("//evil.test")), None);
        assert_eq!(sanitize_next(Some("/bad\\path")), None);
        assert_eq!(sanitize_next(None), None);
    }
}
