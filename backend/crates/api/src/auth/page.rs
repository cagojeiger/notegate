use axum::Router;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

use crate::state::AppState;

const STYLES: &str = concat!(
    include_str!("../../../../../frontend/web/src/design/theme.css"),
    "\n",
    include_str!("../../../../../frontend/web/src/design/auth.css")
);
const SCRIPT: &str = include_str!("page.js");
const BRAND_LIGHT: &str =
    include_str!("../../../../../frontend/web/public/brand/svg/logo-horizontal-light.svg");
const BRAND_DARK: &str =
    include_str!("../../../../../frontend/web/public/brand/svg/logo-horizontal-dark.svg");

// Public static assets do not consume the separate login/authorization rate limit.
pub fn assets() -> Router<AppState> {
    Router::new()
        .route(
            "/auth/ui.css",
            get(|| async { asset("text/css; charset=utf-8", STYLES) }),
        )
        .route(
            "/auth/ui.js",
            get(|| async { asset("text/javascript; charset=utf-8", SCRIPT) }),
        )
}

fn asset(content_type: &'static str, body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        body,
    )
        .into_response()
}

pub fn html_page(status: StatusCode, title: &str, body: &str) -> Response {
    browser_page(
        status,
        title,
        &format!("<p>{}</p>", escape_html(body)),
        None,
    )
}

pub(super) fn browser_page(
    status: StatusCode,
    title: &str,
    body: &str,
    form_redirect: Option<&url::Url>,
) -> Response {
    // Chromium also checks a form POST's redirect against form-action.
    let redirect_source = form_redirect
        .map(|url| format!(" {}", url.origin().ascii_serialization()))
        .unwrap_or_default();
    let policy = format!(
        "default-src 'none'; style-src 'self'; script-src 'self'; form-action 'self'{redirect_source}; frame-ancestors 'none'; base-uri 'none'"
    );
    let Ok(policy) = HeaderValue::from_str(&policy) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = (status, Html(document(title, body))).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_SECURITY_POLICY, policy);
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn document(title: &str, body: &str) -> String {
    let title = escape_html(title);

    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>{title} · NoteGate</title><link rel="stylesheet" href="/auth/ui.css">
<script src="/auth/ui.js"></script></head><body class="ng-auth-screen">
<div class="auth-shell"><header class="auth-header">
<a class="auth-brand" href="/" aria-label="NoteGate home"><span class="auth-brand-light" aria-hidden="true">{BRAND_LIGHT}</span><span class="auth-brand-dark" aria-hidden="true">{BRAND_DARK}</span></a>
<button id="auth-theme" class="auth-theme" type="button" hidden>Change theme</button></header>
<main class="auth-card" aria-labelledby="page-title"><h1 id="page-title">{title}</h1>{body}</main>
<footer class="auth-footer"><a href="/">Back to NoteGate</a>
<nav class="auth-links" aria-label="Legal and support"><a href="/terms">Terms</a><a href="/privacy">Privacy</a><a href="/support">Support</a></nav></footer></div></body></html>"#
    )
}

pub(super) fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn browser_errors_escape_content_and_keep_private_security_headers()
    -> Result<(), Box<dyn std::error::Error>> {
        let response = html_page(
            StatusCode::BAD_REQUEST,
            "<title>",
            "<script>alert(1)</script>",
        );
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );
        assert_eq!(
            response
                .headers()
                .get(header::X_FRAME_OPTIONS)
                .and_then(|v| v.to_str().ok()),
            Some("DENY")
        );
        let policy = response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .ok_or("missing CSP")?
            .to_str()?;
        assert!(policy.contains("default-src 'none'"));
        assert!(policy.contains("style-src 'self'; script-src 'self'"));
        assert!(!policy.contains("unsafe-inline"));
        let body = axum::body::to_bytes(response.into_body(), 100_000).await?;
        let html = String::from_utf8(body.to_vec())?;
        assert!(html.contains("&lt;title&gt;"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("/auth/ui.css"));
        assert!(html.contains("/auth/ui.js"));
        assert!(html.contains(r#"<body class="ng-auth-screen">"#));
        assert!(html.contains(r#"<a href="/">Back to NoteGate</a>"#));
        assert!(html.contains(r#"aria-label="Legal and support""#));
        for (path, label) in [
            ("/terms", "Terms"),
            ("/privacy", "Privacy"),
            ("/support", "Support"),
        ] {
            assert!(html.contains(&format!(r#"<a href="{path}">{label}</a>"#)));
        }
        assert!(!html.contains("<script>alert"));
        Ok(())
    }
}
