//! S3-compatible object upload lifecycle.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, post};
use axum::{Json, Router};
use notegate_model::{Caller, FileEncryptionMode};
use notegate_service::files::BeginObjectUpload;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::object_storage::{CompletedUploadPart, TRANSFER_URL_TTL};
use crate::object_upload_flow::{
    BegunTransfer, UploadFlowError, abort_upload, begin_upload, complete_upload, prepare_parts,
};
use crate::rest::dto::{NodeOut, attribution_ids};
use crate::rest::files::FileResponse;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/{space_id}/file-uploads", post(begin))
        .route(
            "/v1/spaces/{space_id}/file-uploads/{upload_id}/parts",
            post(parts),
        )
        .route(
            "/v1/spaces/{space_id}/file-uploads/{upload_id}/complete",
            post(complete),
        )
        .route(
            "/v1/spaces/{space_id}/file-uploads/{upload_id}",
            delete(abort),
        )
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct BeginUploadBody {
    parent_node_id: Uuid,
    name: String,
    byte_len: i64,
    media_type: String,
    original_filename: Option<String>,
    #[serde(default = "default_encryption_mode")]
    encryption_mode: String,
    encryption_metadata: Option<Value>,
}

fn default_encryption_mode() -> String {
    "none".to_owned()
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct BeginUploadResponse {
    upload_id: Uuid,
    transfer: UploadTransferOut,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub(crate) enum UploadTransferOut {
    Single {
        url: String,
        headers: BTreeMap<String, String>,
    },
    Multipart {
        part_size: i64,
        part_count: i32,
    },
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct PreparePartsBody {
    part_numbers: Vec<i32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PreparePartsResponse {
    parts: Vec<UploadPartOut>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct UploadPartOut {
    part_number: i32,
    url: String,
    headers: BTreeMap<String, String>,
    content_length: i64,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub(crate) struct CompleteUploadBody {
    completed_parts: Option<Vec<CompletedPartBody>>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct CompletedPartBody {
    part_number: i32,
    etag: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/file-uploads",
    tag = "files",
    params(("space_id" = Uuid, Path)),
    request_body = BeginUploadBody,
    responses((status = 201, description = "Object upload created", body = BeginUploadResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn begin(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Json(body): Json<BeginUploadBody>,
) -> Result<(StatusCode, Json<BeginUploadResponse>), ApiError> {
    if body.byte_len > notegate_core::limits::BROWSER_FILE_MAX_BYTES as i64 {
        return Err(ApiError::invalid_field(
            "browser uploads support files up to 10737418240 bytes",
        ));
    }
    let encryption_mode = FileEncryptionMode::parse(&body.encryption_mode)
        .ok_or_else(|| ApiError::invalid_field("encryption_mode must be 'none' or 'client'"))?;
    let command = BeginObjectUpload {
        parent_node_id: body.parent_node_id,
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
        TRANSFER_URL_TTL,
    )
    .await
    .map_err(api_error)?;
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
        },
    };
    Ok((
        StatusCode::CREATED,
        Json(BeginUploadResponse {
            upload_id: begun.upload_id,
            transfer,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/file-uploads/{upload_id}/parts",
    tag = "files",
    params(("space_id" = Uuid, Path), ("upload_id" = Uuid, Path)),
    request_body = PreparePartsBody,
    responses((status = 200, description = "Presigned multipart upload URLs", body = PreparePartsResponse)),
    security(("bearer_auth" = []))
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
        TRANSFER_URL_TTL,
    )
    .await
    .map_err(api_error)?
    .into_iter()
    .map(|part| UploadPartOut {
        part_number: part.part_number,
        url: part.transfer.url,
        headers: part.transfer.headers,
        content_length: part.content_length,
    })
    .collect();
    Ok(Json(PreparePartsResponse { parts }))
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/file-uploads/{upload_id}/complete",
    tag = "files",
    params(("space_id" = Uuid, Path), ("upload_id" = Uuid, Path)),
    request_body = Option<CompleteUploadBody>,
    responses((status = 201, description = "Object file attached", body = FileResponse)),
    security(("bearer_auth" = []))
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
    let view = complete_upload(&state, caller.account_id(), upload, completed_parts)
        .await
        .map_err(api_error)?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view.node]))
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(FileResponse {
            node: NodeOut::from_view(&view.node, &refs),
        }),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/spaces/{space_id}/file-uploads/{upload_id}",
    tag = "files",
    params(("space_id" = Uuid, Path), ("upload_id" = Uuid, Path)),
    responses((status = 204, description = "Object upload cleanup queued")),
    security(("bearer_auth" = []))
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
    abort_upload(&state, caller.account_id(), &upload)
        .await
        .map_err(api_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn api_error(error: UploadFlowError) -> ApiError {
    match error {
        UploadFlowError::InvalidInput(message) => ApiError::invalid_field(message),
        UploadFlowError::Service(error) => error.into(),
        UploadFlowError::Storage(error) => error.into(),
        UploadFlowError::Internal(message) => {
            tracing::error!(event = "error.internal", detail = message);
            ApiError::internal("internal server error")
        }
    }
}
