use std::collections::BTreeMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use notegate_model::{Caller, FileEncryptionMode};
use notegate_service::files::BeginObjectUpload;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::object_storage::{AGENT_TRANSFER_URL_TTL, CompletedUploadPart};
use crate::object_upload_flow::{
    BegunTransfer, PART_UPLOAD_CONCURRENCY_MAX, PART_URL_BATCH_MAX, abort_upload, begin_upload,
    complete_upload, prepare_parts,
};
use crate::state::AppState;

use super::dto::NodeOut;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/spaces/{space_id}/file-uploads", post(begin))
        .route(
            "/spaces/{space_id}/file-uploads/{upload_id}/parts",
            post(parts),
        )
        .route(
            "/spaces/{space_id}/file-uploads/{upload_id}/complete",
            post(complete),
        )
        .route("/spaces/{space_id}/file-uploads/{upload_id}", delete(abort))
        .route("/spaces/{space_id}/files/{node_id}/download", get(download))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Starts a presigned object upload and reserves the destination file name.
#[schema(example = serde_json::json!({
    "parent_id": "11111111-1111-1111-1111-111111111111",
    "name": "report.pdf",
    "byte_len": 7340032,
    "media_type": "application/pdf",
    "original_filename": "Q3 report.pdf",
    "encryption_mode": "none"
}))]
pub(crate) struct BeginUploadBody {
    /// Existing destination folder.
    parent_id: Uuid,
    /// Name of the file node that will be attached by `complete`.
    name: String,
    /// Exact object size in bytes. The system cap is 100 GiB; space quota can be lower.
    #[schema(minimum = 0, maximum = 107374182400)]
    byte_len: i64,
    /// MIME type stored with the object.
    #[schema(example = "application/pdf")]
    media_type: String,
    /// Optional download filename presented to clients.
    #[serde(default)]
    original_filename: Option<String>,
    /// `none` (default) or `client`. NoteGate never receives a client encryption key.
    #[schema(default = "none", examples("none", "client"))]
    #[serde(default = "default_encryption_mode")]
    encryption_mode: String,
    /// Client-defined JSON object required for `client` encryption and omitted for `none`.
    #[serde(default)]
    encryption_metadata: Option<Value>,
}

#[derive(Debug, Serialize, ToSchema)]
/// Upload ledger identifier and the provider transfer instructions.
pub(crate) struct BeginUploadResponse {
    /// NoteGate upload ledger identifier used by parts, complete, and abort calls.
    upload_id: Uuid,
    /// Lifetime of each returned presigned URL, measured from issuance.
    expires_in_seconds: u64,
    transfer: UploadTransferOut,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
/// Provider transfer mode selected from the declared file size.
pub(crate) enum UploadTransferOut {
    /// Upload up to 100 MiB with one HTTP PUT using every returned header.
    Single {
        url: String,
        headers: BTreeMap<String, String>,
    },
    /// Request part URLs in batches, upload all parts, then submit their ETags to `complete`.
    Multipart {
        /// Normal part size in bytes. The final part can be smaller.
        part_size: i64,
        part_count: i32,
        /// Maximum number of part URLs accepted by one parts request.
        part_url_batch_max: usize,
        /// Recommended maximum number of concurrent part uploads.
        upload_concurrency_max: usize,
    },
}

#[utoipa::path(
    post,
    path = "/api/v2/spaces/{space_id}/file-uploads",
    tag = "files",
    params(("space_id" = Uuid, Path)),
    request_body = BeginUploadBody,
    responses((status = 201, description = "Create an object upload", body = BeginUploadResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn begin(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Json(body): Json<BeginUploadBody>,
) -> Result<(StatusCode, Json<BeginUploadResponse>), ApiError> {
    let encryption_mode = FileEncryptionMode::parse(&body.encryption_mode)
        .ok_or_else(|| ApiError::invalid_field("encryption_mode must be 'none' or 'client'"))?;
    let command = BeginObjectUpload {
        parent_node_id: body.parent_id,
        name: body.name,
        byte_len: body.byte_len,
        media_type: body.media_type,
        original_filename: body.original_filename,
        encryption_mode,
        encryption_metadata: body.encryption_metadata,
    };
    let begun = begin_upload(
        &state,
        caller.account_id(),
        space_id,
        &command,
        AGENT_TRANSFER_URL_TTL,
    )
    .await?;
    let transfer = match begun.transfer {
        BegunTransfer::Single(transfer) => UploadTransferOut::Single {
            url: transfer.url,
            headers: transfer.headers,
        },
        BegunTransfer::Multipart {
            part_size,
            part_count,
        } => UploadTransferOut::Multipart {
            part_size,
            part_count,
            part_url_batch_max: PART_URL_BATCH_MAX,
            upload_concurrency_max: PART_UPLOAD_CONCURRENCY_MAX,
        },
    };
    Ok((
        StatusCode::CREATED,
        Json(BeginUploadResponse {
            upload_id: begun.upload_id,
            expires_in_seconds: AGENT_TRANSFER_URL_TTL.as_secs(),
            transfer,
        }),
    ))
}

fn default_encryption_mode() -> String {
    "none".to_owned()
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Requests presigned PUT URLs for selected multipart parts.
#[schema(example = serde_json::json!({"part_numbers": [1, 2, 3]}))]
pub(crate) struct PreparePartsBody {
    /// Unique 1-based part numbers. At most 16 values are accepted per request.
    #[schema(min_items = 1, max_items = 16)]
    part_numbers: Vec<i32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PreparePartsResponse {
    expires_in_seconds: u64,
    parts: Vec<UploadPartOut>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct UploadPartOut {
    part_number: i32,
    url: String,
    /// Headers that must be included in this provider PUT request.
    headers: BTreeMap<String, String>,
    /// Exact number of bytes required for this part.
    content_length: i64,
}

#[utoipa::path(
    post,
    path = "/api/v2/spaces/{space_id}/file-uploads/{upload_id}/parts",
    tag = "files",
    params(("space_id" = Uuid, Path), ("upload_id" = Uuid, Path)),
    request_body = PreparePartsBody,
    responses((status = 200, description = "Create presigned multipart PUT URLs", body = PreparePartsResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn parts(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, upload_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PreparePartsBody>,
) -> Result<Json<PreparePartsResponse>, ApiError> {
    let upload = state
        .files
        .object_upload(caller.account_id(), space_id, upload_id)
        .await?;
    let parts = prepare_parts(
        &state,
        caller.account_id(),
        upload,
        body.part_numbers,
        AGENT_TRANSFER_URL_TTL,
    )
    .await?
    .into_iter()
    .map(|part| UploadPartOut {
        part_number: part.part_number,
        url: part.transfer.url,
        headers: part.transfer.headers,
        content_length: part.content_length,
    })
    .collect();
    Ok(Json(PreparePartsResponse {
        expires_in_seconds: AGENT_TRANSFER_URL_TTL.as_secs(),
        parts,
    }))
}

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Finalizes a provider upload and attaches its file node.
///
/// Omit `completed_parts` for single PUT uploads. Multipart uploads must include every
/// part number and the ETag returned by the provider PUT response.
#[schema(examples(
    serde_json::json!({}),
    serde_json::json!({
        "completed_parts": [
            {"part_number": 1, "etag": "\"provider-etag-1\""},
            {"part_number": 2, "etag": "\"provider-etag-2\""}
        ]
    })
))]
pub(crate) struct CompleteUploadBody {
    #[serde(default)]
    completed_parts: Option<Vec<CompletedPartBody>>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompletedPartBody {
    /// 1-based part number.
    part_number: i32,
    /// ETag returned by the object provider after uploading this part.
    etag: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileResponse {
    node: NodeOut,
}

#[utoipa::path(
    post,
    path = "/api/v2/spaces/{space_id}/file-uploads/{upload_id}/complete",
    tag = "files",
    params(("space_id" = Uuid, Path), ("upload_id" = Uuid, Path)),
    request_body = Option<CompleteUploadBody>,
    responses((status = 201, description = "Attach a completed object as a file node", body = FileResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn complete(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, upload_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<CompleteUploadBody>>,
) -> Result<(StatusCode, Json<FileResponse>), ApiError> {
    let upload = state
        .files
        .object_upload(caller.account_id(), space_id, upload_id)
        .await?;
    let completed_parts = body
        .and_then(|Json(body)| body.completed_parts)
        .map(|parts| {
            parts
                .into_iter()
                .map(|part| CompletedUploadPart {
                    part_number: part.part_number,
                    etag: part.etag,
                })
                .collect()
        });
    let view = complete_upload(&state, caller.account_id(), upload, completed_parts).await?;
    Ok((
        StatusCode::CREATED,
        Json(FileResponse {
            node: NodeOut::from(&view.node),
        }),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v2/spaces/{space_id}/file-uploads/{upload_id}",
    tag = "files",
    params(("space_id" = Uuid, Path), ("upload_id" = Uuid, Path)),
    responses((status = 204, description = "Queue incomplete upload cleanup")),
    security(("api_key" = []))
)]
pub(crate) async fn abort(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, upload_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let upload = state
        .files
        .object_upload(caller.account_id(), space_id, upload_id)
        .await?;
    abort_upload(&state, caller.account_id(), &upload).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DownloadResponse {
    url: String,
    expires_in_seconds: u64,
}

#[utoipa::path(
    get,
    path = "/api/v2/spaces/{space_id}/files/{node_id}/download",
    tag = "files",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses((status = 200, description = "Create a presigned file download URL", body = DownloadResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn download(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<DownloadResponse>, ApiError> {
    let view = state
        .files
        .file_for_download(caller.account_id(), space_id, node_id)
        .await?;
    let url = state
        .object_storage
        .presign_get_with_ttl(
            &view.file.object_key,
            view.file.original_filename.as_deref(),
            AGENT_TRANSFER_URL_TTL,
        )
        .await?;
    Ok(Json(DownloadResponse {
        url,
        expires_in_seconds: AGENT_TRANSFER_URL_TTL.as_secs(),
    }))
}
