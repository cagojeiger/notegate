#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use std::collections::BTreeMap;

use axum::body::{Body, to_bytes};
use axum::http::{
    Request, StatusCode,
    header::{ACCESS_CONTROL_EXPOSE_HEADERS, CACHE_CONTROL, LOCATION, ORIGIN},
};
use notegate_core::S3Config;
use notegate_core::limits::{BROWSER_FILE_MAX_BYTES, SINGLE_PUT_MAX_BYTES};
use notegate_db::{FilesRepo, ObjectStorageRepo, test_support::TestDb};
use notegate_model::files::{BeginObjectUpload, ObjectUploadMode, ObjectUploadRegistration};
use notegate_model::{Caller, FileEncryptionMode};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::mcp::tools::transfers;
use crate::mcp::tools::unified::{CompletedPartInput, FileTransferInput};

use crate::rest::test_support::{
    caller_and_space, empty_request, json_request, rest_app, state_with_s3,
};

fn test_s3_config() -> Option<S3Config> {
    let endpoint = std::env::var("NOTEGATE_TEST_S3_ENDPOINT").ok()?;
    Some(S3Config {
        public_endpoint: Some(
            std::env::var("NOTEGATE_TEST_S3_PUBLIC_ENDPOINT").unwrap_or_else(|_| endpoint.clone()),
        ),
        endpoint,
        region: std::env::var("NOTEGATE_TEST_S3_REGION").unwrap_or_else(|_| "us-east-1".to_owned()),
        bucket: std::env::var("NOTEGATE_TEST_S3_BUCKET")
            .unwrap_or_else(|_| "notegate-test".to_owned()),
        access_key: std::env::var("NOTEGATE_TEST_S3_ACCESS_KEY")
            .unwrap_or_else(|_| "notegate".to_owned()),
        secret_key: SecretString::from(
            std::env::var("NOTEGATE_TEST_S3_SECRET_KEY")
                .unwrap_or_else(|_| "notegate-secret".to_owned()),
        ),
        force_path_style: true,
    })
}

fn unavailable_internal_storage(mut config: S3Config) -> S3Config {
    config.endpoint = "http://127.0.0.1:1".to_owned();
    config
}

struct BegunUpload {
    id: Uuid,
    url: String,
    headers: BTreeMap<String, String>,
}

async fn begin_upload(
    state: &crate::state::AppState,
    caller: &Caller,
    space_id: Uuid,
    parent_node_id: Uuid,
    name: &str,
    byte_len: usize,
) -> Result<BegunUpload, Box<dyn std::error::Error>> {
    begin_upload_with_media_type(
        state,
        caller,
        space_id,
        parent_node_id,
        name,
        byte_len,
        "application/octet-stream",
    )
    .await
}

async fn begin_upload_with_media_type(
    state: &crate::state::AppState,
    caller: &Caller,
    space_id: Uuid,
    parent_node_id: Uuid,
    name: &str,
    byte_len: usize,
    media_type: &str,
) -> Result<BegunUpload, Box<dyn std::error::Error>> {
    let (status, body) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads"),
        json!({
            "parent_node_id": parent_node_id,
            "name": name,
            "byte_len": byte_len,
            "media_type": media_type
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["transfer"]["mode"], "single");
    let headers = body["transfer"]["headers"]
        .as_object()
        .expect("transfer headers")
        .iter()
        .map(|(name, value)| {
            Ok((
                name.clone(),
                value.as_str().ok_or("invalid transfer header")?.to_owned(),
            ))
        })
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    Ok(BegunUpload {
        id: serde_json::from_value(body["upload_id"].clone())?,
        url: body["transfer"]["url"]
            .as_str()
            .expect("put url")
            .to_owned(),
        headers,
    })
}

#[tokio::test]
async fn verified_raster_images_receive_inline_preview_urls()
-> Result<(), Box<dyn std::error::Error>> {
    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00,
    ];
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let upload = begin_upload_with_media_type(
        &state,
        &caller,
        space_id,
        root_id,
        "image.bin",
        PNG.len(),
        "text/plain",
    )
    .await?;
    put_upload(&upload, PNG).await?.error_for_status()?;

    let (status, completed) = complete_upload(&state, &caller, space_id, upload.id).await?;
    assert_eq!(status, StatusCode::CREATED, "{completed}");
    assert_eq!(completed["node"]["media_type"], "text/plain");
    assert_eq!(completed["node"]["detected_media_type"], "image/png");
    assert_eq!(completed["node"]["preview_available"], true);
    let node_id: Uuid = serde_json::from_value(completed["node"]["id"].clone())?;

    let preview_response = rest_app(state.clone(), caller.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/v1/spaces/{space_id}/files/{node_id}/preview-url"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(preview_response.status(), StatusCode::OK);
    assert_eq!(
        preview_response.headers().get(CACHE_CONTROL),
        Some(&axum::http::HeaderValue::from_static("private, no-store"))
    );
    let preview: Value =
        serde_json::from_slice(&to_bytes(preview_response.into_body(), usize::MAX).await?)?;
    assert_eq!(preview["media_type"], "image/png");
    let response = reqwest::get(preview["url"].as_str().ok_or("preview url")?).await?;
    assert!(response.status().is_success());
    assert_eq!(
        response.headers().get(reqwest::header::CONTENT_TYPE),
        Some(&reqwest::header::HeaderValue::from_static("image/png"))
    );
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok()),
        Some("inline")
    );
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store, max-age=0")
    );

    sqlx::query("UPDATE file_objects SET detected_media_type = NULL WHERE node_id = $1")
        .bind(node_id)
        .execute(&db.pool)
        .await?;
    let (status, _) = empty_request(
        rest_app(state.clone(), caller.clone()),
        "GET",
        format!("/v1/spaces/{space_id}/files/{node_id}/preview-url"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let detected: Option<String> =
        sqlx::query_scalar("SELECT detected_media_type FROM file_objects WHERE node_id = $1")
            .bind(node_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(detected.as_deref(), Some("image/png"));

    delete_attached_file(&db, &state, &caller, space_id, node_id).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn non_image_bytes_do_not_receive_preview_urls() -> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let bytes = b"%PDF-1.7\n";
    let upload = begin_upload_with_media_type(
        &state,
        &caller,
        space_id,
        root_id,
        "document.png",
        bytes.len(),
        "image/png",
    )
    .await?;
    put_upload(&upload, bytes).await?.error_for_status()?;
    let (status, completed) = complete_upload(&state, &caller, space_id, upload.id).await?;
    assert_eq!(status, StatusCode::CREATED, "{completed}");
    assert_eq!(completed["node"]["detected_media_type"], "application/pdf");
    assert_eq!(completed["node"]["preview_available"], false);
    let node_id: Uuid = serde_json::from_value(completed["node"]["id"].clone())?;

    let (status, body) = empty_request(
        rest_app(state.clone(), caller.clone()),
        "GET",
        format!("/v1/spaces/{space_id}/files/{node_id}/preview-url"),
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    delete_attached_file(&db, &state, &caller, space_id, node_id).await?;
    db.cleanup().await;
    Ok(())
}

async fn put_upload(
    upload: &BegunUpload,
    bytes: &[u8],
) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
    let mut request = reqwest::Client::new().put(&upload.url);
    for (name, value) in &upload.headers {
        request = request.header(name, value);
    }
    Ok(request.body(bytes.to_vec()).send().await?)
}

async fn put_mcp_part(transfer: &Value) -> Result<CompletedPartInput, Box<dyn std::error::Error>> {
    let part_number = transfer["part_number"].as_i64().ok_or("part number")? as i32;
    let content_length = transfer["content_length"].as_u64().ok_or("part length")?;
    let mut request = reqwest::Client::new().put(transfer["url"].as_str().ok_or("part url")?);
    for (name, value) in transfer["headers"].as_object().ok_or("part headers")? {
        request = request.header(name, value.as_str().ok_or("part header value")?);
    }
    let response = request
        .header(ORIGIN, "http://localhost:5173")
        .body(vec![part_number as u8; content_length as usize])
        .send()
        .await?
        .error_for_status()?;
    let exposed_headers = response
        .headers()
        .get(ACCESS_CONTROL_EXPOSE_HEADERS)
        .and_then(|value| value.to_str().ok())
        .ok_or("storage CORS does not expose response headers")?;
    if !exposed_headers
        .split(',')
        .any(|name| name.trim().eq_ignore_ascii_case("etag"))
    {
        return Err("storage CORS does not expose ETag".into());
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .ok_or("part etag")?
        .to_str()?
        .to_owned();
    Ok(CompletedPartInput { part_number, etag })
}

async fn complete_upload(
    state: &crate::state::AppState,
    caller: &Caller,
    space_id: Uuid,
    upload_id: Uuid,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    empty_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads/{upload_id}/complete"),
    )
    .await
}

async fn mark_upload_stale(db: &TestDb, upload_id: Uuid) -> Result<(), Box<dyn std::error::Error>> {
    set_upload_inactivity(db, upload_id, "3 hours").await
}

async fn set_upload_inactivity(
    db: &TestDb,
    upload_id: Uuid,
    duration: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::query(
        "UPDATE object_storage_objects \
         SET last_activity_at = now() - $2::interval WHERE id = $1",
    )
    .bind(upload_id)
    .bind(duration)
    .execute(&db.pool)
    .await?;
    Ok(())
}

async fn upload_is_stale(db: &TestDb, upload_id: Uuid) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(sqlx::query_scalar(
        "SELECT last_activity_at <= now() - interval '1 hour' \
         FROM object_storage_objects WHERE id = $1",
    )
    .bind(upload_id)
    .fetch_one(&db.pool)
    .await?)
}

async fn run_cleanup(db: &TestDb, state: &crate::state::AppState) {
    crate::object_storage_cleanup_worker::run_once(
        &ObjectStorageRepo::new(db.pool.clone()),
        &state.object_storage,
        &CancellationToken::new(),
    )
    .await;
}

async fn object_state(db: &TestDb, upload_id: Uuid) -> Result<String, Box<dyn std::error::Error>> {
    Ok(
        sqlx::query_scalar("SELECT state FROM object_storage_objects WHERE id = $1")
            .bind(upload_id)
            .fetch_one(&db.pool)
            .await?,
    )
}

async fn object_get_status(state: &crate::state::AppState, upload_id: Uuid) -> StatusCode {
    let url = state
        .object_storage
        .presign_get(&format!("objects/{upload_id}"), None)
        .await
        .expect("presign get");
    reqwest::get(url).await.expect("get object").status()
}

async fn delete_attached_file(
    db: &TestDb,
    state: &crate::state::AppState,
    caller: &Caller,
    space_id: Uuid,
    node_id: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    FilesRepo::new(db.pool.clone())
        .soft_delete_node(space_id, node_id, caller.account.id, false)
        .await?;
    run_cleanup(db, state).await;
    Ok(())
}

#[tokio::test]
async fn object_upload_round_trips_through_s3_presigned_urls()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let payload = b"notegate-s3-round-trip";
    let upload = begin_upload(
        &state,
        &caller,
        space_id,
        root_id,
        "archive.bin",
        payload.len(),
    )
    .await?;
    assert_eq!(upload.headers.get("if-none-match"), Some(&"*".to_owned()));
    assert!(!upload.headers.contains_key("content-length"));
    let signed_headers = url::Url::parse(&upload.url)?
        .query_pairs()
        .find(|(name, _)| name.eq_ignore_ascii_case("x-amz-signedheaders"))
        .map(|(_, value)| value.into_owned())
        .ok_or("missing signed headers")?;
    assert!(
        signed_headers
            .split(';')
            .any(|name| name.eq_ignore_ascii_case("content-length"))
    );

    let put = put_upload(&upload, payload).await?;
    assert!(put.status().is_success(), "PUT failed: {}", put.status());

    let (status, completed) = complete_upload(&state, &caller, space_id, upload.id).await?;
    assert_eq!(status, StatusCode::CREATED, "{completed}");
    let node_id: Uuid = serde_json::from_value(completed["node"]["id"].clone())?;

    let (status, completed_again) = complete_upload(&state, &caller, space_id, upload.id).await?;
    assert_eq!(status, StatusCode::CREATED, "{completed_again}");
    assert_eq!(completed_again["node"]["id"], completed["node"]["id"]);

    let object_key: String =
        sqlx::query_scalar("SELECT object_key FROM file_objects WHERE node_id = $1")
            .bind(node_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(object_key, format!("objects/{}", upload.id));

    let response = rest_app(state.clone(), caller.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/v1/spaces/{space_id}/files/{node_id}/content"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::FOUND);
    let get_url = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("download location");
    let downloaded = reqwest::get(get_url).await?;
    assert!(downloaded.status().is_success());
    assert_eq!(downloaded.bytes().await?.as_ref(), payload);

    delete_attached_file(&db, &state, &caller, space_id, node_id).await?;

    assert_eq!(object_state(&db, upload.id).await?, "deleted");
    assert_eq!(reqwest::get(get_url).await?.status(), StatusCode::NOT_FOUND);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn rest_multipart_upload_round_trips_and_completes_idempotently()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let byte_len = SINGLE_PUT_MAX_BYTES as i64 + 1;

    let (status, begun) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads"),
        json!({
            "parent_node_id": root_id,
            "name": "browser-large.bin",
            "byte_len": byte_len,
            "media_type": "application/octet-stream"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{begun}");
    assert_eq!(begun["transfer"]["mode"], "multipart");
    assert_eq!(begun["transfer"]["part_count"], 2);
    let upload_id: Uuid = serde_json::from_value(begun["upload_id"].clone())?;

    let (status, prepared) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads/{upload_id}/parts"),
        json!({ "part_numbers": [1, 2] }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{prepared}");
    let transfers = prepared["parts"].as_array().ok_or("part transfers")?;
    let (first, second) = tokio::join!(put_mcp_part(&transfers[0]), put_mcp_part(&transfers[1]));
    let completed_parts = [first?, second?];
    let completion_body = json!({
        "completed_parts": completed_parts
            .iter()
            .map(|part| json!({ "part_number": part.part_number, "etag": part.etag }))
            .collect::<Vec<_>>()
    });

    let first = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads/{upload_id}/complete"),
        completion_body.clone(),
    );
    let second = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads/{upload_id}/complete"),
        completion_body,
    );
    let (first, second) = tokio::join!(first, second);
    let (first_status, first_body) = first?;
    let (second_status, second_body) = second?;
    assert_eq!(first_status, StatusCode::CREATED, "{first_body}");
    assert_eq!(second_status, StatusCode::CREATED, "{second_body}");
    assert_eq!(first_body["node"]["id"], second_body["node"]["id"]);
    let node_id: Uuid = serde_json::from_value(first_body["node"]["id"].clone())?;

    let download = state
        .object_storage
        .presign_get(&format!("objects/{upload_id}"), None)
        .await
        .map_err(|error| format!("presign download: {error:?}"))?;
    assert_eq!(
        reqwest::get(download).await?.bytes().await?.len(),
        byte_len as usize
    );

    delete_attached_file(&db, &state, &caller, space_id, node_id).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn begin_rejects_too_many_pending_uploads() -> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;

    // Fill the per-account concurrent-upload allowance with tiny objects so the
    // count cap, rather than the byte quota, is the rejecting invariant.
    for index in 0..notegate_core::limits::OBJECT_UPLOAD_MAX_PENDING {
        begin_upload(
            &state,
            &caller,
            space_id,
            root_id,
            &format!("pending-{index}.bin"),
            4,
        )
        .await?;
    }

    // The next begin is rejected before any object is staged.
    let (status, body) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads"),
        json!({
            "parent_node_id": root_id,
            "name": "over-cap.bin",
            "byte_len": 4,
            "media_type": "application/octet-stream"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn rest_begin_uses_multipart_above_the_single_put_limit()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;

    let (status, body) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads"),
        json!({
            "parent_node_id": root_id,
            "name": "multipart.bin",
            "byte_len": SINGLE_PUT_MAX_BYTES as i64 + 1,
            "media_type": "application/octet-stream"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["transfer"]["mode"], "multipart");
    assert_eq!(body["transfer"]["part_count"], 2);
    let upload_id: Uuid = serde_json::from_value(body["upload_id"].clone())?;
    mark_upload_stale(&db, upload_id).await?;

    for part_numbers in [json!([1, 1]), json!([3])] {
        let (status, error) = json_request(
            rest_app(state.clone(), caller.clone()),
            "POST",
            format!("/v1/spaces/{space_id}/file-uploads/{upload_id}/parts"),
            json!({ "part_numbers": part_numbers }),
        )
        .await?;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(error["kind"], "invalid_input");
    }
    assert!(upload_is_stale(&db, upload_id).await?);

    let (status, error) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads/{upload_id}/complete"),
        json!({
            "completed_parts": [{ "part_number": 1, "etag": "etag-1" }]
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(error["kind"], "invalid_input");
    assert!(upload_is_stale(&db, upload_id).await?);

    let (status, _) = empty_request(
        rest_app(state.clone(), caller),
        "DELETE",
        format!("/v1/spaces/{space_id}/file-uploads/{upload_id}"),
    )
    .await?;
    assert_eq!(status, StatusCode::NO_CONTENT);
    run_cleanup(&db, &state).await;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn rest_begin_rejects_files_above_the_browser_limit() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;

    let (status, body) = json_request(
        rest_app(state, caller),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads"),
        json!({
            "parent_node_id": root_id,
            "name": "too-large-for-browser.bin",
            "byte_len": BROWSER_FILE_MAX_BYTES as i64 + 1,
            "media_type": "application/octet-stream"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["kind"], "invalid_input");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn pending_declared_bytes_count_toward_the_space_quota()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;

    begin_upload(
        &state,
        &caller,
        space_id,
        root_id,
        "pending-100-mib.bin",
        SINGLE_PUT_MAX_BYTES,
    )
    .await?;

    let (status, body) = json_request(
        rest_app(state, caller),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads"),
        json!({
            "parent_node_id": root_id,
            "name": "over-tier-quota.bin",
            "byte_len": 28 * 1024 * 1024 + 1,
            "media_type": "application/octet-stream"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn multipart_abort_cleanup_closes_the_provider_upload()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let upload_id = Uuid::new_v4();
    let object_key = format!("objects/{upload_id}");
    let byte_len = 6 * 1024 * 1024;
    let command = BeginObjectUpload {
        parent_node_id: root_id,
        name: "aborted-multipart.bin".to_owned(),
        byte_len,
        media_type: "application/octet-stream".to_owned(),
        original_filename: None,
        encryption_mode: FileEncryptionMode::None,
        encryption_metadata: None,
    };
    state
        .files
        .prepare_object_upload(caller.account_id(), space_id, &command)
        .await?;
    let provider_upload_id = state
        .object_storage
        .create_multipart_upload(&object_key, &command.media_type)
        .await
        .map_err(|error| format!("create multipart upload: {error:?}"))?;
    let registration = ObjectUploadRegistration {
        id: upload_id,
        object_key: object_key.clone(),
        upload_mode: ObjectUploadMode::Multipart,
        multipart_upload_id: Some(provider_upload_id.clone()),
        multipart_part_size: Some(byte_len),
    };
    state
        .files
        .record_registered_object_upload(&registration, caller.account_id(), space_id, &command)
        .await?;
    assert_eq!(
        state
            .object_storage
            .complete_multipart_upload(
                &object_key,
                &provider_upload_id,
                &[crate::object_storage::CompletedUploadPart {
                    part_number: 1,
                    etag: "missing-part".to_owned(),
                }],
            )
            .await,
        Err(crate::object_storage::ObjectStorageError::InvalidMultipart)
    );
    let transfer = state
        .object_storage
        .presign_upload_part(
            &object_key,
            &provider_upload_id,
            1,
            byte_len,
            crate::object_storage::MCP_TRANSFER_URL_TTL,
        )
        .await
        .map_err(|error| format!("presign upload part: {error:?}"))?;

    state
        .files
        .cancel_object_upload(caller.account_id(), space_id, upload_id)
        .await?;
    run_cleanup(&db, &state).await;
    assert_eq!(object_state(&db, upload_id).await?, "expired");

    let mut request = reqwest::Client::new().put(transfer.url);
    for (name, value) in transfer.headers {
        request = request.header(name, value);
    }
    let response = request.body(vec![1_u8; byte_len as usize]).send().await?;
    assert!(
        !response.status().is_success(),
        "upload part succeeded after multipart abort"
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn begin_counts_expiry_pending_uploads_toward_the_cap()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3.clone());
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let expiring = begin_upload(&state, &caller, space_id, root_id, "expiring.bin", 4).await?;
    mark_upload_stale(&db, expiring.id).await?;

    let unavailable_state = state_with_s3(&db, unavailable_internal_storage(s3));
    run_cleanup(&db, &unavailable_state).await;
    assert_eq!(object_state(&db, expiring.id).await?, "expire_pending");

    for index in 1..notegate_core::limits::OBJECT_UPLOAD_MAX_PENDING {
        begin_upload(
            &state,
            &caller,
            space_id,
            root_id,
            &format!("pending-with-expiry-{index}.bin"),
            4,
        )
        .await?;
    }

    let (status, body) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads"),
        json!({
            "parent_node_id": root_id,
            "name": "over-cap-with-expiry.bin",
            "byte_len": 4,
            "media_type": "application/octet-stream"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn presigned_put_rejects_a_size_mismatch() -> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let upload = begin_upload(&state, &caller, space_id, root_id, "wrong-size.bin", 4).await?;
    let put = put_upload(&upload, b"bad").await?;
    assert!(
        !put.status().is_success(),
        "mismatched PUT unexpectedly succeeded"
    );

    mark_upload_stale(&db, upload.id).await?;
    run_cleanup(&db, &state).await;
    assert_eq!(object_state(&db, upload.id).await?, "expired");
    assert_eq!(
        object_get_status(&state, upload.id).await,
        StatusCode::NOT_FOUND
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn object_upload_rejects_completion_before_put() -> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let upload = begin_upload(&state, &caller, space_id, root_id, "missing.bin", 4).await?;
    mark_upload_stale(&db, upload.id).await?;

    let (status, body) = complete_upload(&state, &caller, space_id, upload.id).await?;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["kind"], "conflict");

    run_cleanup(&db, &state).await;
    assert_eq!(object_state(&db, upload.id).await?, "expired");
    assert_eq!(
        object_get_status(&state, upload.id).await,
        StatusCode::NOT_FOUND
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn abandoned_uploaded_object_is_expired_and_deleted() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let upload = begin_upload(&state, &caller, space_id, root_id, "abandoned.bin", 9).await?;
    put_upload(&upload, b"abandoned")
        .await?
        .error_for_status()?;
    assert_eq!(object_get_status(&state, upload.id).await, StatusCode::OK);

    mark_upload_stale(&db, upload.id).await?;
    run_cleanup(&db, &state).await;

    assert_eq!(object_state(&db, upload.id).await?, "expired");
    assert_eq!(
        object_get_status(&state, upload.id).await,
        StatusCode::NOT_FOUND
    );
    let file_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM file_objects WHERE object_key = $1")
            .bind(format!("objects/{}", upload.id))
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(file_count, 0);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn one_hour_inactive_upload_is_not_expired() -> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let upload = begin_upload(&state, &caller, space_id, root_id, "inactive.bin", 4).await?;

    set_upload_inactivity(&db, upload.id, "1 hour").await?;
    run_cleanup(&db, &state).await;

    assert_eq!(object_state(&db, upload.id).await?, "uploading");
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn active_completion_prevents_stale_upload_cleanup() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let upload = begin_upload(&state, &caller, space_id, root_id, "active.bin", 6).await?;
    put_upload(&upload, b"active").await?.error_for_status()?;
    mark_upload_stale(&db, upload.id).await?;

    let (status, completed) = complete_upload(&state, &caller, space_id, upload.id).await?;
    assert_eq!(status, StatusCode::CREATED, "{completed}");
    let node_id: Uuid = serde_json::from_value(completed["node"]["id"].clone())?;
    run_cleanup(&db, &state).await;
    assert_eq!(object_state(&db, upload.id).await?, "attached");
    assert_eq!(object_get_status(&state, upload.id).await, StatusCode::OK);

    delete_attached_file(&db, &state, &caller, space_id, node_id).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn presigned_put_cannot_overwrite_an_existing_object()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let upload = begin_upload(&state, &caller, space_id, root_id, "immutable.bin", 5).await?;
    put_upload(&upload, b"first").await?.error_for_status()?;

    let replay = put_upload(&upload, b"later").await?;
    assert_eq!(replay.status(), StatusCode::PRECONDITION_FAILED);

    let (status, completed) = complete_upload(&state, &caller, space_id, upload.id).await?;
    assert_eq!(status, StatusCode::CREATED, "{completed}");
    let node_id: Uuid = serde_json::from_value(completed["node"]["id"].clone())?;
    let url = state
        .object_storage
        .presign_get(&format!("objects/{}", upload.id), None)
        .await
        .expect("presign get");
    assert_eq!(reqwest::get(url).await?.bytes().await?.as_ref(), b"first");

    delete_attached_file(&db, &state, &caller, space_id, node_id).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_completion_attaches_one_file_idempotently()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let upload = begin_upload(&state, &caller, space_id, root_id, "concurrent.bin", 10).await?;
    put_upload(&upload, b"concurrent")
        .await?
        .error_for_status()?;

    let first = complete_upload(&state, &caller, space_id, upload.id);
    let second = complete_upload(&state, &caller, space_id, upload.id);
    let (first, second) = tokio::join!(first, second);
    let (first_status, first_body) = first?;
    let (second_status, second_body) = second?;
    assert_eq!(first_status, StatusCode::CREATED, "{first_body}");
    assert_eq!(second_status, StatusCode::CREATED, "{second_body}");
    assert_eq!(first_body["node"]["id"], second_body["node"]["id"]);
    let node_id: Uuid = serde_json::from_value(first_body["node"]["id"].clone())?;
    let file_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM file_objects WHERE object_key = $1")
            .bind(format!("objects/{}", upload.id))
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(file_count, 1);

    delete_attached_file(&db, &state, &caller, space_id, node_id).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn completion_recovers_after_temporary_storage_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let unavailable_state = state_with_s3(&db, unavailable_internal_storage(s3.clone()));
    let (caller, space_id, root_id) = caller_and_space(&unavailable_state).await?;
    let upload = begin_upload(
        &unavailable_state,
        &caller,
        space_id,
        root_id,
        "retry-complete.bin",
        5,
    )
    .await?;
    put_upload(&upload, b"retry").await?.error_for_status()?;

    let (status, body) = complete_upload(&unavailable_state, &caller, space_id, upload.id).await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["kind"], "object_storage_unavailable");
    assert_eq!(object_state(&db, upload.id).await?, "uploading");

    let available_state = state_with_s3(&db, s3);
    let (status, completed) =
        complete_upload(&available_state, &caller, space_id, upload.id).await?;
    assert_eq!(status, StatusCode::CREATED, "{completed}");
    let node_id: Uuid = serde_json::from_value(completed["node"]["id"].clone())?;
    delete_attached_file(&db, &available_state, &caller, space_id, node_id).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn cleanup_retries_after_temporary_storage_failure() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let available_state = state_with_s3(&db, s3.clone());
    let (caller, space_id, root_id) = caller_and_space(&available_state).await?;
    let upload = begin_upload(
        &available_state,
        &caller,
        space_id,
        root_id,
        "retry-cleanup.bin",
        7,
    )
    .await?;
    put_upload(&upload, b"cleanup").await?.error_for_status()?;
    mark_upload_stale(&db, upload.id).await?;

    let unavailable_state = state_with_s3(&db, unavailable_internal_storage(s3));
    run_cleanup(&db, &unavailable_state).await;
    let failed: (String, i32, Option<String>, bool) = sqlx::query_as(
        "SELECT state, retry_count, last_error_code, retry_after > now() \
         FROM object_storage_objects WHERE id = $1",
    )
    .bind(upload.id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        failed,
        (
            "expire_pending".to_owned(),
            1,
            Some("unavailable".to_owned()),
            true,
        )
    );
    assert_eq!(
        object_get_status(&available_state, upload.id).await,
        StatusCode::OK
    );

    sqlx::query(
        "UPDATE object_storage_objects SET retry_after = now() - interval '1 second' WHERE id = $1",
    )
    .bind(upload.id)
    .execute(&db.pool)
    .await?;
    run_cleanup(&db, &available_state).await;
    assert_eq!(object_state(&db, upload.id).await?, "expired");
    assert_eq!(
        object_get_status(&available_state, upload.id).await,
        StatusCode::NOT_FOUND
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn cleanup_recovers_when_object_was_deleted_before_state_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let upload = begin_upload(&state, &caller, space_id, root_id, "cleanup-crash.bin", 5).await?;
    put_upload(&upload, b"crash").await?.error_for_status()?;
    mark_upload_stale(&db, upload.id).await?;

    let repo = ObjectStorageRepo::new(db.pool.clone());
    assert!(repo.claim_cleanup(1_800, 30).await?.is_some());
    assert!(repo.begin_expiry(upload.id).await?);
    state
        .object_storage
        .delete(&format!("objects/{}", upload.id))
        .await
        .expect("delete object");
    sqlx::query(
        "UPDATE object_storage_objects SET retry_after = now() - interval '1 second' WHERE id = $1",
    )
    .bind(upload.id)
    .execute(&db.pool)
    .await?;

    run_cleanup(&db, &state).await;
    assert_eq!(object_state(&db, upload.id).await?, "expired");
    assert_eq!(
        object_get_status(&state, upload.id).await,
        StatusCode::NOT_FOUND
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn attachment_conflict_expires_only_the_unattached_object()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let first = begin_upload(&state, &caller, space_id, root_id, "same.bin", 5).await?;
    let second = begin_upload(&state, &caller, space_id, root_id, "same.bin", 6).await?;
    put_upload(&first, b"first").await?.error_for_status()?;
    put_upload(&second, b"second").await?.error_for_status()?;

    let (status, completed) = complete_upload(&state, &caller, space_id, first.id).await?;
    assert_eq!(status, StatusCode::CREATED, "{completed}");
    let first_node_id: Uuid = serde_json::from_value(completed["node"]["id"].clone())?;
    let (status, conflict) = complete_upload(&state, &caller, space_id, second.id).await?;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(object_state(&db, first.id).await?, "attached");
    assert_eq!(object_state(&db, second.id).await?, "uploading");

    mark_upload_stale(&db, second.id).await?;
    run_cleanup(&db, &state).await;
    assert_eq!(object_state(&db, second.id).await?, "expired");
    assert_eq!(object_get_status(&state, first.id).await, StatusCode::OK);
    assert_eq!(
        object_get_status(&state, second.id).await,
        StatusCode::NOT_FOUND
    );

    delete_attached_file(&db, &state, &caller, space_id, first_node_id).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn mcp_single_upload_guides_put_completion_and_abort()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, _space_id, _root_id) = caller_and_space(&state).await?;
    let (mut request_parts, _) = Request::new(()).into_parts();
    request_parts.extensions.insert(caller);

    let begun = transfers::call(
        &state,
        &request_parts,
        FileTransferInput {
            op: "begin_upload".to_owned(),
            target: Some("rest-test:/guided-single.bin".to_owned()),
            byte_len: Some(4),
            media_type: None,
            original_filename: None,
            encryption_mode: None,
            encryption_metadata: None,
            upload_id: None,
            part_numbers: None,
            completed_parts: None,
        },
    )
    .await?
    .0;
    assert_eq!(begun["next_action"]["kind"], "http_upload");
    assert_eq!(
        begun["next_action"]["then"]["input"]["op"],
        "complete_upload"
    );
    let upload_id = begun["upload_id"].as_str().ok_or("upload id")?.to_owned();

    let aborted = transfers::call(
        &state,
        &request_parts,
        FileTransferInput {
            op: "abort_upload".to_owned(),
            target: None,
            byte_len: None,
            media_type: None,
            original_filename: None,
            encryption_mode: None,
            encryption_metadata: None,
            upload_id: Some(upload_id.clone()),
            part_numbers: None,
            completed_parts: None,
        },
    )
    .await?
    .0;
    assert_eq!(aborted["next_action"]["kind"], "done");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn mcp_multipart_upload_and_presigned_download_round_trip()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, _root_id) = caller_and_space(&state).await?;
    let (mut request_parts, _) = Request::new(()).into_parts();
    request_parts.extensions.insert(caller.clone());
    let byte_len = SINGLE_PUT_MAX_BYTES as i64 + 1;

    let begun = transfers::call(
        &state,
        &request_parts,
        FileTransferInput {
            op: "begin_upload".to_owned(),
            target: Some("rest-test:/large.bin".to_owned()),
            byte_len: Some(byte_len),
            media_type: Some("application/octet-stream".to_owned()),
            original_filename: Some("large.bin".to_owned()),
            encryption_mode: None,
            encryption_metadata: None,
            upload_id: None,
            part_numbers: None,
            completed_parts: None,
        },
    )
    .await?
    .0;
    assert_eq!(begun["transfer"]["mode"], "multipart");
    assert_eq!(begun["transfer"]["part_count"], 2);
    assert_eq!(begun["next_action"]["kind"], "call_tool");
    assert_eq!(begun["next_action"]["input"]["part_numbers"], json!([1, 2]));
    let upload_id = begun["upload_id"].as_str().ok_or("upload id")?.to_owned();

    let prepared = transfers::call(
        &state,
        &request_parts,
        FileTransferInput {
            op: "prepare_parts".to_owned(),
            target: None,
            byte_len: None,
            media_type: None,
            original_filename: None,
            encryption_mode: None,
            encryption_metadata: None,
            upload_id: Some(upload_id.clone()),
            part_numbers: Some(vec![1, 2]),
            completed_parts: None,
        },
    )
    .await?
    .0;
    assert_eq!(prepared["next_action"]["kind"], "http_upload_parts");
    assert_eq!(prepared["next_action"]["collect_response_header"], "etag");
    assert_eq!(prepared["next_action"]["max_concurrency"], 4);
    assert_eq!(
        prepared["next_action"]["repeat"]["input"]["op"],
        "prepare_parts"
    );
    assert_eq!(
        prepared["next_action"]["then"]["input"]["op"],
        "complete_upload"
    );
    let part_transfers = prepared["parts"].as_array().ok_or("part transfers")?;
    assert_eq!(part_transfers.len(), 2);
    let (first, second) = tokio::join!(
        put_mcp_part(&part_transfers[0]),
        put_mcp_part(&part_transfers[1])
    );
    let completed_parts = vec![first?, second?];

    let completed = transfers::call(
        &state,
        &request_parts,
        FileTransferInput {
            op: "complete_upload".to_owned(),
            target: None,
            byte_len: None,
            media_type: None,
            original_filename: None,
            encryption_mode: None,
            encryption_metadata: None,
            upload_id: Some(upload_id.clone()),
            part_numbers: None,
            completed_parts: Some(completed_parts),
        },
    )
    .await?
    .0;
    assert_eq!(completed["next_action"]["kind"], "done");
    let prepare_after_completion = transfers::call(
        &state,
        &request_parts,
        FileTransferInput {
            op: "prepare_parts".to_owned(),
            target: None,
            byte_len: None,
            media_type: None,
            original_filename: None,
            encryption_mode: None,
            encryption_metadata: None,
            upload_id: Some(upload_id),
            part_numbers: Some(vec![1]),
            completed_parts: None,
        },
    )
    .await;
    assert!(prepare_after_completion.is_err());
    let node_id = state
        .files
        .resolve_path(caller.account_id(), space_id, "/large.bin")
        .await?
        .node
        .id;

    let download = transfers::call(
        &state,
        &request_parts,
        FileTransferInput {
            op: "prepare_download".to_owned(),
            target: Some("rest-test:/large.bin".to_owned()),
            byte_len: None,
            media_type: None,
            original_filename: None,
            encryption_mode: None,
            encryption_metadata: None,
            upload_id: None,
            part_numbers: None,
            completed_parts: None,
        },
    )
    .await?
    .0;
    assert_eq!(download["next_action"]["kind"], "http_download");
    let mut response = reqwest::Client::new()
        .get(download["transfer"]["url"].as_str().ok_or("download url")?)
        .send()
        .await?
        .error_for_status()?;
    let mut downloaded = 0_u64;
    while let Some(chunk) = response.chunk().await? {
        downloaded += chunk.len() as u64;
    }
    assert_eq!(downloaded, byte_len as u64);

    delete_attached_file(&db, &state, &caller, space_id, node_id).await?;
    db.cleanup().await;
    Ok(())
}
