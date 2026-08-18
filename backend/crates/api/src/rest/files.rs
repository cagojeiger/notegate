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
    PREVIEW_URL_TTL_SECONDS, audio_preview_media_type, detect_object_media_type,
    is_preview_size_allowed, is_previewable_docx_type, is_previewable_image_type,
    is_previewable_pdf_type,
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
            "/v1/spaces/{space_id}/files/{node_id}/pdf-preview-url",
            get(pdf_preview_url),
        )
        .route(
            "/v1/spaces/{space_id}/files/{node_id}/docx-preview-url",
            get(docx_preview_url),
        )
        .route(
            "/v1/spaces/{space_id}/files/{node_id}/audio-preview-url",
            get(audio_preview_url),
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
    security(("browser_session" = []))
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
    security(("browser_session" = []))
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
    security(("browser_session" = []))
)]
pub(crate) async fn preview_url(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    preview_response(&state, &caller, space_id, node_id, PreviewPolicy::Image).await
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/files/{node_id}/pdf-preview-url",
    tag = "files",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses(
        (status = 200, description = "Short-lived URL for a detected PDF up to 10 MiB", body = FilePreviewUrlResponse),
        (status = 404, description = "File has no supported PDF preview or exceeds 10 MiB")
    ),
    security(("browser_session" = []))
)]
pub(crate) async fn pdf_preview_url(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    preview_response(&state, &caller, space_id, node_id, PreviewPolicy::Pdf).await
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/files/{node_id}/docx-preview-url",
    tag = "files",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses(
        (status = 200, description = "Short-lived inline URL for a verified DOCX package up to 10 MiB", body = FilePreviewUrlResponse),
        (status = 404, description = "File has no supported DOCX preview or exceeds 10 MiB")
    ),
    security(("browser_session" = []))
)]
pub(crate) async fn docx_preview_url(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    preview_response(&state, &caller, space_id, node_id, PreviewPolicy::Docx).await
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/files/{node_id}/audio-preview-url",
    tag = "files",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses(
        (status = 200, description = "Short-lived inline URL for server-verified audio", body = FilePreviewUrlResponse),
        (status = 404, description = "File has no supported audio preview")
    ),
    security(("browser_session" = []))
)]
pub(crate) async fn audio_preview_url(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    preview_response(&state, &caller, space_id, node_id, PreviewPolicy::Audio).await
}

async fn preview_response(
    state: &AppState,
    caller: &Caller,
    space_id: Uuid,
    node_id: Uuid,
    policy: PreviewPolicy,
) -> Result<Response, ApiError> {
    let file_view = state
        .files
        .file_for_download(caller.account_id(), space_id, node_id)
        .await?;
    let prepared = prepare_preview(
        state,
        &file_view.file,
        Some(file_view.node.node.name.as_str()),
        policy,
    )
    .await;
    let detected_media_type = match &prepared {
        Ok(prepared) => prepared.detected_media_type.clone(),
        Err(error) => error.detected_media_type().map(str::to_owned),
    };
    if let Some(media_type) = detected_media_type {
        state
            .metadata_writes
            .record_media_type(space_id, node_id, media_type, Utc::now());
    }
    let preview = prepared
        .map(|prepared| prepared.preview)
        .map_err(|error| match error {
            PreviewPreparationError::Unsupported { .. } => {
                ApiError::not_found(policy.unavailable_message())
            }
            PreviewPreparationError::Storage { error, .. } => error,
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
    security(("browser_session" = []))
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
    let outcomes: Vec<BatchPreviewOutcome> =
        stream::iter(candidates.into_iter().map(|candidate| {
            let state = state.clone();
            async move { batch_preview_item(&state, space_id, candidate).await }
        }))
        .buffered(BATCH_PREVIEW_CONCURRENCY)
        .collect()
        .await;
    let detected_media_types = outcomes
        .iter()
        .filter_map(|outcome| outcome.detected_media_type.clone())
        .collect::<Vec<_>>();
    let observed_at = Utc::now();
    for (node_id, media_type) in detected_media_types {
        state
            .metadata_writes
            .record_media_type(space_id, node_id, media_type, observed_at);
    }
    let results = outcomes.into_iter().map(|outcome| outcome.item).collect();

    Ok((
        [(CACHE_CONTROL, HeaderValue::from_static("private, no-store"))],
        Json(BatchFilePreviewResponse { results }),
    )
        .into_response())
}

enum PreviewPreparationError {
    Unsupported {
        detected_media_type: Option<String>,
    },
    Storage {
        error: ApiError,
        detected_media_type: Option<String>,
    },
}

impl PreviewPreparationError {
    fn detected_media_type(&self) -> Option<&str> {
        match self {
            Self::Unsupported {
                detected_media_type,
            }
            | Self::Storage {
                detected_media_type,
                ..
            } => detected_media_type.as_deref(),
        }
    }
}

struct PreparedPreview {
    preview: FilePreviewUrlResponse,
    detected_media_type: Option<String>,
}

struct BatchPreviewOutcome {
    item: BatchFilePreviewItem,
    detected_media_type: Option<(Uuid, String)>,
}

#[derive(Clone, Copy)]
enum PreviewPolicy {
    Image,
    Pdf,
    Docx,
    Audio,
}

impl PreviewPolicy {
    fn response_media_type(
        self,
        declared_media_type: &str,
        detected_media_type: &str,
    ) -> Option<String> {
        match self {
            Self::Image if is_previewable_image_type(detected_media_type) => {
                Some(detected_media_type.to_owned())
            }
            Self::Pdf if is_previewable_pdf_type(detected_media_type) => {
                Some(detected_media_type.to_owned())
            }
            Self::Docx if is_previewable_docx_type(detected_media_type) => {
                Some(detected_media_type.to_owned())
            }
            Self::Audio => audio_preview_media_type(declared_media_type, detected_media_type),
            _ => None,
        }
    }

    fn allows_size(self, byte_len: i64) -> bool {
        match self {
            Self::Image | Self::Pdf | Self::Docx => is_preview_size_allowed(byte_len),
            Self::Audio => byte_len > 0,
        }
    }

    fn unavailable_message(self) -> &'static str {
        match self {
            Self::Image => "file preview is not available",
            Self::Pdf => "pdf preview is not available",
            Self::Docx => "docx preview is not available",
            Self::Audio => "audio preview is not available",
        }
    }
}

async fn prepare_preview(
    state: &AppState,
    file: &FileObject,
    filename_hint: Option<&str>,
    policy: PreviewPolicy,
) -> Result<PreparedPreview, PreviewPreparationError> {
    if file.encryption_mode != FileEncryptionMode::None || !policy.allows_size(file.byte_len) {
        return Err(PreviewPreparationError::Unsupported {
            detected_media_type: None,
        });
    }

    let should_detect_media_type = file.detected_media_type.is_none()
        || (matches!(policy, PreviewPolicy::Docx)
            && !file
                .detected_media_type
                .as_deref()
                .is_some_and(is_previewable_docx_type));
    let (media_type, detected_media_type) = if should_detect_media_type {
        let media_type = detect_object_media_type(
            &state.object_storage,
            &file.object_key,
            file.byte_len,
            file.encryption_mode,
            &file.media_type,
            file.original_filename.as_deref().or(filename_hint),
        )
        .await
        .map_err(|error| PreviewPreparationError::Storage {
            error: error.into(),
            detected_media_type: None,
        })?
        .ok_or(PreviewPreparationError::Unsupported {
            detected_media_type: None,
        })?;
        (media_type.clone(), Some(media_type))
    } else if let Some(media_type) = &file.detected_media_type {
        (media_type.clone(), None)
    } else {
        return Err(PreviewPreparationError::Unsupported {
            detected_media_type: None,
        });
    };
    let Some(media_type) = policy.response_media_type(&file.media_type, &media_type) else {
        return Err(PreviewPreparationError::Unsupported {
            detected_media_type,
        });
    };

    let ttl = std::time::Duration::from_secs(PREVIEW_URL_TTL_SECONDS as u64);
    let url = state
        .object_storage
        .presign_inline_get(&file.object_key, &media_type, ttl)
        .await
        .map_err(|error| PreviewPreparationError::Storage {
            error: error.into(),
            detected_media_type: detected_media_type.clone(),
        })?;
    Ok(PreparedPreview {
        preview: FilePreviewUrlResponse {
            url,
            media_type,
            expires_at: Utc::now() + TimeDelta::seconds(PREVIEW_URL_TTL_SECONDS),
        },
        detected_media_type,
    })
}

async fn batch_preview_item(
    state: &AppState,
    space_id: Uuid,
    candidate: BatchPreviewCandidate,
) -> BatchPreviewOutcome {
    let Some(node) = candidate.node else {
        return batch_outcome(
            batch_item(candidate.path, BatchFilePreviewStatus::NotFound, None, None),
            None,
        );
    };
    if node.kind != NodeKind::File {
        return batch_outcome(
            batch_item(
                candidate.path,
                BatchFilePreviewStatus::Unsupported,
                Some(node.id),
                None,
            ),
            None,
        );
    }
    let Some(file) = candidate.file else {
        tracing::error!(
            event = "file.batch_preview_metadata_missing",
            %space_id,
            node_id = %node.id,
        );
        return batch_outcome(
            batch_item(
                candidate.path,
                BatchFilePreviewStatus::Error,
                Some(node.id),
                None,
            ),
            None,
        );
    };

    match prepare_preview(state, &file, Some(node.name.as_str()), PreviewPolicy::Image).await {
        Ok(prepared) => {
            let detection = prepared
                .detected_media_type
                .clone()
                .map(|media_type| (node.id, media_type));
            batch_outcome(
                BatchFilePreviewItem {
                    path: candidate.path,
                    status: BatchFilePreviewStatus::Ready,
                    node_id: Some(node.id),
                    media_type: Some(prepared.preview.media_type),
                    url: Some(prepared.preview.url),
                    expires_at: Some(prepared.preview.expires_at),
                },
                detection,
            )
        }
        Err(PreviewPreparationError::Unsupported {
            detected_media_type,
        }) => {
            let media_type = file
                .detected_media_type
                .or_else(|| detected_media_type.clone());
            let detection = detected_media_type.map(|media_type| (node.id, media_type));
            batch_outcome(
                batch_item(
                    candidate.path,
                    BatchFilePreviewStatus::Unsupported,
                    Some(node.id),
                    media_type,
                ),
                detection,
            )
        }
        Err(PreviewPreparationError::Storage {
            error,
            detected_media_type,
        }) => {
            tracing::warn!(
                event = "file.batch_preview_failed",
                %space_id,
                node_id = %node.id,
                ?error,
            );
            let detection = detected_media_type.map(|media_type| (node.id, media_type));
            batch_outcome(
                batch_item(
                    candidate.path,
                    BatchFilePreviewStatus::Error,
                    Some(node.id),
                    None,
                ),
                detection,
            )
        }
    }
}

fn batch_outcome(
    item: BatchFilePreviewItem,
    detected_media_type: Option<(Uuid, String)>,
) -> BatchPreviewOutcome {
    BatchPreviewOutcome {
        item,
        detected_media_type,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_error_retains_a_detected_media_type() {
        let error = PreviewPreparationError::Storage {
            error: ApiError::object_storage_unavailable(),
            detected_media_type: Some("image/png".to_owned()),
        };

        assert_eq!(error.detected_media_type(), Some("image/png"));
    }

    #[test]
    fn audio_preview_is_not_subject_to_the_image_and_pdf_size_cap() {
        assert!(PreviewPolicy::Audio.allows_size(crate::file_preview::PREVIEW_MAX_BYTES + 1));
        assert!(!PreviewPolicy::Audio.allows_size(0));
        assert!(!PreviewPolicy::Image.allows_size(crate::file_preview::PREVIEW_MAX_BYTES + 1));
    }
}
