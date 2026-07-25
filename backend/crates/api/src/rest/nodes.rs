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
use crate::rest::dto::{
    FileChangeDeltaOut, FileChangeEventListResponse, FileChangeEventOut, FileChangeSyncResponse,
    NodeOut, NodeRef, NodeSummaryOut, attribution_ids, parse_kind,
};
use crate::state::AppState;

use notegate_service::files::{
    BatchChildrenRequest, BatchChildrenResult, ChildrenRequest, CreateFolder, CreateText,
    DeleteNode, ListFileChangeEvents, ListNodesRequest, MoveNode, NodeListSort, SyncFileChanges,
    WriteTarget, WriteText, WriteTextBody,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/{space_id}/paths/resolve", get(resolve_path))
        .route("/v1/spaces/{space_id}/nodes", get(list).post(create))
        .route(
            "/v1/spaces/{space_id}/file-change-events",
            get(list_file_change_events),
        )
        .route(
            "/v1/spaces/{space_id}/file-change-sync",
            get(sync_file_changes),
        )
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
            "/v1/spaces/{space_id}/nodes:batchListChildren",
            post(batch_children),
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

#[derive(Debug, Deserialize)]
pub(crate) struct ListFileChangeEventsQuery {
    node_id: Option<Uuid>,
    limit: Option<i64>,
    cursor: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/file-change-events",
    tag = "events",
    params(
        ("space_id" = Uuid, Path),
        ("node_id" = Option<Uuid>, Query, description = "Optional node id filter"),
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List file change event history in a space", body = FileChangeEventListResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn list_file_change_events(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Query(query): Query<ListFileChangeEventsQuery>,
) -> Result<Json<FileChangeEventListResponse>, ApiError> {
    let page = state
        .files
        .list_file_change_events(
            caller.account_id(),
            space_id,
            ListFileChangeEvents {
                node_id: query.node_id,
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let actor_ids = page
        .items
        .iter()
        .filter_map(|event| event.actor_account_id)
        .collect::<Vec<_>>();
    let refs = state.accounts.find_account_refs(&actor_ids).await?;
    let events = page
        .items
        .iter()
        .map(|event| FileChangeEventOut::from_event(event, &refs))
        .collect();

    Ok(Json(FileChangeEventListResponse {
        events,
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct SyncFileChangesQuery {
    after_id: Option<i64>,
    limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/file-change-sync",
    tag = "events",
    params(
        ("space_id" = Uuid, Path),
        ("after_id" = Option<i64>, Query, description = "Last applied event id; omit to establish a baseline"),
        ("limit" = Option<i64>, Query, description = "Page size"),
    ),
    responses((status = 200, description = "Read file changes after a sync token", body = FileChangeSyncResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn sync_file_changes(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Query(query): Query<SyncFileChangesQuery>,
) -> Result<Json<FileChangeSyncResponse>, ApiError> {
    let page = state
        .files
        .sync_file_changes(
            caller.account_id(),
            space_id,
            SyncFileChanges {
                after_id: query.after_id,
                limit: query.limit,
            },
        )
        .await?;
    Ok(Json(FileChangeSyncResponse {
        changes: page
            .items
            .iter()
            .map(FileChangeDeltaOut::from_event)
            .collect(),
        next_after_id: page.next_after_id,
        has_more: page.has_more,
        resync_required: page.resync_required,
    }))
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
    view: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum NodeCollectionOut {
    Full(Box<NodeOut>),
    Summary(NodeSummaryOut),
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct NodesListResponse {
    nodes: Vec<NodeCollectionOut>,
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
        ("view" = Option<String>, Query, description = "summary for compact collection nodes; omitted returns full nodes"),
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
    let sort = NodeListSort::parse(query.sort.as_deref().unwrap_or("updated_at_desc"))
        .ok_or_else(|| ApiError::invalid_field("sort must be 'updated_at_desc' or 'name_asc'"))?;
    let summary = parse_collection_view(query.view.as_deref())?;
    let request = ListNodesRequest {
        kind,
        sort,
        limit: query.limit,
        cursor: query.cursor,
    };
    if summary {
        let page = state
            .files
            .list_node_summaries(caller.account_id(), space_id, request)
            .await?;
        let nodes = page
            .items
            .iter()
            .map(NodeSummaryOut::from)
            .map(NodeCollectionOut::Summary)
            .collect();
        return Ok(Json(NodesListResponse {
            nodes,
            page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
        }));
    }

    let page = state
        .files
        .list_nodes(caller.account_id(), space_id, request)
        .await?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids(page.items.iter()))
        .await?;
    let nodes = page
        .items
        .iter()
        .map(|view| Box::new(NodeOut::from_view(view, &refs)))
        .map(NodeCollectionOut::Full)
        .collect();
    Ok(Json(NodesListResponse {
        nodes,
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChildrenQuery {
    view: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ChildrenResponse {
    parent: NodeRef,
    children: Vec<NodeCollectionOut>,
    page: Page,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct BatchChildrenRequestBody {
    parent_ids: Vec<Uuid>,
    limit: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BatchChildrenStatus {
    Ready,
    NotFound,
    NotFolder,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct BatchChildrenItem {
    parent_id: Uuid,
    status: BatchChildrenStatus,
    parent: Option<NodeRef>,
    children: Vec<NodeSummaryOut>,
    page: Option<Page>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct BatchChildrenResponse {
    results: Vec<BatchChildrenItem>,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/nodes/{node_id}/children",
    tag = "nodes",
    params(
        ("space_id" = Uuid, Path),
        ("node_id" = Uuid, Path),
        ("view" = Option<String>, Query, description = "summary for compact collection nodes; omitted returns full nodes"),
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
    let summary = parse_collection_view(query.view.as_deref())?;
    let request = ChildrenRequest {
        limit: query.limit,
        cursor: query.cursor,
    };
    if summary {
        let page = state
            .files
            .children(caller.account_id(), space_id, node_id, request)
            .await?;
        return Ok(Json(children_summary_response(&page)));
    }

    let page = state
        .files
        .canonical_children(caller.account_id(), space_id, node_id, request)
        .await?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids(page.items.iter()))
        .await?;
    Ok(Json(ChildrenResponse {
        parent: NodeRef::from(&page.parent),
        children: page
            .items
            .iter()
            .map(|view| Box::new(NodeOut::from_view(view, &refs)))
            .map(NodeCollectionOut::Full)
            .collect(),
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/nodes:batchListChildren",
    tag = "nodes",
    params(("space_id" = Uuid, Path)),
    request_body = BatchChildrenRequestBody,
    responses(
        (status = 200, description = "List the first children page for each requested parent", body = BatchChildrenResponse),
        (status = 400, description = "Invalid, duplicate, or excessive parent input")
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn batch_children(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    Json(request): Json<BatchChildrenRequestBody>,
) -> Result<Json<BatchChildrenResponse>, ApiError> {
    let results = state
        .files
        .batch_children(
            caller.account_id(),
            space_id,
            BatchChildrenRequest {
                parent_node_ids: request.parent_ids,
                limit: request.limit,
            },
        )
        .await?;
    let results = results
        .iter()
        .map(|result| match result {
            BatchChildrenResult::Ready(page) => BatchChildrenItem {
                parent_id: page.parent.node.id,
                status: BatchChildrenStatus::Ready,
                parent: Some(NodeRef::from(&page.parent)),
                children: page.items.iter().map(NodeSummaryOut::from).collect(),
                page: Some(Page::from_items(
                    page.limit,
                    &page.items,
                    page.has_more,
                    page.next_cursor.clone(),
                )),
            },
            BatchChildrenResult::NotFound { parent_node_id } => BatchChildrenItem {
                parent_id: *parent_node_id,
                status: BatchChildrenStatus::NotFound,
                parent: None,
                children: Vec::new(),
                page: None,
            },
            BatchChildrenResult::NotFolder { parent_node_id } => BatchChildrenItem {
                parent_id: *parent_node_id,
                status: BatchChildrenStatus::NotFolder,
                parent: None,
                children: Vec::new(),
                page: None,
            },
        })
        .collect();
    Ok(Json(BatchChildrenResponse { results }))
}

fn children_summary_response(page: &notegate_service::files::ChildrenPage) -> ChildrenResponse {
    ChildrenResponse {
        parent: NodeRef::from(&page.parent),
        children: page
            .items
            .iter()
            .map(NodeSummaryOut::from)
            .map(NodeCollectionOut::Summary)
            .collect(),
        page: Page::from_items(
            page.limit,
            &page.items,
            page.has_more,
            page.next_cursor.clone(),
        ),
    }
}

fn parse_collection_view(view: Option<&str>) -> Result<bool, ApiError> {
    match view {
        None => Ok(false),
        Some("summary") => Ok(true),
        Some(_) => Err(ApiError::invalid_field(
            "view must be 'summary' when provided",
        )),
    }
}

#[cfg(test)]
mod collection_response_tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    #[test]
    fn maximum_batch_summary_response_stays_below_two_mib() -> Result<(), Box<dyn std::error::Error>>
    {
        let updated_at = Utc
            .with_ymd_and_hms(2026, 7, 25, 0, 0, 0)
            .single()
            .ok_or("invalid test timestamp")?;
        let mut results = Vec::new();
        for parent_index in 0..16_u128 {
            let parent_id = Uuid::from_u128(10 + parent_index);
            let children = (0..100_u128)
                .map(|child_index| NodeSummaryOut {
                    id: Uuid::from_u128(1_000 + parent_index * 100 + child_index),
                    parent_id: Some(parent_id),
                    name: "n".repeat(128),
                    kind: "file".to_owned(),
                    path: format!("/{}", "p".repeat(902)),
                    has_children: false,
                    byte_len: Some(i64::MAX),
                    line_count: None,
                    preview_available: Some(true),
                    updated_at,
                })
                .collect();
            results.push(BatchChildrenItem {
                parent_id,
                status: BatchChildrenStatus::Ready,
                parent: Some(NodeRef {
                    id: parent_id,
                    path: format!("/{}", "p".repeat(902)),
                    kind: "folder".to_owned(),
                }),
                children,
                page: Some(Page::new(100, 100, true, Some("c".repeat(256)))),
            });
        }
        let response = BatchChildrenResponse { results };
        let bytes = serde_json::to_vec(&response)?;

        assert!(
            bytes.len() < 2 * 1024 * 1024,
            "maximum compact batch was {} bytes",
            bytes.len()
        );
        assert!(
            !bytes
                .windows(b"metadata".len())
                .any(|window| window == b"metadata")
        );
        assert!(
            !bytes
                .windows(b"space_id".len())
                .any(|window| window == b"space_id")
        );
        Ok(())
    }

    #[test]
    fn collection_summary_view_is_explicit_and_bounded() {
        assert_eq!(parse_collection_view(None).ok(), Some(false));
        assert_eq!(parse_collection_view(Some("summary")).ok(), Some(true));
        assert!(parse_collection_view(Some("full")).is_err());
    }
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

#[cfg(test)]
#[path = "nodes_event_tests/mod.rs"]
mod event_tests;
