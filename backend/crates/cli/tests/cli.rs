#![allow(clippy::expect_used)]

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Redirect;
use axum::routing::{get, post};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const TEST_KEY: &str = "ngk_v2_test-secret-that-must-not-be-printed";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn me_sends_the_api_key_and_prints_only_success_json() {
    let base_url = spawn(Router::new().route(
        "/cli",
        post(
            |headers: HeaderMap, Json(envelope): Json<Value>| async move {
                if headers.get(header::AUTHORIZATION)
                    == Some(&HeaderValue::from_static(
                        "Bearer ngk_v2_test-secret-that-must-not-be-printed",
                    ))
                    && headers.get("x-notegate-cli-version")
                        == Some(&HeaderValue::from_static(env!("CARGO_PKG_VERSION")))
                    && envelope == json!({"tool":"me","input":{}})
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
            },
        ),
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
        "/cli",
        post(|Json(envelope): Json<Value>| async move {
            if envelope
                == json!({
                    "tool": "read",
                    "input": {"purpose":"list spaces","op":"spaces"},
                })
            {
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
async fn write_routes_to_the_write_endpoint_with_the_exact_json_object() {
    let expected = json!({
        "purpose": "append one audit entry",
        "op": "append",
        "target": "daily:/audit.md",
        "content": "entry",
        "ensure_newline": true,
        "expected_sha256": "abc123",
    });
    let expected_request = expected.clone();
    let base_url = spawn(Router::new().route(
        "/cli",
        post(move |headers: HeaderMap, Json(envelope): Json<Value>| {
            let expected_request = expected_request.clone();
            async move {
                if headers.get(header::AUTHORIZATION)
                    == Some(&HeaderValue::from_static(
                        "Bearer ngk_v2_test-secret-that-must-not-be-printed",
                    ))
                    && envelope == json!({"tool":"write","input":expected_request})
                {
                    (StatusCode::OK, Json(json!({"sha256":"def456"})))
                } else {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error":"wrong_write_request"})),
                    )
                }
            }
        }),
    ))
    .await;

    let output = cli(&base_url)
        .args([
            "write",
            "--input",
            r#"{"purpose":"append one audit entry","op":"append","target":"daily:/audit.md","content":"entry","ensure_newline":true,"expected_sha256":"abc123"}"#,
        ])
        .output()
        .expect("run write CLI");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("JSON stdout"),
        json!({"sha256":"def456"})
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manage_routes_to_the_manage_endpoint_with_the_exact_json_object() {
    let expected = json!({
        "purpose": "copy the notes folder",
        "op": "cp",
        "source": "daily:/notes",
        "destination": "daily:/archive/notes",
        "recursive": true,
    });
    let expected_request = expected.clone();
    let base_url = spawn(Router::new().route(
        "/cli",
        post(move |Json(envelope): Json<Value>| {
            let expected_request = expected_request.clone();
            async move {
                if envelope == json!({"tool":"manage","input":expected_request}) {
                    (StatusCode::OK, Json(json!({"copied":true})))
                } else {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error":"wrong_manage_request"})),
                    )
                }
            }
        }),
    ))
    .await;

    let output = cli(&base_url)
        .args([
            "manage",
            "--input",
            r#"{"purpose":"copy the notes folder","op":"cp","source":"daily:/notes","destination":"daily:/archive/notes","recursive":true}"#,
        ])
        .output()
        .expect("run manage CLI");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("JSON stdout"),
        json!({"copied":true})
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_remaining_mcp_tool_uses_the_same_cli_envelope() {
    let cases = [
        (
            "search",
            json!({
                "purpose": "find project notes",
                "op": "find",
                "target": "daily:/",
                "q": "project",
            }),
        ),
        (
            "file_download",
            json!({
                "purpose": "download one report",
                "target": "daily:/report.pdf",
            }),
        ),
        (
            "file_upload",
            json!({
                "purpose": "begin uploading one report",
                "op": "begin_upload",
                "target": "daily:/report.pdf",
                "byte_len": 1024,
                "media_type": "application/pdf",
            }),
        ),
        (
            "run_read_sequence",
            json!({
                "purpose": "inspect related project notes",
                "commands": [
                    {"tool": "read", "op": "spaces"},
                    {
                        "tool": "search",
                        "op": "find",
                        "target": "daily:/",
                        "q": "project"
                    }
                ],
            }),
        ),
        (
            "run_write_sequence",
            json!({
                "purpose": "create the project notes folder",
                "commands": [
                    {
                        "tool": "manage",
                        "op": "mkdir",
                        "target": "daily:/projects",
                        "parents": true
                    }
                ],
            }),
        ),
    ];

    for (tool, input) in cases {
        let serialized_input = serde_json::to_string(&input).expect("serialize input");
        let expected_envelope = json!({"tool": tool, "input": input});
        let response = expected_envelope.clone();
        let base_url = spawn(Router::new().route(
            "/cli",
            post(move |Json(envelope): Json<Value>| {
                let response = response.clone();
                async move {
                    if envelope == response {
                        (StatusCode::OK, Json(envelope))
                    } else {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"error":"wrong_cli_envelope"})),
                        )
                    }
                }
            }),
        ))
        .await;
        let output = cli(&base_url)
            .args([tool, "--input", &serialized_input])
            .output()
            .expect("run symmetric CLI tool");

        assert!(
            output.status.success(),
            "{tool}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty(), "tool: {tool}");
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).expect("JSON stdout"),
            expected_envelope,
            "tool: {tool}"
        );
    }
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
        "/cli",
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
        "/cli",
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_update_required_preserves_the_structured_recovery_contract() {
    let body = json!({
        "error": "cli_update_required",
        "kind": "client_version_incompatible",
        "message": "update notegate-cli before retrying",
        "data": {
            "client_version": env!("CARGO_PKG_VERSION"),
            "server_version": "0.1.80",
            "retryable": false,
            "next_action": {"kind":"run_command","command":"notegate-cli update"},
        },
    });
    let response_body = body.clone();
    let base_url = spawn(Router::new().route(
        "/cli",
        post(move || {
            let response_body = response_body.clone();
            async move { (StatusCode::UPGRADE_REQUIRED, Json(response_body)) }
        }),
    ))
    .await;

    let output = cli(&base_url).arg("me").output().expect("run stale CLI");

    assert_eq!(output.status.code(), Some(4));
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

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    let error = serde_json::from_slice::<Value>(&output.stderr).expect("JSON stderr");
    assert_eq!(
        error.get("error").and_then(Value::as_str),
        Some("login_required")
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains(TEST_KEY));
}

#[test]
fn auth_status_reports_api_key_precedence_without_exposing_it() {
    let output = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .args(["--base-url", "https://notegate.example", "auth", "status"])
        .env("NOTEGATE_API_KEY", TEST_KEY)
        .env("NOTEGATE_BASE_URL", "https://ignored.example")
        .env_remove("NOTEGATE_AUTHGATE_URL")
        .env_remove("NOTEGATE_CLI_CLIENT_ID")
        .output()
        .expect("run auth status");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let status = serde_json::from_slice::<Value>(&output.stdout).expect("JSON stdout");
    assert_eq!(
        status.get("credential").and_then(Value::as_str),
        Some("agent_api_key")
    );
    assert_eq!(
        status.get("source").and_then(Value::as_str),
        Some("environment")
    );
    assert_eq!(
        status.get("base_url").and_then(Value::as_str),
        Some("https://notegate.example")
    );
    assert!(!combined_output(&output).contains(TEST_KEY));
}

#[test]
fn auth_status_uses_production_base_url_by_default() {
    let output = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .args(["auth", "status"])
        .env("NOTEGATE_API_KEY", TEST_KEY)
        .env_remove("NOTEGATE_BASE_URL")
        .env_remove("NOTEGATE_AUTHGATE_URL")
        .env_remove("NOTEGATE_CLI_CLIENT_ID")
        .output()
        .expect("run auth status with the default base URL");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let status = serde_json::from_slice::<Value>(&output.stdout).expect("JSON stdout");
    assert_eq!(
        status.get("base_url").and_then(Value::as_str),
        Some("https://notegate.project-jelly.io")
    );
    assert!(!combined_output(&output).contains(TEST_KEY));
}

#[test]
fn auth_status_uses_environment_base_url_over_default() {
    let output = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .args(["auth", "status"])
        .env("NOTEGATE_API_KEY", TEST_KEY)
        .env("NOTEGATE_BASE_URL", "https://self-hosted.example")
        .env_remove("NOTEGATE_AUTHGATE_URL")
        .env_remove("NOTEGATE_CLI_CLIENT_ID")
        .output()
        .expect("run auth status with an environment base URL");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let status = serde_json::from_slice::<Value>(&output.stdout).expect("JSON stdout");
    assert_eq!(
        status.get("base_url").and_then(Value::as_str),
        Some("https://self-hosted.example")
    );
    assert!(!combined_output(&output).contains(TEST_KEY));
}

#[test]
fn command_schemas_and_help_need_no_credentials() {
    let top_level_help = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .arg("--help")
        .env_remove("NOTEGATE_BASE_URL")
        .output()
        .expect("run top-level help");
    assert!(top_level_help.status.success());
    assert!(
        String::from_utf8_lossy(&top_level_help.stdout)
            .contains("https://notegate.project-jelly.io")
    );

    let me_schema = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .args(["me", "--schema"])
        .env_remove("NOTEGATE_API_KEY")
        .env_remove("NOTEGATE_BASE_URL")
        .output()
        .expect("run me schema command");
    assert!(me_schema.status.success());
    assert!(me_schema.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&me_schema.stdout)
            .expect("me JSON schema")
            .get("additionalProperties")
            .and_then(Value::as_bool),
        Some(false)
    );

    for (command, expected_properties, expected_operations) in [
        (
            "read",
            &["purpose", "op", "target"][..],
            Some(&["spaces", "read"][..]),
        ),
        (
            "search",
            &["purpose", "op", "target", "q", "match"][..],
            Some(&["find", "grep"][..]),
        ),
        (
            "write",
            &[
                "purpose",
                "op",
                "target",
                "content",
                "edits",
                "expected_sha256",
            ][..],
            Some(&["write", "append", "patch", "edit"][..]),
        ),
        (
            "manage",
            &[
                "purpose",
                "op",
                "target",
                "source",
                "destination",
                "recursive",
            ][..],
            Some(&["mkdir", "mv", "cp", "rm"][..]),
        ),
        ("file_download", &["purpose", "target"][..], None),
        (
            "file_upload",
            &["purpose", "op", "target", "upload_id", "completed_parts"][..],
            Some(
                &[
                    "begin_upload",
                    "prepare_parts",
                    "complete_upload",
                    "abort_upload",
                ][..],
            ),
        ),
    ] {
        let schema = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
            .args([command, "--schema"])
            .env_remove("NOTEGATE_API_KEY")
            .env_remove("NOTEGATE_BASE_URL")
            .output()
            .expect("run schema command");
        assert!(
            schema.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&schema.stderr)
        );
        assert!(schema.stderr.is_empty(), "command: {command}");
        let schema = serde_json::from_slice::<Value>(&schema.stdout).expect("JSON schema");
        for property in expected_properties {
            assert!(
                schema
                    .get("properties")
                    .and_then(|properties| properties.get(property))
                    .is_some(),
                "missing {command} schema property: {property}"
            );
        }
        if let Some(expected_operations) = expected_operations {
            let operations = schema
                .pointer("/properties/op/enum")
                .and_then(Value::as_array)
                .expect("operation enum");
            for operation in expected_operations {
                assert!(
                    operations
                        .iter()
                        .any(|value| value.as_str() == Some(operation)),
                    "missing {command} operation: {operation}"
                );
            }
        }
        assert_eq!(
            schema.get("additionalProperties").and_then(Value::as_bool),
            Some(false),
            "command: {command}"
        );

        let help = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
            .args([command, "--help"])
            .env_remove("NOTEGATE_API_KEY")
            .env_remove("NOTEGATE_BASE_URL")
            .output()
            .expect("run help");
        assert!(help.status.success(), "command: {command}");
        assert!(help.stderr.is_empty(), "command: {command}");
        let help = String::from_utf8(help.stdout).expect("UTF-8 help");
        assert!(help.contains("--input <JSON>"), "command: {command}");
        assert!(help.contains("--input-file <PATH>"), "command: {command}");
        assert!(help.contains("--schema"), "command: {command}");
    }

    for command in ["run_read_sequence", "run_write_sequence"] {
        let schema = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
            .args([command, "--schema"])
            .env_remove("NOTEGATE_API_KEY")
            .env_remove("NOTEGATE_BASE_URL")
            .output()
            .expect("run sequence schema command");
        assert!(
            schema.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&schema.stderr)
        );
        let schema = serde_json::from_slice::<Value>(&schema.stdout).expect("JSON schema");
        assert!(schema.pointer("/properties/purpose").is_some());
        assert!(schema.pointer("/properties/commands").is_some());
        assert_eq!(
            schema
                .pointer("/properties/commands/minItems")
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            schema
                .pointer("/properties/commands/maxItems")
                .and_then(Value::as_u64),
            Some(20)
        );

        let help = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
            .args([command, "--help"])
            .output()
            .expect("run sequence help");
        assert!(help.status.success(), "command: {command}");
        let help = String::from_utf8(help.stdout).expect("UTF-8 help");
        assert!(help.contains("--input <JSON>"), "command: {command}");
        assert!(help.contains("--input-file <PATH>"), "command: {command}");
        assert!(help.contains("--schema"), "command: {command}");
    }

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
    for tool in [
        "me",
        "read",
        "search",
        "write",
        "manage",
        "file_download",
        "file_upload",
        "run_read_sequence",
        "run_write_sequence",
    ] {
        assert!(top_level_help.contains(tool), "missing tool: {tool}");
    }
    assert!(top_level_help.contains("update"));

    let update_help = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .args(["update", "--help"])
        .env_remove("NOTEGATE_API_KEY")
        .env_remove("NOTEGATE_BASE_URL")
        .output()
        .expect("run update help");
    assert!(update_help.status.success());
    assert!(update_help.stderr.is_empty());
    let update_help = String::from_utf8(update_help.stdout).expect("UTF-8 help");
    assert!(update_help.contains("--check"));

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
fn update_reports_unmanaged_install_without_credentials_or_base_url() {
    let output = Command::new(env!("CARGO_BIN_EXE_notegate-cli"))
        .args(["update", "--check"])
        .env_remove("NOTEGATE_API_KEY")
        .env_remove("NOTEGATE_BASE_URL")
        .output()
        .expect("run unmanaged update check");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(error_code(&output).as_deref(), Some("unmanaged_install"));
    let error = serde_json::from_slice::<Value>(&output.stderr).expect("JSON stderr");
    assert_eq!(
        error.pointer("/data/retryable").and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn argument_errors_are_structured_json() {
    for args in [
        &["read"][..],
        &["search"][..],
        &["write"][..],
        &["manage"][..],
        &["file_download"][..],
        &["file_upload"][..],
        &["run_read_sequence"][..],
        &["run_write_sequence"][..],
        &["--timeout-seconds", "nope", "me"],
        &["read", "--input", "{}", "--schema"],
        &["write", "--input", "{}", "--schema"],
        &["manage", "--input", "{}", "--input-file", "input.json"],
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
        "/cli",
        post(move || {
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
        "/cli",
        post(|| async {
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
    let oversized_base_url =
        spawn(Router::new().route("/cli", post(|| async { vec![b'x'; 8 * 1024 * 1024 + 1] })))
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
        "/cli",
        post(|Json(envelope): Json<Value>| async move {
            if envelope
                == json!({
                    "tool": "read",
                    "input": {"purpose":"list spaces from stdin","op":"spaces"},
                })
            {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_file_and_manage_stdin_inputs_use_their_shared_contracts() {
    let base_url = spawn(Router::new().route(
        "/cli",
        post(|Json(envelope): Json<Value>| async move {
            if envelope
                == json!({
                    "tool": "write",
                    "input": {
                        "purpose": "replace a note from a file",
                        "op": "write",
                        "target": "daily:/note.md",
                        "content": "replacement",
                        "create": false,
                    },
                })
            {
                (StatusCode::OK, Json(json!({"source":"file"})))
            } else if envelope
                == json!({
                    "tool": "manage",
                    "input": {
                        "purpose": "create nested folders from stdin",
                        "op": "mkdir",
                        "target": "daily:/notes/archive",
                        "parents": true,
                    },
                })
            {
                (StatusCode::OK, Json(json!({"source":"stdin"})))
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"wrong_command_input"})),
                )
            }
        }),
    ))
    .await;

    let input_path = std::env::temp_dir().join(format!(
        "notegate-cli-write-input-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &input_path,
        r#"{"purpose":"replace a note from a file","op":"write","target":"daily:/note.md","content":"replacement","create":false}"#,
    )
    .expect("write CLI input file");
    let file_output = cli(&base_url)
        .args(["write", "--input-file"])
        .arg(&input_path)
        .output()
        .expect("run write file input CLI");
    std::fs::remove_file(&input_path).expect("remove CLI input file");

    assert!(
        file_output.status.success(),
        "{}",
        String::from_utf8_lossy(&file_output.stderr)
    );
    assert!(file_output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&file_output.stdout).expect("JSON stdout"),
        json!({"source":"file"})
    );

    let mut child = cli(&base_url)
        .args(["manage", "--input-file", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn manage stdin CLI");
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(
                br#"{"purpose":"create nested folders from stdin","op":"mkdir","target":"daily:/notes/archive","parents":true}"#,
            )
            .expect("write manage CLI stdin");
    }
    let output = child.wait_with_output().expect("wait for manage CLI");

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
fn write_and_manage_reject_unknown_fields_before_http() {
    for (command, input, expected_error) in [
        (
            "write",
            r#"{"purpose":"write a note","op":"write","target":"daily:/note.md","content":"body","unexpected":true}"#,
            "invalid_write_input",
        ),
        (
            "manage",
            r#"{"purpose":"create a folder","op":"mkdir","target":"daily:/notes","unexpected":true}"#,
            "invalid_manage_input",
        ),
    ] {
        let output = cli("http://127.0.0.1:9")
            .args([command, "--input", input])
            .output()
            .expect("run invalid shared input");

        assert_eq!(output.status.code(), Some(2), "command: {command}");
        assert!(output.stdout.is_empty(), "command: {command}");
        assert_eq!(
            error_code(&output).as_deref(),
            Some(expected_error),
            "command: {command}"
        );
    }
}

#[test]
fn local_input_and_key_errors_are_structured_and_redacted() {
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
