//! Nodes category: tree metadata under `/api/v1/spaces/{space_id}`.
//!
//! `GET /paths/resolve?path=`, `GET /nodes`, `GET /nodes/{id}`,
//! `GET /nodes/{id}/children` (paginated), `GET /nodes/{id}/reveal`,
//! `POST /nodes` (create folder/text), `PATCH /nodes/{id}`
//! (rename / reorder), `GET`/`PUT`/`PATCH /nodes/{id}/metadata`,
//! `POST /nodes/{id}/move`, and `DELETE /nodes/{id}`.
//! All handlers delegate to the files service,
//! which owns authorization (no live permission ⇒ 404, insufficient permission ⇒ 403) and
//! validation.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use notegate_model::{Caller, NodeKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::page::Page;
use crate::rest::dto::{NodeOut, NodeRef, attribution_ids, parse_kind};
use crate::state::AppState;

use notegate_service::files::{
    ChildrenRequest, CreateFolder, CreateText, DeleteNode, ListNodesRequest, MoveNode,
    NodeListSort, WriteTarget, WriteText, WriteTextBody,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/{space_id}/paths/resolve", get(resolve_path))
        .route("/v1/spaces/{space_id}/nodes", get(list).post(create))
        .route(
            "/v1/spaces/{space_id}/nodes/{node_id}",
            get(get_node).patch(update).delete(delete),
        )
        .route("/v1/spaces/{space_id}/nodes/{node_id}/reveal", get(reveal))
        .route(
            "/v1/spaces/{space_id}/nodes/{node_id}/children",
            get(children),
        )
        .route(
            "/v1/spaces/{space_id}/nodes/{node_id}/metadata",
            get(get_metadata)
                .put(replace_metadata)
                .patch(patch_metadata),
        )
        .route(
            "/v1/spaces/{space_id}/nodes/{node_id}/move",
            post(move_node),
        )
}

#[derive(Debug, Deserialize)]
pub(crate) struct ResolveQuery {
    path: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/paths/resolve",
    tag = "nodes",
    params(
        ("space_id" = Uuid, Path, description = "Space id"),
        ("path" = String, Query, description = "Absolute path inside the space"),
    ),
    responses((status = 200, description = "Resolve a path to a node", body = NodeOut)),
    security(("bearer_auth" = []))
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
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view]))
        .await?;
    Ok(Json(NodeOut::from_view(&view, &refs)))
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}",
    tag = "nodes",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses((status = 200, description = "Get node", body = NodeOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn get_node(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<NodeOut>, ApiError> {
    let view = state
        .files
        .stat(caller.account_id(), space_id, node_id)
        .await?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view]))
        .await?;
    Ok(Json(NodeOut::from_view(&view, &refs)))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListNodesQuery {
    kind: Option<String>,
    sort: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct NodesListResponse {
    nodes: Vec<NodeOut>,
    page: Page,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/nodes",
    tag = "nodes",
    params(
        ("space_id" = Uuid, Path),
        ("kind" = Option<String>, Query, description = "Optional kind filter: folder, text, or file"),
        ("sort" = Option<String>, Query, description = "updated_at_desc (default) or name_asc"),
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List nodes in a space", body = NodesListResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Query(query): Query<ListNodesQuery>,
) -> Result<Json<NodesListResponse>, ApiError> {
    let kind = query.kind.as_deref().map(parse_kind).transpose()?;
    let sort = match query.sort.as_deref().unwrap_or("updated_at_desc") {
        value => NodeListSort::parse(value).ok_or_else(|| {
            ApiError::invalid_field("sort must be 'updated_at_desc' or 'name_asc'")
        })?,
    };
    let page = state
        .files
        .list_nodes(
            caller.account_id(),
            space_id,
            ListNodesRequest {
                kind,
                sort,
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids(page.items.iter()))
        .await?;
    let nodes = page
        .items
        .iter()
        .map(|view| NodeOut::from_view(view, &refs))
        .collect();

    Ok(Json(NodesListResponse {
        nodes,
        page: Page::new(
            page.limit,
            page.items.len(),
            page.has_more,
            page.next_cursor,
        ),
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChildrenQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ChildrenResponse {
    parent: NodeRef,
    children: Vec<NodeOut>,
    page: Page,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/children",
    tag = "nodes",
    params(
        ("space_id" = Uuid, Path),
        ("node_id" = Uuid, Path),
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List children", body = ChildrenResponse)),
    security(("bearer_auth" = []))
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

    let mut all = vec![&page.parent];
    all.extend(page.items.iter());
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids(all))
        .await?;

    let children = page
        .items
        .iter()
        .map(|view| NodeOut::from_view(view, &refs))
        .collect();

    Ok(Json(ChildrenResponse {
        parent: NodeRef::from(&page.parent),
        children,
        page: Page::new(
            page.limit,
            page.items.len(),
            page.has_more,
            page.next_cursor,
        ),
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct RevealResponse {
    ancestors: Vec<NodeOut>,
    target: NodeOut,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/reveal",
    tag = "nodes",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses((status = 200, description = "Reveal a node in the tree", body = RevealResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn reveal(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<RevealResponse>, ApiError> {
    let reveal = state
        .files
        .reveal_node(caller.account_id(), space_id, node_id)
        .await?;

    let mut all: Vec<&notegate_service::files::NodeView> = reveal.ancestors.iter().collect();
    all.push(&reveal.target);
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids(all))
        .await?;
    let ancestors = reveal
        .ancestors
        .iter()
        .map(|view| NodeOut::from_view(view, &refs))
        .collect();

    Ok(Json(RevealResponse {
        ancestors,
        target: NodeOut::from_view(&reveal.target, &refs),
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct CreateNodeBody {
    parent_id: Uuid,
    kind: String,
    name: String,
    #[serde(default)]
    content: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/nodes",
    tag = "nodes",
    params(("space_id" = Uuid, Path)),
    request_body = CreateNodeBody,
    responses((status = 201, description = "Create node", body = NodeOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Json(body): Json<CreateNodeBody>,
) -> Result<(StatusCode, Json<NodeOut>), ApiError> {
    let kind = parse_kind(&body.kind)?;
    let account_id = caller.account_id();

    let view = match kind {
        NodeKind::Folder => {
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
        NodeKind::Text => {
            // With initial content, create-and-write; otherwise create empty.
            match body.content {
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
            }
        }
        NodeKind::File => {
            return Err(ApiError::invalid_field(
                "file node creation is not supported by this endpoint",
            ));
        }
    };

    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view]))
        .await?;
    Ok((StatusCode::CREATED, Json(NodeOut::from_view(&view, &refs))))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct UpdateNodeBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    sort_order: Option<i32>,
}

#[utoipa::path(
    patch,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}",
    tag = "nodes",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = UpdateNodeBody,
    responses((status = 200, description = "Rename or reorder node", body = NodeOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn update(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateNodeBody>,
) -> Result<Json<NodeOut>, ApiError> {
    let view = state
        .files
        .update_node(
            caller.account_id(),
            space_id,
            node_id,
            body.name,
            body.sort_order,
        )
        .await?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view]))
        .await?;
    Ok(Json(NodeOut::from_view(&view, &refs)))
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct MetadataBody {
    metadata: Value,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct MetadataPatchBody {
    patch: Value,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/metadata",
    tag = "nodes",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses((status = 200, description = "Get node metadata", body = MetadataBody)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn get_metadata(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<MetadataBody>, ApiError> {
    let metadata = state
        .files
        .read_metadata(caller.account_id(), space_id, node_id)
        .await?;
    Ok(Json(MetadataBody { metadata }))
}

#[utoipa::path(
    put,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/metadata",
    tag = "nodes",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = MetadataBody,
    responses((status = 200, description = "Replace node metadata", body = NodeOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn replace_metadata(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MetadataBody>,
) -> Result<Json<NodeOut>, ApiError> {
    let view = state
        .files
        .replace_metadata(caller.account_id(), space_id, node_id, body.metadata)
        .await?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view]))
        .await?;
    Ok(Json(NodeOut::from_view(&view, &refs)))
}

#[utoipa::path(
    patch,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/metadata",
    tag = "nodes",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = MetadataPatchBody,
    responses((status = 200, description = "Patch node metadata", body = NodeOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn patch_metadata(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MetadataPatchBody>,
) -> Result<Json<NodeOut>, ApiError> {
    let view = state
        .files
        .patch_metadata(caller.account_id(), space_id, node_id, body.patch)
        .await?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view]))
        .await?;
    Ok(Json(NodeOut::from_view(&view, &refs)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct MoveNodeBody {
    new_parent_id: Uuid,
    #[serde(default)]
    new_name: Option<String>,
    /// Optional optimistic guard. When present and it does not match the node's
    /// current parent, the move is rejected as a conflict.
    #[serde(default)]
    expected_parent_id: Option<Uuid>,
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/move",
    tag = "nodes",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = MoveNodeBody,
    responses((status = 200, description = "Move node", body = NodeOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn move_node(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MoveNodeBody>,
) -> Result<Json<NodeOut>, ApiError> {
    let account_id = caller.account_id();

    let view = state
        .files
        .move_node(
            account_id,
            space_id,
            MoveNode {
                node_id,
                new_parent_node_id: body.new_parent_id,
                new_name: body.new_name,
                expected_parent_id: body.expected_parent_id,
            },
        )
        .await?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view]))
        .await?;
    Ok(Json(NodeOut::from_view(&view, &refs)))
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeleteQuery {
    #[serde(default)]
    recursive: bool,
}

#[utoipa::path(
    delete,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}",
    tag = "nodes",
    params(
        ("space_id" = Uuid, Path),
        ("node_id" = Uuid, Path),
        ("recursive" = Option<bool>, Query, description = "Required to delete folders"),
    ),
    responses((status = 204, description = "Delete node")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn delete(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<DeleteQuery>,
) -> Result<StatusCode, ApiError> {
    state
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
    Ok(StatusCode::NO_CONTENT)
}
