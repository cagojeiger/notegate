//! Object-backed file metadata and download redirects.
//!
//! REST handles file content. MCP only exposes file nodes and metadata through
//! `ls`/`stat`/`find`.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::http::header::{HeaderValue, LOCATION};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use notegate_model::Caller;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::rest::dto::{NodeOut, attribution_ids};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/{space_id}/files/{node_id}", get(stat))
        .route(
            "/v1/spaces/{space_id}/files/{node_id}/content",
            get(download),
        )
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileResponse {
    pub(crate) node: NodeOut,
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
