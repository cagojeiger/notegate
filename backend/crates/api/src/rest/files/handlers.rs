use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use notegate_db::{FilesRepo, FindRequest as RepoFindRequest, GrepRequest as RepoGrepRequest};
use notegate_domain::Caller;
use uuid::Uuid;

use super::dto::{
    ChildrenResponse, CreateNodeRequest, DocumentResponse, FindRequest, FindResponse, GrepRequest,
    GrepResponse, MoveNodeRequest, NodeOutput, NodeResponseBody, ResolveQuery, SaveDocumentRequest,
};
use super::error::map_files_error;
use crate::error::ApiError;
use crate::state::AppState;

pub(super) async fn root(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<NodeResponseBody>, ApiError> {
    let repo = FilesRepo::new(state.db.clone());
    let node = repo.root(caller.user.id).await.map_err(map_files_error)?;
    Ok(Json(NodeResponseBody {
        node: NodeOutput::from(node),
    }))
}

pub(super) async fn resolve(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ResolveQuery>,
) -> Result<Json<NodeResponseBody>, ApiError> {
    let repo = FilesRepo::new(state.db.clone());
    let node = repo
        .resolve(caller.user.id, &query.path)
        .await
        .map_err(map_files_error)?;
    Ok(Json(NodeResponseBody {
        node: NodeOutput::from(node),
    }))
}

pub(super) async fn children(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<ChildrenResponse>, ApiError> {
    let repo = FilesRepo::new(state.db.clone());
    let result = repo
        .children(caller.user.id, node_id)
        .await
        .map_err(map_files_error)?;
    Ok(Json(ChildrenResponse::from(result)))
}

pub(super) async fn create_folder(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<CreateNodeRequest>,
) -> Result<Json<NodeResponseBody>, ApiError> {
    let repo = FilesRepo::new(state.db.clone());
    let node = repo
        .create_folder(caller.user.id, request.parent_node_id, &request.name)
        .await
        .map_err(map_files_error)?;
    Ok(Json(NodeResponseBody {
        node: NodeOutput::from(node),
    }))
}

pub(super) async fn create_document(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<CreateNodeRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    let repo = FilesRepo::new(state.db.clone());
    let bundle = repo
        .create_document(caller.user.id, request.parent_node_id, &request.name)
        .await
        .map_err(map_files_error)?;
    Ok(Json(DocumentResponse::from(bundle)))
}

pub(super) async fn open_document(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<DocumentResponse>, ApiError> {
    let repo = FilesRepo::new(state.db.clone());
    let bundle = repo
        .document(caller.user.id, node_id)
        .await
        .map_err(map_files_error)?;
    Ok(Json(DocumentResponse::from(bundle)))
}

pub(super) async fn save_document(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(node_id): Path<Uuid>,
    Json(request): Json<SaveDocumentRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    let repo = FilesRepo::new(state.db.clone());
    let bundle = repo
        .save_document(caller.user.id, node_id, &request.content_md)
        .await
        .map_err(map_files_error)?;
    Ok(Json(DocumentResponse::from(bundle)))
}

pub(super) async fn move_node(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(node_id): Path<Uuid>,
    Json(request): Json<MoveNodeRequest>,
) -> Result<Json<NodeResponseBody>, ApiError> {
    let repo = FilesRepo::new(state.db.clone());
    let node = repo
        .move_node(
            caller.user.id,
            node_id,
            request.new_parent_node_id,
            request.new_name.as_deref(),
        )
        .await
        .map_err(map_files_error)?;
    Ok(Json(NodeResponseBody {
        node: NodeOutput::from(node),
    }))
}

pub(super) async fn delete_node(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(node_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let repo = FilesRepo::new(state.db.clone());
    repo.delete_node(caller.user.id, node_id)
        .await
        .map_err(map_files_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn find(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<FindRequest>,
) -> Result<Json<FindResponse>, ApiError> {
    let repo = FilesRepo::new(state.db.clone());
    let results = repo
        .find(
            caller.user.id,
            RepoFindRequest {
                q: request.q,
                path: request.path,
                kind: request.kind,
                limit: request.limit,
            },
        )
        .await
        .map_err(map_files_error)?
        .into_iter()
        .map(NodeOutput::from)
        .collect();
    Ok(Json(FindResponse { results }))
}

pub(super) async fn grep(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<GrepRequest>,
) -> Result<Json<GrepResponse>, ApiError> {
    let repo = FilesRepo::new(state.db.clone());
    let results = repo
        .grep(
            caller.user.id,
            RepoGrepRequest {
                q: request.q,
                path: request.path,
                context: request.context,
                limit: request.limit,
            },
        )
        .await
        .map_err(map_files_error)?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(GrepResponse { results }))
}
