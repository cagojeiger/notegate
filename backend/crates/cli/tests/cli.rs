#![allow(clippy::expect_used)]

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Redirect;
use axum::routing::{get, post};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const TEST_KEY: &str = "ngk_v2_test-secret-that-must-not-be-printed";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn me_sends_the_api_key_and_prints_only_success_json() {
    let base_url = spawn(Router::new().route(
        "/api/commands/v1/me",
        get(|request: Request| async move {
            if request.headers().get(header::AUTHORIZATION)
                == Some(&HeaderValue::from_static(
                    "Bearer ngk_v2_test-secret-that-must-not-be-printed",
                ))
            {
                (
                    StatusCode::OK,
                    Json(json!({"account_kind":"agent","server_version":"0.1.77"})),
                )
            } else {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error":"unauthorized"})),
                )
            }
        }),
    ))
    .await;

    let output = cli(&base_url).arg("me").output().expect("run CLI");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("JSON stdout"),
        json!({"account_kind":"agent","server_version":"0.1.77"})
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_sends_the_shared_json_object() {
    let base_url = spawn(Router::new().route(
        "/api/commands/v1/read",
        post(|Json(input): Json<Value>| async move {
            if input == json!({"purpose":"list spaces","op":"spaces"}) {
                (StatusCode::OK, Json(json!({"spaces":[]})))
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"wrong_input"})),
                )
            }
        }),
    ))
    .await;

    let output = cli(&base_url)
        .args([
            "read",
            "--input",
            r#"{"purpose":"list spaces","op":"spaces"}"#,
        ])
        .output()
        .expect("run CLI");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("JSON stdout"),
        json!({"spaces":[]})
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_error_stays_structured_on_stderr_with_a_stable_exit_code() {
    let body = json!({
        "error": "required_field_missing",
        "kind": "invalid_input",
        "message": "target is required",
        "data": {
            "retryable": false,
            "next_action": {"kind": "add_fields", "fields": [{"field":"target"}]},
        },
    });
    let response_body = body.clone();
    let base_url = spawn(Router::new().route(
        "/api/commands/v1/read",
        post(move || {
            let response_body = response_body.clone();
            async move { (StatusCode::BAD_REQUEST, Json(response_body)) }
        }),
    ))
    .await;

    let output = cli(&base_url)
        .args(["read", "--input", r#"{"purpose":"read text","op":"read"}"#])
        .output()
        .expect("run CLI");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).expect("JSON stderr"),
        body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_timeout_is_unavailable_and_preserves_the_error_body() {
    let body = json!({
        "error": "request_timeout",
        "kind": "request_timeout",
        "message": "request processing timed out",
    });
    let response_body = body.clone();
    let base_url = spawn(Router::new().route(
        "/api/commands/v1/read",
        post(move || {
            let response_body = response_body.clone();
            async move { (StatusCode::REQUEST_TIMEOUT, Json(response_body)) }
        }),
    ))
    .await;

    let output = cli(&base_url)
        .args([
            "read",
            "--input",
            r#"{"purpose":"list spaces","op":"spaces"}"#,
        ])
        .output()
        .expect("run CLI");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).expect("JSON stderr"),
        body
    );
}

#[test]
fn missing_key_is_actionable_and_never_echoes_a_secret() {
    let output = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .args(["--base-url", "https://notegate.example", "me"])
        .env_remove("NOTEGATE_API_KEY")
        .output()
        .expect("run CLI");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = serde_json::from_slice::<Value>(&output.stderr).expect("JSON stderr");
    assert_eq!(
        error.get("error").and_then(Value::as_str),
        Some("missing_api_key")
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains(TEST_KEY));
}

#[test]
fn read_schema_and_help_need_no_credentials() {
    let schema = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .args(["read", "--schema"])
        .env_remove("NOTEGATE_API_KEY")
        .env_remove("NOTEGATE_BASE_URL")
        .output()
        .expect("run schema command");
    assert!(schema.status.success());
    assert!(schema.stderr.is_empty());
    let schema = serde_json::from_slice::<Value>(&schema.stdout).expect("JSON schema");
    assert!(
        schema
            .get("properties")
            .and_then(|value| value.get("purpose"))
            .is_some()
    );

    let help = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .args(["read", "--help"])
        .env_remove("NOTEGATE_API_KEY")
        .env_remove("NOTEGATE_BASE_URL")
        .output()
        .expect("run help");
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    let help = String::from_utf8(help.stdout).expect("UTF-8 help");
    assert!(help.contains("--input <JSON>"));
    assert!(help.contains("--input-file <PATH>"));
    assert!(help.contains("--schema"));

    let top_level_help = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .arg("--help")
        .env_remove("NOTEGATE_API_KEY")
        .env_remove("NOTEGATE_BASE_URL")
        .output()
        .expect("run top-level help");
    assert!(top_level_help.status.success());
    assert!(top_level_help.stderr.is_empty());
    let top_level_help = String::from_utf8(top_level_help.stdout).expect("UTF-8 help");
    assert!(top_level_help.contains("NOTEGATE_API_KEY"));
    assert!(top_level_help.contains("NOTEGATE_BASE_URL"));
    assert!(top_level_help.contains("never accepted as a command-line argument"));

    let version = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .arg("--version")
        .env_remove("NOTEGATE_API_KEY")
        .env_remove("NOTEGATE_BASE_URL")
        .output()
        .expect("run version");
    assert!(version.status.success());
    assert!(version.stderr.is_empty());
    assert_eq!(
        String::from_utf8(version.stdout).expect("UTF-8 version"),
        format!("notegate-cli {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn argument_errors_are_structured_json() {
    for args in [
        &["read"][..],
        &["--timeout-seconds", "nope", "me"],
        &["read", "--input", "{}", "--schema"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
            .args(args)
            .env_remove("NOTEGATE_API_KEY")
            .env_remove("NOTEGATE_BASE_URL")
            .output()
            .expect("run invalid CLI arguments");

        assert_eq!(output.status.code(), Some(2), "arguments: {args:?}");
        assert!(output.stdout.is_empty(), "arguments: {args:?}");
        let error = serde_json::from_slice::<Value>(&output.stderr).expect("JSON stderr");
        assert_eq!(
            error.get("error").and_then(Value::as_str),
            Some("invalid_arguments"),
            "arguments: {args:?}"
        );
        assert_eq!(
            error.get("kind").and_then(Value::as_str),
            Some("invalid_input"),
            "arguments: {args:?}"
        );
        assert_eq!(
            error.pointer("/data/retryable").and_then(Value::as_bool),
            Some(false),
            "arguments: {args:?}"
        );
        assert!(
            error
                .get("message")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("--help")),
            "arguments: {args:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redirect_is_not_followed_and_never_forwards_the_api_key() {
    let redirected_hits = Arc::new(AtomicUsize::new(0));
    let hits = Arc::clone(&redirected_hits);
    let redirected_base = spawn(Router::new().route(
        "/stolen",
        get(move || {
            hits.fetch_add(1, Ordering::SeqCst);
            async { Json(json!({"unexpected":true})) }
        }),
    ))
    .await;
    let location = format!("{redirected_base}/stolen");
    let base_url = spawn(Router::new().route(
        "/api/commands/v1/me",
        get(move || {
            let location = location.clone();
            async move { Redirect::temporary(&location) }
        }),
    ))
    .await;

    let output = cli(&base_url).arg("me").output().expect("run CLI");

    assert_eq!(output.status.code(), Some(5));
    assert_eq!(redirected_hits.load(Ordering::SeqCst), 0);
    assert!(!combined_output(&output).contains(TEST_KEY));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeout_is_retryable_without_secret_leakage() {
    let slow_base_url = spawn(Router::new().route(
        "/api/commands/v1/me",
        get(|| async {
            tokio::time::sleep(Duration::from_secs(2)).await;
            Json(json!({"too_late":true}))
        }),
    ))
    .await;
    let timeout = cli(&slow_base_url)
        .args(["--timeout-seconds", "1", "me"])
        .output()
        .expect("run timeout CLI");
    assert_eq!(timeout.status.code(), Some(5));
    assert_eq!(error_code(&timeout).as_deref(), Some("request_failed"));
    assert!(!combined_output(&timeout).contains(TEST_KEY));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_response_guides_request_reduction_without_secret_leakage() {
    let oversized_base_url = spawn(Router::new().route(
        "/api/commands/v1/me",
        get(|| async { vec![b'x'; 8 * 1024 * 1024 + 1] }),
    ))
    .await;
    let oversized = cli(&oversized_base_url)
        .arg("me")
        .output()
        .expect("run oversized response CLI");
    assert_eq!(oversized.status.code(), Some(5));
    assert_eq!(
        error_code(&oversized).as_deref(),
        Some("response_too_large")
    );
    let error = serde_json::from_slice::<Value>(&oversized.stderr).expect("JSON stderr");
    assert_eq!(
        error.pointer("/data/retryable").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        error.pointer("/data/recoverable").and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        error
            .pointer("/data/hint")
            .and_then(Value::as_str)
            .is_some_and(|hint| hint.contains("limit") && hint.contains("max_bytes"))
    );
    assert!(!combined_output(&oversized).contains(TEST_KEY));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_and_stdin_inputs_use_the_same_read_contract() {
    let base_url = spawn(Router::new().route(
        "/api/commands/v1/read",
        post(|Json(input): Json<Value>| async move {
            if input == json!({"purpose":"list spaces from stdin","op":"spaces"}) {
                (StatusCode::OK, Json(json!({"source":"stdin"})))
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"wrong_input"})),
                )
            }
        }),
    ))
    .await;

    let input_path = std::env::temp_dir().join(format!(
        "notegate-cli-read-input-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &input_path,
        r#"{"purpose":"list spaces from stdin","op":"spaces"}"#,
    )
    .expect("write CLI input file");
    let file_output = cli(&base_url)
        .args(["read", "--input-file"])
        .arg(&input_path)
        .output()
        .expect("run file input CLI");
    std::fs::remove_file(&input_path).expect("remove CLI input file");

    assert!(
        file_output.status.success(),
        "{}",
        String::from_utf8_lossy(&file_output.stderr)
    );
    assert!(file_output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&file_output.stdout).expect("JSON stdout"),
        json!({"source":"stdin"})
    );

    let mut child = cli(&base_url)
        .args(["read", "--input-file", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn CLI");
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(br#"{"purpose":"list spaces from stdin","op":"spaces"}"#)
            .expect("write CLI stdin");
    }
    let output = child.wait_with_output().expect("wait for CLI");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("JSON stdout"),
        json!({"source":"stdin"})
    );
}

#[test]
fn local_input_and_key_errors_are_structured_and_redacted() {
    let missing_base_url = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .arg("me")
        .env("NOTEGATE_API_KEY", TEST_KEY)
        .env_remove("NOTEGATE_BASE_URL")
        .output()
        .expect("run missing base URL CLI");
    assert_eq!(missing_base_url.status.code(), Some(2));
    assert_eq!(
        error_code(&missing_base_url).as_deref(),
        Some("missing_base_url")
    );
    assert!(!combined_output(&missing_base_url).contains(TEST_KEY));

    let invalid_json = cli("http://127.0.0.1:9")
        .args(["read", "--input", "{"])
        .output()
        .expect("run invalid JSON CLI");
    assert_eq!(invalid_json.status.code(), Some(2));
    assert_eq!(error_code(&invalid_json).as_deref(), Some("invalid_json"));
    assert!(!combined_output(&invalid_json).contains(TEST_KEY));

    let invalid_key = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .args(["--base-url", "https://notegate.example", "me"])
        .env("NOTEGATE_API_KEY", format!(" {TEST_KEY} "))
        .output()
        .expect("run invalid key CLI");
    assert_eq!(invalid_key.status.code(), Some(2));
    assert_eq!(error_code(&invalid_key).as_deref(), Some("invalid_api_key"));
    assert!(!combined_output(&invalid_key).contains(TEST_KEY));
}

#[test]
fn remote_http_base_url_is_rejected_before_sending_the_api_key() {
    let output = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .args(["--base-url", "http://notegate.example", "me"])
        .env("NOTEGATE_API_KEY", TEST_KEY)
        .output()
        .expect("run remote HTTP CLI");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(error_code(&output).as_deref(), Some("invalid_base_url"));
    assert!(
        serde_json::from_slice::<Value>(&output.stderr)
            .expect("JSON stderr")
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("HTTPS"))
    );
    assert!(!combined_output(&output).contains(TEST_KEY));
}

fn cli(base_url: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_notegate-cli"));
    command
        .args(["--base-url", base_url])
        .env("NOTEGATE_API_KEY", TEST_KEY);
    command
}

fn error_code(output: &std::process::Output) -> Option<String> {
    serde_json::from_slice::<Value>(&output.stderr)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

async fn spawn(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve test HTTP requests");
    });
    format!("http://{address}")
}
