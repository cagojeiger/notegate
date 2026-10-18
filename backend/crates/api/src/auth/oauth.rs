use axum::extract::{Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use serde::Deserialize;

use crate::auth::oauth_flow::hardened_cookie;
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
    super::standalone_login::login(&state, jar, sanitize_next(query.next.as_deref())).await
}

pub async fn callback(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<CallbackQuery>,
) -> Response {
    super::standalone_login::callback(&state, jar, query.code, query.state, query.error).await
}

pub(super) async fn complete_login(
    state: &AppState,
    jar: CookieJar,
    login_userinfo: super::oauth_exchange::LoginUserInfo,
    next: Option<String>,
) -> Response {
    let attrs = ResolveAttrs {
        issuer: state.oidc.issuer().to_owned(),
        sub: login_userinfo.userinfo.sub,
        email: login_userinfo.userinfo.email.unwrap_or_default(),
        name: login_userinfo.userinfo.name.unwrap_or_default(),
    };
    {
        let oidc = &state.config.oidc;
        match state
            .accounts
            .find_user_id_by_identity(&attrs.issuer, &attrs.sub)
            .await
        {
            Ok(None) if !oidc.allow_signup => {
                return (
                    jar,
                    html_page(
                        StatusCode::FORBIDDEN,
                        "Account linking required",
                        "Ask the operator to link your existing account before signing in.",
                    ),
                )
                    .into_response();
            }
            Err(_) => {
                return (
                    jar,
                    html_page(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Login failed",
                        "Account lookup failed",
                    ),
                )
                    .into_response();
            }
            _ => {}
        }
    }
    match state.resolver.resolve_browser(attrs).await {
        Ok(caller) => {
            let session = match create_browser_session(state, caller.account_id()).await {
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
                jar.add(browser_session_cookie(state, session)),
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
                "identity attributes exceed NoteGate limits",
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
    super::page::browser_page(
        StatusCode::OK,
        "Login complete",
        r#"<p>You can close this tab and return to NoteGate.</p><script src="/auth/login-complete.js" defer></script>"#,
        None,
    )
}

pub async fn success_script(State(state): State<AppState>) -> Response {
    let Some(script) = login_complete_script(&state.config.notegate_public_url) else {
        tracing::error!(event = "oauth.success_script_failed");
        let mut response = StatusCode::INTERNAL_SERVER_ERROR.into_response();
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
        return response;
    };
    (
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/javascript; charset=utf-8"),
            ),
            (CACHE_CONTROL, HeaderValue::from_static("private, no-store")),
        ],
        script,
    )
        .into_response()
}

fn login_complete_script(public_url: &str) -> Option<String> {
    let public_url = url::Url::parse(public_url).ok()?;
    let target_origin = serde_json::to_string(&public_url.origin().ascii_serialization()).ok()?;
    Some(
        r#"(() => {
    if (window.opener) {
      window.opener.postMessage({ type: "notegate:login-complete" }, __NOTEGATE_TARGET_ORIGIN__);
      window.close();
    }
})();"#
            .replace("__NOTEGATE_TARGET_ORIGIN__", &target_origin),
    )
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar, headers: HeaderMap) -> Response {
    if !has_trusted_browser_origin(&headers, &state) {
        return ApiError::forbidden("logout requires same-origin Origin or Referer")
            .into_response();
    }

    if let Err(error) = revoke_browser_session_for_logout(
        &state,
        jar.get(BROWSER_SESSION_COOKIE).map(|cookie| cookie.value()),
    )
    .await
    {
        tracing::error!(event = "oauth.logout_local_revoke_failed", %error);
        return ApiError::internal("logout failed").into_response();
    }
    (
        jar.add(expired_browser_session_cookie(state.config.secure_cookies)),
        Redirect::to("/"),
    )
        .into_response()
}

fn sanitize_next(value: Option<&str>) -> Option<String> {
    let value = value?;
    if value.chars().any(char::is_control) {
        return None;
    }
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

#[cfg(test)]
mod tests {
    use axum::extract::Query;
    use axum::http::Uri;

    use super::{LoginQuery, login_complete_script, sanitize_next, success};

    #[tokio::test]
    async fn login_complete_page_uses_same_origin_external_scripts()
    -> Result<(), Box<dyn std::error::Error>> {
        let response = success().await;
        let bytes = axum::body::to_bytes(response.into_body(), 100_000).await?;
        let html = String::from_utf8(bytes.to_vec())?;
        assert!(html.contains(r#"<script src="/auth/login-complete.js" defer></script>"#));
        assert_eq!(html.matches("<script").count(), 2);
        assert!(html.contains(r#"<script src="/auth/ui.js"></script>"#));
        assert!(!html.contains("postMessage"));
        Ok(())
    }

    #[test]
    fn login_complete_script_posts_only_to_configured_origin_and_closes_popup() {
        let script = login_complete_script("https://notes.example.test/app").unwrap_or_default();
        assert!(script.contains(
            r#"postMessage({ type: "notegate:login-complete" }, "https://notes.example.test")"#
        ));
        assert!(!script.contains(r#"postMessage({ type: "notegate:login-complete" }, "*")"#));
        assert!(script.contains("window.close()"));
    }

    #[test]
    fn sanitize_next_allows_relative_paths_only() {
        for path in [
            "/",
            "/dashboard?x=1",
            "/notes/meeting%20notes.md?tab=preview#section",
            "/notes/%ED%9A%8C%EC%9D%98.md",
        ] {
            assert_eq!(sanitize_next(Some(path)).as_deref(), Some(path));
        }
        assert_eq!(sanitize_next(Some("https://evil.test")), None);
        assert_eq!(sanitize_next(Some("//evil.test")), None);
        assert_eq!(sanitize_next(Some("/bad\\path")), None);
        assert_eq!(sanitize_next(None), None);
    }

    #[test]
    fn sanitize_next_rejects_control_characters_before_url_normalization() {
        for byte in (0_u8..=0x1f).chain(std::iter::once(0x7f)) {
            let control = char::from(byte);
            for path in [
                format!("{control}/notes"),
                format!("/{control}/evil.test"),
                format!("/note{control}s"),
                format!("/notes{control}"),
            ] {
                assert_eq!(sanitize_next(Some(&path)), None, "{path:?}");
            }
        }
    }

    #[test]
    fn sanitize_next_checks_decoded_login_query() -> Result<(), Box<dyn std::error::Error>> {
        for (encoded, expected) in [
            ("%2F%09%2Fevil.test", None),
            ("%2F%0A%2Fevil.test", None),
            ("%2F%0D%2Fevil.test", None),
            ("%2Fnotes%7F", None),
            (
                "%2Fnotes%2Fmeeting%2520notes.md%3Ftab%3Dpreview%23section",
                Some("/notes/meeting%20notes.md?tab=preview#section"),
            ),
        ] {
            let uri: Uri = format!("/auth/login?next={encoded}").parse()?;
            let Query(query) = Query::<LoginQuery>::try_from_uri(&uri)?;
            let next = query.next.ok_or("missing decoded next query")?;
            assert_eq!(sanitize_next(Some(&next)).as_deref(), expected);
        }
        Ok(())
    }
}
