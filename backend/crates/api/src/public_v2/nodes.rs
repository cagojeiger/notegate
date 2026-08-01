use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use notegate_core::validation::normalize_path;
use notegate_model::{Caller, NodeKind};
use notegate_service::files::{
    ChildrenRequest, CopyNode, CreateFolder, CreateText, DeleteNode, MoveNode, WriteTarget,
    WriteText, WriteTextBody,
};
use notegate_service::search::TreeRequest;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

use super::dto::{NodeOut, NodeSummaryOut, PageOut};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/spaces/{space_id}/paths/resolve", get(resolve_path))
        .route("/spaces/{space_id}/tree", get(tree))
        .route("/spaces/{space_id}/nodes", post(create))
        .route(
            "/spaces/{space_id}/nodes/{node_id}",
            get(get_one).delete(delete),
        )
        .route("/spaces/{space_id}/nodes/{node_id}/children", get(children))
        .route("/spaces/{space_id}/nodes/{node_id}/move", post(move_node))
        .route("/spaces/{space_id}/nodes/{node_id}/copy", post(copy_node))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ResolveQuery {
    path: String,
}

#[utoipa::path(
    get,
    path = "/api/v2/spaces/{space_id}/paths/resolve",
    tag = "nodes",
    params(
        ("space_id" = Uuid, Path, description = "Space id"),
        ("path" = String, Query, description = "Absolute path inside the space"),
    ),
    responses((status = 200, description = "Resolve a path to a node", body = NodeOut)),
    security(("api_key" = []))
)]
pub(crate) async fn resolve_path(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Query(query): Query<ResolveQuery>,
) -> Result<Json<NodeOut>, ApiError> {
    let view = state
        .files
        .resolve_path(caller.account_id(), space_id, &query.path)
        .await?;
    Ok(Json(NodeOut::from(&view)))
}

#[utoipa::path(
    get,
    path = "/api/v2/spaces/{space_id}/nodes/{node_id}",
    tag = "nodes",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses((status = 200, description = "Get node metadata and effective write-lock state", body = NodeOut)),
    security(("api_key" = []))
)]
pub(crate) async fn get_one(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<NodeOut>, ApiError> {
    let view = state
        .files
        .stat(caller.account_id(), space_id, node_id)
        .await?;
    Ok(Json(NodeOut::from(&view)))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChildrenQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ChildrenResponse {
    parent: NodeOut,
    children: Vec<NodeSummaryOut>,
    page: PageOut,
}

#[utoipa::path(
    get,
    path = "/api/v2/spaces/{space_id}/nodes/{node_id}/children",
    tag = "nodes",
    params(
        ("space_id" = Uuid, Path),
        ("node_id" = Uuid, Path),
        ("limit" = Option<i64>, Query, description = "Page size; defaults to 100 and is capped at 200"),
        ("cursor" = Option<String>, Query, description = "Opaque cursor returned by the preceding response; keep all other parameters unchanged"),
    ),
    responses((status = 200, description = "List direct children", body = ChildrenResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn children(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ChildrenQuery>,
) -> Result<Json<ChildrenResponse>, ApiError> {
    let page = state
        .files
        .children(
            caller.account_id(),
            space_id,
            node_id,
            ChildrenRequest {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let children = page
        .items
        .iter()
        .map(NodeSummaryOut::from)
        .collect::<Vec<_>>();
    Ok(Json(ChildrenResponse {
        parent: NodeOut::from(&page.parent),
        page: PageOut::new(page.limit, children.len(), page.has_more, page.next_cursor),
        children,
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct TreeQuery {
    path: Option<String>,
    depth: Option<i64>,
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TreeResponse {
    path: String,
    depth: i64,
    nodes: Vec<NodeSummaryOut>,
    page: PageOut,
}

#[utoipa::path(
    get,
    path = "/api/v2/spaces/{space_id}/tree",
    tag = "nodes",
    params(
        ("space_id" = Uuid, Path),
        ("path" = Option<String>, Query, description = "Absolute folder path; defaults to /"),
        ("depth" = Option<i64>, Query, description = "Tree depth; defaults to 2 and is capped at 7"),
        ("limit" = Option<i64>, Query, description = "Page size; defaults to 100 and is capped at 200"),
        ("cursor" = Option<String>, Query, description = "Opaque cursor returned by the preceding response; keep path and depth unchanged"),
    ),
    responses((status = 200, description = "List a bounded subtree", body = TreeResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn tree(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Query(query): Query<TreeQuery>,
) -> Result<Json<TreeResponse>, ApiError> {
    let path = normalize_path(query.path.as_deref().unwrap_or("/"))
        .map_err(|error| ApiError::invalid_field(error.to_string()))?;
    let page = state
        .search
        .tree(
            caller.account_id(),
            space_id,
            TreeRequest {
                path: Some(path.clone()),
                depth: query.depth,
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let nodes = page
        .items
        .iter()
        .map(NodeSummaryOut::from_view)
        .collect::<Vec<_>>();
    Ok(Json(TreeResponse {
        path,
        depth: page.depth,
        page: PageOut::new(page.limit, nodes.len(), page.has_more, page.next_cursor),
        nodes,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Creates a folder or a plain-text node below an existing folder.
#[schema(examples(
    json!({
        "parent_id": "11111111-1111-1111-1111-111111111111",
        "name": "notes",
        "kind": "folder"
    }),
    json!({
        "parent_id": "11111111-1111-1111-1111-111111111111",
        "name": "README.md",
        "kind": "text",
        "content": "# Project notes\n"
    })
))]
pub(crate) struct CreateNodeBody {
    /// Existing folder that will contain the new node.
    parent_id: Uuid,
    /// Name of the new node, not a path.
    #[schema(example = "README.md")]
    name: String,
    /// Node kind. Allowed values are `folder` and `text`.
    #[schema(examples("folder", "text"))]
    kind: String,
    /// Initial UTF-8 text. Allowed only when `kind=text`; omitted content creates an empty text.
    #[serde(default)]
    content: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/v2/spaces/{space_id}/nodes",
    tag = "nodes",
    params(("space_id" = Uuid, Path)),
    request_body = CreateNodeBody,
    responses((status = 201, description = "Create a folder or plain-text node", body = NodeOut)),
    security(("api_key" = []))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Json(body): Json<CreateNodeBody>,
) -> Result<(StatusCode, Json<NodeOut>), ApiError> {
    let account_id = caller.account_id();
    let view = match NodeKind::parse(&body.kind) {
        Some(NodeKind::Folder) if body.content.is_none() => {
            state
                .files
                .create_folder(
                    account_id,
                    space_id,
                    CreateFolder {
                        parent_node_id: body.parent_id,
                        name: body.name,
                    },
                )
                .await?
        }
        Some(NodeKind::Text) => match body.content {
            Some(content) => {
                state
                    .files
                    .write_text(
                        account_id,
                        space_id,
                        WriteText {
                            target: WriteTarget::Create {
                                parent_node_id: body.parent_id,
                                name: body.name,
                            },
                            body: WriteTextBody::Plain(content),
                            expected_sha256: None,
                        },
                    )
                    .await?
                    .node
            }
            None => {
                state
                    .files
                    .create_text(
                        account_id,
                        space_id,
                        CreateText {
                            parent_node_id: body.parent_id,
                            name: body.name,
                        },
                    )
                    .await?
                    .node
            }
        },
        Some(NodeKind::Folder) => {
            return Err(ApiError::invalid_field(
                "content is only valid when kind=text",
            ));
        }
        Some(NodeKind::File) => {
            return Err(ApiError::invalid_field(
                "files must be created through file-uploads",
            ));
        }
        None => {
            return Err(ApiError::invalid_field("kind must be 'folder' or 'text'"));
        }
    };
    Ok((StatusCode::CREATED, Json(NodeOut::from(&view))))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Moves a node to another folder in the same space and optionally renames it.
#[schema(example = json!({
    "new_parent_id": "22222222-2222-2222-2222-222222222222",
    "new_name": "renamed.md",
    "expected_parent_id": "11111111-1111-1111-1111-111111111111"
}))]
pub(crate) struct MoveNodeBody {
    /// Existing destination folder in the same space.
    new_parent_id: Uuid,
    /// New node name. Omit it to preserve the current name.
    #[serde(default)]
    new_name: Option<String>,
    /// Optional optimistic guard. The move fails if the current parent differs.
    #[serde(default)]
    expected_parent_id: Option<Uuid>,
}

#[utoipa::path(
    post,
    path = "/api/v2/spaces/{space_id}/nodes/{node_id}/move",
    tag = "nodes",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = MoveNodeBody,
    responses((status = 200, description = "Move or rename a node", body = NodeOut)),
    security(("api_key" = []))
)]
pub(crate) async fn move_node(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MoveNodeBody>,
) -> Result<Json<NodeOut>, ApiError> {
    let view = state
        .files
        .move_node(
            caller.account_id(),
            space_id,
            MoveNode {
                node_id,
                new_parent_node_id: body.new_parent_id,
                new_name: body.new_name,
                expected_parent_id: body.expected_parent_id,
            },
        )
        .await?;
    Ok(Json(NodeOut::from(&view)))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
/// Copies a node to another folder in the same space.
#[schema(example = json!({
    "new_parent_id": "22222222-2222-2222-2222-222222222222",
    "new_name": "README-copy.md",
    "recursive": false
}))]
pub(crate) struct CopyNodeBody {
    /// Existing destination folder in the same space.
    new_parent_id: Uuid,
    /// Name of the copied root node.
    new_name: String,
    /// Must be true to copy a non-empty folder subtree.
    #[schema(default = false)]
    #[serde(default)]
    recursive: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CopyNodeResponse {
    node: NodeOut,
    counts: CopyCountsOut,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CopyCountsOut {
    nodes: usize,
    texts: usize,
    files: usize,
}

#[utoipa::path(
    post,
    path = "/api/v2/spaces/{space_id}/nodes/{node_id}/copy",
    tag = "nodes",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = CopyNodeBody,
    responses((status = 201, description = "Copy a node inside the same space", body = CopyNodeResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn copy_node(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CopyNodeBody>,
) -> Result<(StatusCode, Json<CopyNodeResponse>), ApiError> {
    let result = state
        .files
        .copy_node(
            caller.account_id(),
            space_id,
            CopyNode {
                node_id,
                new_parent_node_id: body.new_parent_id,
                new_name: body.new_name,
                recursive: body.recursive,
            },
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CopyNodeResponse {
            node: NodeOut::from(&result.node),
            counts: CopyCountsOut {
                nodes: result.counts.nodes,
                texts: result.counts.texts,
                files: result.counts.files,
            },
        }),
    ))
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DeleteQuery {
    #[serde(default)]
    recursive: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DeleteNodeResponse {
    node_id: Uuid,
    path: String,
    purge_after: DateTime<Utc>,
}

#[utoipa::path(
    delete,
    path = "/api/v2/spaces/{space_id}/nodes/{node_id}",
    tag = "nodes",
    params(
        ("space_id" = Uuid, Path),
        ("node_id" = Uuid, Path),
        ("recursive" = Option<bool>, Query, description = "Required for non-empty folders"),
    ),
    responses((status = 200, description = "Soft-delete a node", body = DeleteNodeResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn delete(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<DeleteNodeResponse>, ApiError> {
    let result = state
        .files
        .delete_node(
            caller.account_id(),
            space_id,
            DeleteNode {
                node_id,
                recursive: query.recursive,
            },
        )
        .await?;
    Ok(Json(DeleteNodeResponse {
        node_id: result.node_id,
        path: result.path,
        purge_after: result.purge_after,
    }))
}
