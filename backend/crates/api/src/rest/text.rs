use axum::extract::{Extension, Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::rest::dto::{AccountRef, NodeOut, NodeRef, attribution_ids};
use crate::state::AppState;

use notegate_service::files::{
    FileMutationContext, NodeView, ReadResult, ReadText, ReadTextBody, TextWriteResult,
    UpdateTextEncryption, WriteTarget, WriteText, WriteTextBody,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/spaces/{space_id}/text/{node_id}",
            get(read).put(replace),
        )
        .route(
            "/v1/spaces/{space_id}/text/{node_id}/encryption",
            axum::routing::put(update_encryption),
        )
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReadQuery {
    #[serde(default)]
    start_line: Option<i64>,
    #[serde(default)]
    max_lines: Option<i64>,
    #[serde(default)]
    max_bytes: Option<usize>,
    #[serde(default)]
    if_none_match_sha256: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ReadResponse {
    node: NodeRef,
    text: ReadTextOut,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum ReadTextOut {
    Content(ReadContentOut),
    Encrypted(ReadEncryptedOut),
    Unchanged(ReadUnchangedOut),
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ReadContentOut {
    node_id: Uuid,
    storage_format: String,
    content: String,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
    start_line: i64,
    end_line: i64,
    returned_lines: i64,
    truncated: bool,
    next_start_line: Option<i64>,
    updated_by: AccountRef,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ReadEncryptedOut {
    node_id: Uuid,
    storage_format: String,
    encrypted_payload: Value,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
    updated_by: AccountRef,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ReadUnchangedOut {
    node_id: Uuid,
    storage_format: String,
    unchanged: bool,
    content_returned: bool,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
    updated_by: AccountRef,
    updated_at: DateTime<Utc>,
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/text/{node_id}",
    tag = "text",
    params(
        ("space_id" = Uuid, Path),
        ("node_id" = Uuid, Path),
        ("start_line" = Option<i64>, Query, description = "1-based first line to return"),
        ("max_lines" = Option<i64>, Query, description = "Maximum lines to return"),
        ("max_bytes" = Option<usize>, Query, description = "Maximum bytes to return"),
        ("if_none_match_sha256" = Option<String>, Query, description = "Conditional read content hash"),
    ),
    responses((status = 200, description = "Read text", body = ReadResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn read(
    State(state): State<AppState>,
    Extension(caller): Extension<notegate_model::Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ReadQuery>,
) -> Result<Json<ReadResponse>, ApiError> {
    let result = state
        .files
        .read_text(
            caller.account_id(),
            space_id,
            ReadText {
                node_id,
                start_line: query.start_line,
                max_lines: query.max_lines,
                max_bytes: query.max_bytes,
                if_none_match_sha256: query.if_none_match_sha256,
            },
        )
        .await?;

    let updated_by = self_updated_by(&state, &result.node).await?;
    let text = read_text_out(&result, updated_by);
    Ok(Json(ReadResponse {
        node: NodeRef::from(&result.node),
        text,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct ReplaceBody {
    #[serde(default = "default_storage_format")]
    storage_format: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    encrypted_payload: Option<Value>,
    #[serde(default)]
    expected_sha256: Option<String>,
    /// Optional editing session or AI operation ID; missing means an independent save.
    #[serde(default)]
    edit_session_id: Option<Uuid>,
}

fn default_storage_format() -> String {
    "plain".to_owned()
}

#[utoipa::path(
    put,
    path = "/api/v1/spaces/{space_id}/text/{node_id}",
    tag = "text",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = ReplaceBody,
    responses((status = 200, description = "Replace text", body = TextResponse)),
    security(("browser_session" = []))
)]
pub(crate) async fn replace(
    State(state): State<AppState>,
    Extension(caller): Extension<notegate_model::Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ReplaceBody>,
) -> Result<Json<TextResponse>, ApiError> {
    let view = state
        .files
        .clone()
        .with_mutation_context(FileMutationContext {
            edit_session_id: body.edit_session_id,
            ..FileMutationContext::for_channel(caller.channel)
        })
        .write_text(
            caller.account_id(),
            space_id,
            WriteText {
                target: WriteTarget::Existing { node_id },
                body: write_body(body.storage_format, body.content, body.encrypted_payload)?,
                expected_sha256: body.expected_sha256,
            },
        )
        .await?;
    let updated_by = self_updated_by(&state, &view.node).await?;
    Ok(Json(text_response(&view, updated_by)))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateEncryptionBody {
    enabled: bool,
}

#[utoipa::path(
    put,
    path = "/api/v1/spaces/{space_id}/text/{node_id}/encryption",
    tag = "text",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    request_body = UpdateEncryptionBody,
    responses((status = 200, description = "Update text encryption", body = NodeOut)),
    security(("browser_session" = []))
)]
pub(crate) async fn update_encryption(
    State(state): State<AppState>,
    Extension(caller): Extension<notegate_model::Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateEncryptionBody>,
) -> Result<Json<NodeOut>, ApiError> {
    let view = state
        .files
        .update_text_encryption(
            caller.account.kind,
            caller.account_id(),
            space_id,
            UpdateTextEncryption {
                node_id,
                enabled: body.enabled,
            },
        )
        .await?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view]))
        .await?;
    Ok(Json(NodeOut::from_view(&view, &refs)))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TextResponse {
    node: NodeRef,
    text: TextMetaOut,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TextMetaOut {
    node_id: Uuid,
    storage_format: String,
    content_sha256: String,
    byte_len: i64,
    line_count: i32,
    updated_by: AccountRef,
    updated_at: DateTime<Utc>,
}

/// Resolve the `updated_by` account ref for a node view.
async fn self_updated_by(state: &AppState, view: &NodeView) -> Result<AccountRef, ApiError> {
    let refs = state
        .accounts
        .find_account_refs(&[view.node.updated_by_account_id])
        .await?;
    Ok(AccountRef::resolve(view.node.updated_by_account_id, &refs))
}

/// Build the `text` object for a read response, covering both the full-content
/// slice and the `unchanged` (conditional-read hit) shapes.
fn read_text_out(result: &ReadResult, updated_by: AccountRef) -> ReadTextOut {
    match &result.body {
        ReadTextBody::Unchanged => ReadTextOut::Unchanged(ReadUnchangedOut {
            node_id: result.node.node.id,
            storage_format: result.storage_format.as_str().to_owned(),
            unchanged: true,
            content_returned: false,
            content_sha256: result.content_sha256.clone(),
            byte_len: result.byte_len,
            line_count: result.line_count,
            updated_by,
            updated_at: result.node.node.updated_at,
        }),
        ReadTextBody::Encrypted(payload) => ReadTextOut::Encrypted(ReadEncryptedOut {
            node_id: result.node.node.id,
            storage_format: result.storage_format.as_str().to_owned(),
            encrypted_payload: payload.clone(),
            content_sha256: result.content_sha256.clone(),
            byte_len: result.byte_len,
            line_count: result.line_count,
            updated_by,
            updated_at: result.node.node.updated_at,
        }),
        ReadTextBody::Content(content) => ReadTextOut::Content(ReadContentOut {
            node_id: result.node.node.id,
            storage_format: result.storage_format.as_str().to_owned(),
            content: content.content.clone(),
            content_sha256: result.content_sha256.clone(),
            byte_len: result.byte_len,
            line_count: result.line_count,
            start_line: content.start_line,
            end_line: content.end_line,
            returned_lines: content.returned_lines,
            truncated: content.truncated,
            next_start_line: content.next_start_line,
            updated_by,
            updated_at: result.node.node.updated_at,
        }),
    }
}

fn write_body(
    storage_format: String,
    content: Option<String>,
    encrypted_payload: Option<Value>,
) -> Result<WriteTextBody, ApiError> {
    match storage_format.as_str() {
        "plain" => Ok(WriteTextBody::Plain(content.ok_or_else(|| {
            ApiError::invalid_field("content is required when storage_format=plain")
        })?)),
        "encrypted" => Ok(WriteTextBody::Encrypted(encrypted_payload.ok_or_else(
            || {
                ApiError::invalid_field(
                    "encrypted_payload is required when storage_format=encrypted",
                )
            },
        )?)),
        _ => Err(ApiError::invalid_field(
            "storage_format must be 'plain' or 'encrypted'",
        )),
    }
}

fn text_response(view: &TextWriteResult, updated_by: AccountRef) -> TextResponse {
    TextResponse {
        node: NodeRef::from(&view.node),
        text: TextMetaOut {
            node_id: view.text.node_id,
            storage_format: view.text.storage_format.as_str().to_owned(),
            content_sha256: view.text.content_sha256.clone(),
            byte_len: view.text.byte_len,
            line_count: view.text.line_count,
            updated_by,
            updated_at: view.text.updated_at,
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]

    use axum::http::StatusCode;
    use notegate_db::test_support::TestDb;
    use serde_json::json;
    use uuid::Uuid;

    use crate::rest::test_support::{caller_and_space, get_json, json_request, rest_app, state};

    #[tokio::test]
    async fn conditional_read_preserves_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = state(&db);
        let (caller, space, root) = caller_and_space(&state).await?;
        let app = rest_app(state, caller.clone());
        let (status, created) = json_request(
            app.clone(),
            "POST",
            format!("/v1/spaces/{space}/nodes"),
            json!({
                "parent_id": root,
                "kind": "text",
                "name": "before.md",
                "content": "unchanged content\n"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        let node: Uuid = serde_json::from_value(created["id"].clone())?;
        let uri = format!("/v1/spaces/{space}/text/{node}");
        let (status, rejected) = json_request(
            app.clone(),
            "PATCH",
            uri.clone(),
            json!({"edits": [{"old_text": "unchanged", "new_text": "changed"}]}),
        )
        .await?;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{rejected}");
        let (status, before) = get_json(app.clone(), uri.clone()).await?;
        assert_eq!(status, StatusCode::OK, "{before}");
        let hash = before["text"]["content_sha256"]
            .as_str()
            .ok_or("full read omitted its content hash")?;

        let (status, renamed) = json_request(
            app.clone(),
            "PATCH",
            format!("/v1/spaces/{space}/nodes/{node}"),
            json!({"name": "after.md"}),
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{renamed}");
        let (status, full) = get_json(app.clone(), uri.clone()).await?;
        assert_eq!(status, StatusCode::OK, "{full}");
        assert_eq!(full["text"]["content_sha256"], hash);
        assert_eq!(full["node"]["path"], "/after.md");
        assert_eq!(full["text"]["updated_by"]["id"], json!(caller.account_id()));
        assert!(full["text"]["updated_at"].is_string());
        assert_eq!(full["text"]["updated_at"], renamed["updated_at"]);

        let (status, unchanged) =
            get_json(app.clone(), format!("{uri}?if_none_match_sha256={hash}")).await?;
        assert_eq!(status, StatusCode::OK, "{unchanged}");
        assert_eq!(unchanged["node"], full["node"]);
        assert_eq!(unchanged["text"]["unchanged"], true);
        assert_eq!(unchanged["text"]["content_returned"], false);
        assert!(unchanged["text"].get("content").is_none());
        assert!(unchanged["text"].get("encrypted_payload").is_none());
        for field in [
            "node_id",
            "storage_format",
            "content_sha256",
            "byte_len",
            "line_count",
            "updated_by",
            "updated_at",
        ] {
            assert_eq!(unchanged["text"][field], full["text"][field], "{field}");
        }

        let (status, stale) = get_json(
            app,
            format!("{uri}?if_none_match_sha256={}", "0".repeat(64)),
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{stale}");
        assert_eq!(stale, full);
        assert_eq!(stale["text"]["content"], "unchanged content\n");
        db.cleanup().await;
        Ok(())
    }
}
