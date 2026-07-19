#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Request, StatusCode, header::LOCATION};
use notegate_core::S3Config;
use notegate_db::{FilesRepo, ObjectStorageRepo, test_support::TestDb};
use notegate_model::Caller;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as _;
use uuid::Uuid;

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
    let (status, body) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/file-uploads"),
        json!({
            "parent_node_id": parent_node_id,
            "name": name,
            "byte_len": byte_len,
            "media_type": "application/octet-stream"
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
    sqlx::query(
        "UPDATE object_storage_objects \
         SET last_activity_at = now() - interval '1 hour' WHERE id = $1",
    )
    .bind(upload_id)
    .execute(&db.pool)
    .await?;
    Ok(())
}

async fn run_cleanup(db: &TestDb, state: &crate::state::AppState) {
    crate::object_storage_cleanup_worker::run_once(
        &ObjectStorageRepo::new(db.pool.clone()),
        state.object_storage.as_ref().expect("object storage"),
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
        .as_ref()
        .expect("object storage")
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
async fn begin_rejects_too_many_pending_uploads() -> Result<(), Box<dyn std::error::Error>> {
    let Some(s3) = test_s3_config() else {
        return Ok(());
    };
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state_with_s3(&db, s3);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;

    // Fill the per-account concurrent-upload allowance; quota is only charged at
    // `/complete`, so this cap is what bounds unattached staging in object storage.
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
        .as_ref()
        .expect("object storage")
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
        .as_ref()
        .expect("object storage")
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
