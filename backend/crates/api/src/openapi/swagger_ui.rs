use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use super::PublicApiDoc;
use crate::state::AppState;

pub(super) fn routes() -> Router<AppState> {
    let schema = PublicApiDoc::openapi();
    Router::new()
        .route(
            "/swagger-ui/v2",
            get(|| async { Redirect::to("/swagger-ui/v2/") }),
        )
        .route("/swagger-ui/v2/", get(|| async { asset("index.html") }))
        .route(
            "/swagger-ui/v2/{*name}",
            get(|Path(name): Path<String>| async move { asset(&name) }),
        )
        .route("/openapi/v2.json", get(move || async move { Json(schema) }))
}

fn asset(name: &str) -> Response {
    let (content_type, bytes): (&'static str, &'static [u8]) = match name {
        "index.html" => (
            "text/html; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/index.html")),
        ),
        "index.css" => (
            "text/css; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/index.css")),
        ),
        "swagger-ui.css" => (
            "text/css; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/swagger-ui.css")),
        ),
        "swagger-ui-bundle.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/swagger-ui-bundle.js")),
        ),
        "swagger-ui-standalone-preset.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/swagger-ui-standalone-preset.js")),
        ),
        "swagger-initializer.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("swagger-initializer.js"),
        ),
        "favicon-16x16.png" => (
            "image/png",
            include_bytes!(concat!(env!("OUT_DIR"), "/favicon-16x16.png")),
        ),
        "favicon-32x32.png" => (
            "image/png",
            include_bytes!(concat!(env!("OUT_DIR"), "/favicon-32x32.png")),
        ),
        "LICENSE" => (
            "text/plain; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/LICENSE")),
        ),
        "NOTICE" => (
            "text/plain; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/NOTICE")),
        ),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, content_type)], bytes).into_response()
}
