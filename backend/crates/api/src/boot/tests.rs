use std::sync::Arc;
use std::time::Duration;

use axum::{Router, http::StatusCode, routing::get};
use tokio::sync::Notify;

use super::{HttpRuntime, shutdown_http_runtimes};

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn main_requests_can_reach_search_while_main_listener_drains()
-> Result<(), Box<dyn std::error::Error>> {
    let search_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let search_url = format!("http://{}/search", search_listener.local_addr()?);
    let mut search_runtime = HttpRuntime::new();
    search_runtime.spawn(
        "test search HTTP server",
        search_listener,
        Router::new().route("/search", get(|| async { "search result" })),
    );

    let request_started = Arc::new(Notify::new());
    let continue_request = Arc::new(Notify::new());
    let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let main_url = format!("http://{}/search", main_listener.local_addr()?);
    let main_router = Router::new().route(
        "/search",
        get({
            let request_started = request_started.clone();
            let continue_request = continue_request.clone();
            move || {
                let request_started = request_started.clone();
                let continue_request = continue_request.clone();
                let search_url = search_url.clone();
                async move {
                    request_started.notify_one();
                    continue_request.notified().await;
                    let response = reqwest::get(search_url)
                        .await
                        .map_err(|_error| StatusCode::BAD_GATEWAY)?;
                    response
                        .text()
                        .await
                        .map_err(|_error| StatusCode::BAD_GATEWAY)
                }
            }
        }),
    );
    let mut main_runtime = HttpRuntime::new();
    main_runtime.spawn("test public HTTP server", main_listener, main_router);

    let response = tokio::spawn(async move { reqwest::get(main_url).await?.text().await });
    tokio::time::timeout(TEST_TIMEOUT, request_started.notified()).await?;

    let shutdown = tokio::spawn(shutdown_http_runtimes(main_runtime, search_runtime));
    continue_request.notify_one();
    let body = tokio::time::timeout(TEST_TIMEOUT, response).await???;
    assert_eq!(body, "search result");
    tokio::time::timeout(TEST_TIMEOUT, shutdown).await???;
    Ok(())
}
