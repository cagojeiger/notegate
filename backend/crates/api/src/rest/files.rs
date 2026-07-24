//! Object-backed file metadata and download redirects.
//!
//! REST handles browser file content. MCP prepares direct transfers through
//! `file_transfer` without carrying file bytes in MCP payloads.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, HeaderValue, LOCATION};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, TimeDelta, Utc};
use notegate_model::{Caller, FileEncryptionMode};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::file_preview::{
    PREVIEW_URL_TTL_SECONDS, detect_object_media_type, is_preview_size_allowed,
    is_previewable_image_type,
};
use crate::rest::dto::{NodeOut, attribution_ids};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/{space_id}/files/{node_id}", get(stat))
        .route(
            "/v1/spaces/{space_id}/files/{node_id}/content",
            get(download),
        )
        .route(
            "/v1/spaces/{space_id}/files/{node_id}/preview-url",
            get(preview_url),
        )
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileResponse {
    pub(crate) node: NodeOut,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FilePreviewUrlResponse {
    pub(crate) url: String,
    pub(crate) media_type: String,
    pub(crate) expires_at: DateTime<Utc>,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/files/{node_id}",
    tag = "files",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses((status = 200, description = "File metadata", body = FileResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn stat(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<FileResponse>, ApiError> {
    let view = state
        .files
        .stat(caller.account_id(), space_id, node_id)
        .await?;
    if view.node.kind != notegate_model::NodeKind::File {
        return Err(ApiError::invalid_field("target is not a file"));
    }
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view]))
        .await?;
    Ok(Json(FileResponse {
        node: NodeOut::from_view(&view, &refs),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/files/{node_id}/content",
    tag = "files",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses(
        (status = 302, description = "Redirect to a presigned object download URL")
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn download(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let file_view = state
        .files
        .file_for_download(caller.account_id(), space_id, node_id)
        .await?;
    let get_url = state
        .object_storage
        .presign_get(
            &file_view.file.object_key,
            file_view.file.original_filename.as_deref(),
        )
        .await?;
    let location =
        HeaderValue::from_str(&get_url).map_err(|_| ApiError::object_storage_unavailable())?;
    Ok((StatusCode::FOUND, [(LOCATION, location)]).into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/files/{node_id}/preview-url",
    tag = "files",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses(
        (status = 200, description = "Short-lived URL for a detected raster image up to 10 MiB", body = FilePreviewUrlResponse),
        (status = 404, description = "File has no supported image preview or exceeds 10 MiB")
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn preview_url(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let file_view = state
        .files
        .file_for_download(caller.account_id(), space_id, node_id)
        .await?;
    if file_view.file.encryption_mode == FileEncryptionMode::Client {
        return Err(ApiError::not_found("file preview is not available"));
    }
    if !is_preview_size_allowed(file_view.file.byte_len) {
        return Err(ApiError::not_found("file preview is not available"));
    }

    let media_type = match file_view.file.detected_media_type {
        Some(media_type) => media_type,
        None => {
            let media_type = detect_object_media_type(
                &state.object_storage,
                &file_view.file.object_key,
                file_view.file.byte_len,
                file_view.file.encryption_mode,
            )
            .await?
            .ok_or_else(|| ApiError::not_found("file preview is not available"))?;
            if let Err(error) = state
                .files
                .record_detected_file_media_type(
                    caller.account_id(),
                    space_id,
                    node_id,
                    &media_type,
                )
                .await
            {
                tracing::warn!(
                    event = "file.detected_media_type_persist_failed",
                    %space_id,
                    %node_id,
                    ?error,
                );
            }
            media_type
        }
    };
    if !is_previewable_image_type(&media_type) {
        return Err(ApiError::not_found("file preview is not available"));
    }

    let ttl = std::time::Duration::from_secs(PREVIEW_URL_TTL_SECONDS as u64);
    let url = state
        .object_storage
        .presign_inline_get(&file_view.file.object_key, &media_type, ttl)
        .await?;
    Ok((
        [(CACHE_CONTROL, HeaderValue::from_static("private, no-store"))],
        Json(FilePreviewUrlResponse {
            url,
            media_type,
            expires_at: Utc::now() + TimeDelta::seconds(PREVIEW_URL_TTL_SECONDS),
        }),
    )
        .into_response())
}
