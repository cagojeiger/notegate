use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use notegate_db::{
    Children, Document, DocumentBundle, FindRequest as RepoFindRequest, GrepMatch,
    GrepRequest as RepoGrepRequest, Node, VaultRepo, VaultRepoError,
};
use notegate_domain::Caller;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/vault/root", get(root))
        .route("/v1/vault/resolve", get(resolve))
        .route("/v1/vault/nodes/{node_id}/children", get(children))
        .route("/v1/vault/nodes/{node_id}/move", patch(move_node))
        .route("/v1/vault/nodes/{node_id}", delete(delete_node))
        .route("/v1/vault/folders", post(create_folder))
        .route("/v1/vault/documents", post(create_document))
        .route(
            "/v1/vault/documents/{node_id}",
            get(open_document).patch(save_document),
        )
        .route("/v1/vault/search/find", post(find))
        .route("/v1/vault/search/grep", post(grep))
}

async fn root(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<NodeResponseBody>, ApiError> {
    let repo = VaultRepo::new(state.db.clone());
    let node = repo.root(caller.user.id).await.map_err(map_vault_error)?;
    Ok(Json(NodeResponseBody {
        node: NodeOutput::from(node),
    }))
}

async fn resolve(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ResolveQuery>,
) -> Result<Json<NodeResponseBody>, ApiError> {
    let repo = VaultRepo::new(state.db.clone());
    let node = repo
        .resolve(caller.user.id, &query.path)
        .await
        .map_err(map_vault_error)?;
    Ok(Json(NodeResponseBody {
        node: NodeOutput::from(node),
    }))
}

async fn children(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<ChildrenResponse>, ApiError> {
    let repo = VaultRepo::new(state.db.clone());
    let result = repo
        .children(caller.user.id, node_id)
        .await
        .map_err(map_vault_error)?;
    Ok(Json(ChildrenResponse::from(result)))
}

async fn create_folder(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<CreateNodeRequest>,
) -> Result<Json<NodeResponseBody>, ApiError> {
    let repo = VaultRepo::new(state.db.clone());
    let node = repo
        .create_folder(caller.user.id, request.parent_node_id, &request.name)
        .await
        .map_err(map_vault_error)?;
    Ok(Json(NodeResponseBody {
        node: NodeOutput::from(node),
    }))
}

async fn create_document(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<CreateNodeRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    let repo = VaultRepo::new(state.db.clone());
    let bundle = repo
        .create_document(caller.user.id, request.parent_node_id, &request.name)
        .await
        .map_err(map_vault_error)?;
    Ok(Json(DocumentResponse::from(bundle)))
}

async fn open_document(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<DocumentResponse>, ApiError> {
    let repo = VaultRepo::new(state.db.clone());
    let bundle = repo
        .document(caller.user.id, node_id)
        .await
        .map_err(map_vault_error)?;
    Ok(Json(DocumentResponse::from(bundle)))
}

async fn save_document(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(node_id): Path<Uuid>,
    Json(request): Json<SaveDocumentRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    let repo = VaultRepo::new(state.db.clone());
    let bundle = repo
        .save_document(caller.user.id, node_id, &request.content_md)
        .await
        .map_err(map_vault_error)?;
    Ok(Json(DocumentResponse::from(bundle)))
}

async fn move_node(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(node_id): Path<Uuid>,
    Json(request): Json<MoveNodeRequest>,
) -> Result<Json<NodeResponseBody>, ApiError> {
    let repo = VaultRepo::new(state.db.clone());
    let node = repo
        .move_node(
            caller.user.id,
            node_id,
            request.new_parent_node_id,
            request.new_name.as_deref(),
        )
        .await
        .map_err(map_vault_error)?;
    Ok(Json(NodeResponseBody {
        node: NodeOutput::from(node),
    }))
}

async fn delete_node(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(node_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let repo = VaultRepo::new(state.db.clone());
    repo.delete_node(caller.user.id, node_id)
        .await
        .map_err(map_vault_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn find(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<FindRequest>,
) -> Result<Json<FindResponse>, ApiError> {
    let repo = VaultRepo::new(state.db.clone());
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
        .map_err(map_vault_error)?
        .into_iter()
        .map(NodeOutput::from)
        .collect();
    Ok(Json(FindResponse { results }))
}

async fn grep(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<GrepRequest>,
) -> Result<Json<GrepResponse>, ApiError> {
    let repo = VaultRepo::new(state.db.clone());
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
        .map_err(map_vault_error)?
        .into_iter()
        .map(GrepMatchOutput::from)
        .collect();
    Ok(Json(GrepResponse { results }))
}

fn map_vault_error(error: VaultRepoError) -> ApiError {
    match error {
        VaultRepoError::NotFound(message) => ApiError::not_found(message),
        VaultRepoError::InvalidInput(message) => ApiError::invalid_field(message),
        VaultRepoError::Conflict(message) => ApiError::conflict(message),
        VaultRepoError::Internal(message) => {
            tracing::error!(event = "vault.error", detail = %message);
            ApiError::internal("internal server error")
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResolveQuery {
    path: String,
}

#[derive(Debug, Deserialize)]
struct CreateNodeRequest {
    parent_node_id: Uuid,
    name: String,
}

#[derive(Debug, Deserialize)]
struct SaveDocumentRequest {
    content_md: String,
}

#[derive(Debug, Deserialize)]
struct MoveNodeRequest {
    new_parent_node_id: Uuid,
    new_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FindRequest {
    q: String,
    path: Option<String>,
    kind: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct GrepRequest {
    q: String,
    path: Option<String>,
    context: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct NodeResponseBody {
    node: NodeOutput,
}

#[derive(Debug, Serialize)]
struct ChildrenResponse {
    parent: ParentOutput,
    children: Vec<NodeOutput>,
}

impl From<Children> for ChildrenResponse {
    fn from(value: Children) -> Self {
        Self {
            parent: ParentOutput {
                id: value.parent.id,
                path: value.parent.path,
            },
            children: value.children.into_iter().map(NodeOutput::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct DocumentResponse {
    node: NodeOutput,
    document: DocumentOutput,
}

impl From<DocumentBundle> for DocumentResponse {
    fn from(value: DocumentBundle) -> Self {
        Self {
            node: NodeOutput::from(value.node),
            document: DocumentOutput::from(value.document),
        }
    }
}

#[derive(Debug, Serialize)]
struct FindResponse {
    results: Vec<NodeOutput>,
}

#[derive(Debug, Serialize)]
struct GrepResponse {
    results: Vec<GrepMatchOutput>,
}

#[derive(Debug, Serialize)]
struct ParentOutput {
    id: Uuid,
    path: String,
}

#[derive(Debug, Serialize)]
struct NodeOutput {
    id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    kind: &'static str,
    path: String,
    sort_order: i32,
    has_children: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Node> for NodeOutput {
    fn from(value: Node) -> Self {
        Self {
            id: value.id,
            parent_id: value.parent_id,
            name: value.name,
            kind: value.kind.as_str(),
            path: value.path,
            sort_order: value.sort_order,
            has_children: value.has_children,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct DocumentOutput {
    node_id: Uuid,
    content_md: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Document> for DocumentOutput {
    fn from(value: Document) -> Self {
        Self {
            node_id: value.node_id,
            content_md: value.content_md,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct GrepMatchOutput {
    node_id: Uuid,
    path: String,
    line_no: i64,
    line: String,
    before: Vec<String>,
    after: Vec<String>,
}

impl From<GrepMatch> for GrepMatchOutput {
    fn from(value: GrepMatch) -> Self {
        Self {
            node_id: value.node_id,
            path: value.path,
            line_no: value.line_no,
            line: value.line,
            before: value.before,
            after: value.after,
        }
    }
}
