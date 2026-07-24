//! Object-backed file metadata and download redirects.
//!
//! REST handles browser file content. MCP prepares direct transfers through
//! `file_transfer` without carrying file bytes in MCP payloads.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, HeaderValue, LOCATION};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, TimeDelta, Utc};
use futures_util::{StreamExt, stream};
use notegate_model::{Caller, FileEncryptionMode, FileObject, NodeKind};
use notegate_service::files::BatchPreviewCandidate;
use serde::{Deserialize, Serialize};
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
        .route(
            "/v1/spaces/{space_id}/file-previews:batchResolve",
            post(batch_preview_urls),
        )
}

const BATCH_PREVIEW_CONCURRENCY: usize = 4;

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

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct BatchFilePreviewRequest {
    pub(crate) paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BatchFilePreviewStatus {
    Ready,
    NotFound,
    Unsupported,
    Error,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct BatchFilePreviewItem {
    pub(crate) path: String,
    pub(crate) status: BatchFilePreviewStatus,
    pub(crate) node_id: Option<Uuid>,
    pub(crate) media_type: Option<String>,
    pub(crate) url: Option<String>,
    pub(crate) expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct BatchFilePreviewResponse {
    pub(crate) results: Vec<BatchFilePreviewItem>,
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
    let preview = prepare_preview(
        &state,
        caller.account_id(),
        space_id,
        node_id,
        &file_view.file,
    )
    .await
    .map_err(|error| match error {
        PreviewPreparationError::Unsupported => {
            ApiError::not_found("file preview is not available")
        }
        PreviewPreparationError::Storage(error) => error,
    })?;
    Ok((
        [(CACHE_CONTROL, HeaderValue::from_static("private, no-store"))],
        Json(preview),
    )
        .into_response())
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/file-previews:batchResolve",
    tag = "files",
    params(("space_id" = Uuid, Path)),
    request_body = BatchFilePreviewRequest,
    responses(
        (status = 200, description = "Ordered per-path image preview results", body = BatchFilePreviewResponse),
        (status = 400, description = "Invalid, duplicate, or excessive path input")
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn batch_preview_urls(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Json(request): Json<BatchFilePreviewRequest>,
) -> Result<Response, ApiError> {
    let candidates = state
        .files
        .batch_preview_candidates(caller.account_id(), space_id, request.paths)
        .await?;
    let account_id = caller.account_id();
    let results = stream::iter(candidates.into_iter().map(|candidate| {
        let state = state.clone();
        async move { batch_preview_item(&state, account_id, space_id, candidate).await }
    }))
    .buffered(BATCH_PREVIEW_CONCURRENCY)
    .collect()
    .await;

    Ok((
        [(CACHE_CONTROL, HeaderValue::from_static("private, no-store"))],
        Json(BatchFilePreviewResponse { results }),
    )
        .into_response())
}

enum PreviewPreparationError {
    Unsupported,
    Storage(ApiError),
}

async fn prepare_preview(
    state: &AppState,
    account_id: Uuid,
    space_id: Uuid,
    node_id: Uuid,
    file: &FileObject,
) -> Result<FilePreviewUrlResponse, PreviewPreparationError> {
    if file.encryption_mode == FileEncryptionMode::Client || !is_preview_size_allowed(file.byte_len)
    {
        return Err(PreviewPreparationError::Unsupported);
    }

    let media_type = match file.detected_media_type.as_deref() {
        Some(media_type) => media_type.to_owned(),
        None => {
            let media_type = detect_object_media_type(
                &state.object_storage,
                &file.object_key,
                file.byte_len,
                file.encryption_mode,
            )
            .await
            .map_err(|error| PreviewPreparationError::Storage(error.into()))?
            .ok_or(PreviewPreparationError::Unsupported)?;
            if let Err(error) = state
                .files
                .record_detected_file_media_type(account_id, space_id, node_id, &media_type)
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
        return Err(PreviewPreparationError::Unsupported);
    }

    let ttl = std::time::Duration::from_secs(PREVIEW_URL_TTL_SECONDS as u64);
    let url = state
        .object_storage
        .presign_inline_get(&file.object_key, &media_type, ttl)
        .await
        .map_err(|error| PreviewPreparationError::Storage(error.into()))?;
    Ok(FilePreviewUrlResponse {
        url,
        media_type,
        expires_at: Utc::now() + TimeDelta::seconds(PREVIEW_URL_TTL_SECONDS),
    })
}

async fn batch_preview_item(
    state: &AppState,
    account_id: Uuid,
    space_id: Uuid,
    candidate: BatchPreviewCandidate,
) -> BatchFilePreviewItem {
    let Some(node) = candidate.node else {
        return batch_item(candidate.path, BatchFilePreviewStatus::NotFound, None, None);
    };
    if node.kind != NodeKind::File {
        return batch_item(
            candidate.path,
            BatchFilePreviewStatus::Unsupported,
            Some(node.id),
            None,
        );
    }
    let Some(file) = candidate.file else {
        tracing::error!(
            event = "file.batch_preview_metadata_missing",
            %space_id,
            node_id = %node.id,
        );
        return batch_item(
            candidate.path,
            BatchFilePreviewStatus::Error,
            Some(node.id),
            None,
        );
    };

    match prepare_preview(state, account_id, space_id, node.id, &file).await {
        Ok(preview) => BatchFilePreviewItem {
            path: candidate.path,
            status: BatchFilePreviewStatus::Ready,
            node_id: Some(node.id),
            media_type: Some(preview.media_type),
            url: Some(preview.url),
            expires_at: Some(preview.expires_at),
        },
        Err(PreviewPreparationError::Unsupported) => batch_item(
            candidate.path,
            BatchFilePreviewStatus::Unsupported,
            Some(node.id),
            file.detected_media_type,
        ),
        Err(PreviewPreparationError::Storage(error)) => {
            tracing::warn!(
                event = "file.batch_preview_failed",
                %space_id,
                node_id = %node.id,
                ?error,
            );
            batch_item(
                candidate.path,
                BatchFilePreviewStatus::Error,
                Some(node.id),
                None,
            )
        }
    }
}

fn batch_item(
    path: String,
    status: BatchFilePreviewStatus,
    node_id: Option<Uuid>,
    media_type: Option<String>,
) -> BatchFilePreviewItem {
    BatchFilePreviewItem {
        path,
        status,
        node_id,
        media_type,
        url: None,
        expires_at: None,
    }
}
