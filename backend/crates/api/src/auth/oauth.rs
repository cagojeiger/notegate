use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use notegate_domain::{IdentityError, ResolveAttrs};
use serde::Deserialize;
use subtle::ConstantTimeEq;
use time::Duration as CookieDuration;

use crate::auth::oauth_exchange::exchange_code_for_userinfo;
use crate::auth::oauth_flow::{
    LOGIN_NONCE_COOKIE, LOGIN_STATE_COOKIE, LOGIN_VERIFIER_COOKIE, clear_flow_cookies, flow_cookie,
    new_login_flow,
};
use crate::auth::page::html_page;
use crate::auth::session::{BROWSER_SESSION_COOKIE, create_browser_session};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn login(State(state): State<AppState>, jar: CookieJar) -> Response {
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
    let jar = jar
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

    let userinfo =
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
        sub: userinfo.sub,
        email: userinfo.email.unwrap_or_default(),
        name: userinfo.name.unwrap_or_default(),
    };
    match state.resolver.resolve_browser(attrs.clone()).await {
        Ok(_caller) => {
            let session = match create_browser_session(&state, &attrs.sub) {
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
                Redirect::to("/"),
            )
                .into_response()
        }
        Err(IdentityError::Inactive) => (
            jar,
            html_page(StatusCode::FORBIDDEN, "Login forbidden", "user is inactive"),
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

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    (
        jar.add(expired_browser_session_cookie(state.config.secure_cookies)),
        Redirect::to("/"),
    )
        .into_response()
}

fn browser_session_cookie(state: &AppState, value: String) -> Cookie<'static> {
    Cookie::build((BROWSER_SESSION_COOKIE, value))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(state.config.secure_cookies)
        .max_age(CookieDuration::seconds(
            state.config.browser_session_ttl.as_secs() as i64,
        ))
        .build()
}

fn expired_browser_session_cookie(secure: bool) -> Cookie<'static> {
    Cookie::build((BROWSER_SESSION_COOKIE, ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .max_age(CookieDuration::seconds(0))
        .build()
}
