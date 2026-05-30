use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use notegate_db::FilesRepo;
use notegate_domain::Caller;
use notegate_domain::files::{
    CreateDocument, CreateFolder, FilesService, FindRequest as DomainFindRequest,
    GrepRequest as DomainGrepRequest, MoveNode, SaveDocument,
};
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
    let service = files_service(&state);
    let node = service
        .root(caller.user.id)
        .await
        .map_err(map_files_error)?;
    Ok(Json(NodeResponseBody {
        node: NodeOutput::from(node),
    }))
}

pub(super) async fn resolve(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ResolveQuery>,
) -> Result<Json<NodeResponseBody>, ApiError> {
    let service = files_service(&state);
    let node = service
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
    let service = files_service(&state);
    let result = service
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
    let service = files_service(&state);
    let node = service
        .create_folder(
            caller.user.id,
            CreateFolder {
                parent_node_id: request.parent_node_id,
                name: request.name,
            },
        )
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
    let service = files_service(&state);
    let bundle = service
        .create_document(
            caller.user.id,
            CreateDocument {
                parent_node_id: request.parent_node_id,
                name: request.name,
            },
        )
        .await
        .map_err(map_files_error)?;
    Ok(Json(DocumentResponse::from(bundle)))
}

pub(super) async fn open_document(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<DocumentResponse>, ApiError> {
    let service = files_service(&state);
    let bundle = service
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
    let service = files_service(&state);
    let bundle = service
        .save_document(
            caller.user.id,
            SaveDocument {
                node_id,
                content_md: request.content_md,
            },
        )
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
    let service = files_service(&state);
    let node = service
        .move_node(
            caller.user.id,
            MoveNode {
                node_id,
                new_parent_node_id: request.new_parent_node_id,
                new_name: request.new_name,
            },
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
    let service = files_service(&state);
    service
        .delete_node(caller.user.id, node_id)
        .await
        .map_err(map_files_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn find(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<FindRequest>,
) -> Result<Json<FindResponse>, ApiError> {
    let service = files_service(&state);
    let results = service
        .find(
            caller.user.id,
            DomainFindRequest {
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
    let service = files_service(&state);
    let results = service
        .grep(
            caller.user.id,
            DomainGrepRequest {
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

fn files_service(state: &AppState) -> FilesService<FilesRepo> {
    FilesService::new(FilesRepo::new(state.db.clone()))
}
