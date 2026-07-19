//! S3-compatible object upload lifecycle.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use notegate_model::{Caller, FileEncryptionMode};
use notegate_service::files::BeginObjectUpload;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::rest::dto::{NodeOut, attribution_ids};
use crate::rest::files::FileResponse;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/{space_id}/file-uploads", post(begin))
        .route(
            "/v1/spaces/{space_id}/file-uploads/{upload_id}/complete",
            post(complete),
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
    state
        .files
        .prepare_object_upload(caller.account_id(), space_id, &command)
        .await?;
    let storage = state
        .object_storage
        .as_ref()
        .ok_or_else(ApiError::object_storage_unavailable)?;

    let upload_id = Uuid::new_v4();
    let object_key = format!("objects/{upload_id}");
    state
        .files
        .record_object_upload(
            upload_id,
            &object_key,
            caller.account_id(),
            space_id,
            &command,
        )
        .await?;
    let transfer = storage
        .presign_put(&object_key, &command.media_type, command.byte_len)
        .await?;

    tracing::info!(
        event = "object_storage.upload_created",
        %upload_id,
        %space_id,
        account_id = %caller.account_id(),
        byte_len = command.byte_len,
    );
    Ok((
        StatusCode::CREATED,
        Json(BeginUploadResponse {
            upload_id,
            transfer: UploadTransferOut::Single {
                url: transfer.url,
                headers: transfer.headers,
            },
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/file-uploads/{upload_id}/complete",
    tag = "files",
    params(("space_id" = Uuid, Path), ("upload_id" = Uuid, Path)),
    responses((status = 201, description = "Object file attached", body = FileResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn complete(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, upload_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<FileResponse>), ApiError> {
    let upload = state
        .files
        .object_upload(caller.account_id(), space_id, upload_id)
        .await?;
    if upload.node_id.is_none() {
        let storage = state
            .object_storage
            .as_ref()
            .ok_or_else(ApiError::object_storage_unavailable)?;
        let etag = storage
            .verify_upload(&upload.object_key, upload.byte_len)
            .await?;
        tracing::info!(
            event = "object_storage.upload_verified",
            %upload_id,
            %space_id,
            %etag,
        );
        state
            .files
            .touch_object_upload(caller.account_id(), space_id, upload_id)
            .await?;
    }

    let view = state
        .files
        .complete_object_upload(caller.account_id(), space_id, upload_id)
        .await?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view.node]))
        .await?;
    tracing::info!(
        event = "object_storage.file_attached",
        %upload_id,
        node_id = %view.node.node.id,
        %space_id,
    );
    Ok((
        StatusCode::CREATED,
        Json(FileResponse {
            node: NodeOut::from_view(&view.node, &refs),
        }),
    ))
}
