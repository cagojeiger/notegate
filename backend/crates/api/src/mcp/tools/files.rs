//! Internal file operation handlers used by the unified MCP tools (`docs/spec/mcp/tools.md`).

use axum::http::request::Parts;
use notegate_model::TextStorageFormat;
use notegate_service::ServiceError;
use notegate_service::files::{
    AppendText, ChildrenRequest, CopyNode, CreateFolder, DeleteNode, Edit as ServiceEdit, EditText,
    LineEdit, MoveNode, NodeView, PatchMode, PatchText, ReadText, ReadTextBody, WriteTarget,
    WriteText, WriteTextBody,
};
use notegate_service::search::TreeRequest;
use rmcp::{ErrorData, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::resolve::{
    caller, invalid_input_error, node_summary, resolve_target, service_error, split_parent_name,
};
use super::support::page_json;
use crate::state::AppState;

/// One exact replacement.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchEdit {
    /// The exact text to find (must match exactly once).
    pub old_text: String,
    /// The replacement text (must differ from `old_text`).
    pub new_text: String,
    /// Replacement mode: `unique` (default), `first`, or `all`.
    #[serde(default)]
    pub mode: Option<String>,
    /// Optional guard for the number of matches in the current text.
    #[serde(default)]
    pub expected_count: Option<usize>,
}

/// One line-based edit.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineEditInput {
    /// `insert_before_line`, `insert_after_line`, `replace_lines`, or `delete_lines`.
    pub op: String,
    /// 1-based line for insert operations.
    #[serde(default)]
    pub line: Option<i64>,
    /// 1-based first line for replace/delete operations.
    #[serde(default)]
    pub start_line: Option<i64>,
    /// 1-based last line for replace/delete operations.
    #[serde(default)]
    pub end_line: Option<i64>,
    /// Content to insert or replace with.
    #[serde(default)]
    pub content: Option<String>,
}

pub async fn list(
    state: &AppState,
    parts: &Parts,
    target: String,
    depth: Option<i64>,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();
    let depth = depth.unwrap_or(1);

    if depth < 1 {
        return Err(invalid_input_error("depth must be at least 1"));
    }

    if depth == 1 {
        let folder = state
            .files
            .resolve_path(account_id, space_id, &path)
            .await
            .map_err(service_error)?;

        let page = state
            .files
            .children(
                account_id,
                space_id,
                folder.node.id,
                ChildrenRequest { limit, cursor },
            )
            .await
            .map_err(service_error)?;

        let items: Vec<Value> = page.items.iter().map(node_summary).collect();
        let returned = items.len();

        return Ok(Json(json!({
            "space": resolved.name(),
            "path": page.parent.path,
            "depth": 1,
            "items": items,
            "page": page_json(
                page.limit,
                returned,
                page.has_more,
                page.next_cursor.as_deref(),
            ),
        })));
    }

    let page = state
        .search
        .tree(
            account_id,
            space_id,
            TreeRequest {
                path: Some(path.clone()),
                depth: Some(depth),
                limit,
                cursor,
            },
        )
        .await
        .map_err(service_error)?;

    let items: Vec<Value> = page.items.iter().map(node_summary).collect();
    let returned = items.len();

    Ok(Json(json!({
        "space": resolved.name(),
        "path": path,
        "depth": page.depth,
        "items": items,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    })))
}

pub async fn stat(
    state: &AppState,
    parts: &Parts,
    target: String,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;

    let view = state
        .files
        .resolve_path(caller.account_id(), resolved.space_id(), &path)
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "node": node_summary(&view),
    })))
}

pub async fn mkdir(
    state: &AppState,
    parts: &Parts,
    target: String,
    parents: bool,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    if parents {
        let (view, created_paths) = state
            .files
            .create_folder_recursive(account_id, space_id, &path)
            .await
            .map_err(service_error)?;

        return Ok(Json(json!({
            "space": resolved.name(),
            "node": node_summary(&view),
            "created_paths": created_paths,
        })));
    }

    let (parent_path, name) = split_parent_name(&path)?;
    let parent = state
        .files
        .resolve_path(account_id, space_id, &parent_path)
        .await
        .map_err(service_error)?;

    let view = state
        .files
        .create_folder(
            account_id,
            space_id,
            CreateFolder {
                parent_node_id: parent.node.id,
                name,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "node": node_summary(&view),
    })))
}

pub async fn read(
    state: &AppState,
    parts: &Parts,
    target: String,
    start_line: Option<i64>,
    max_lines: Option<i64>,
    max_bytes: Option<usize>,
    if_none_match_sha256: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let node = state
        .files
        .resolve_path(account_id, space_id, &path)
        .await
        .map_err(service_error)?;

    let result = state
        .files
        .read_text(
            account_id,
            space_id,
            ReadText {
                node_id: node.node.id,
                start_line,
                max_lines,
                max_bytes,
                if_none_match_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    if result.storage_format == TextStorageFormat::Encrypted {
        return Err(service_error(ServiceError::InvalidInput(
            "encrypted text is not readable through MCP".to_owned(),
        )));
    }

    let space = resolved.name();
    let body = match &result.body {
        ReadTextBody::Unchanged => json!({
            "space": space,
            "path": result.node.path,
            "unchanged": true,
            "content_returned": false,
            "content_sha256": result.content_sha256,
        }),
        ReadTextBody::Encrypted(_) => {
            return Err(service_error(ServiceError::InvalidInput(
                "encrypted text is not readable through MCP".to_owned(),
            )));
        }
        ReadTextBody::Content(content) => json!({
            "space": space,
            "path": result.node.path,
            "content": content.content,
            "content_sha256": result.content_sha256,
            "byte_len": result.byte_len,
            "line_count": result.line_count,
            "start_line": content.start_line,
            "end_line": content.end_line,
            "returned_lines": content.returned_lines,
            "truncated": content.truncated,
            "next_start_line": content.next_start_line,
        }),
    };
    Ok(Json(body))
}

/// Resolve `path` to an existing node's write target, or a create target under
/// its parent when it does not exist and `create` is set. Shared by `write`
/// and `append`, which only differ in what they do with the existing view.
async fn resolve_write_target(
    state: &AppState,
    account_id: uuid::Uuid,
    space_id: uuid::Uuid,
    path: &str,
    create: bool,
) -> Result<(WriteTarget, Option<NodeView>), ErrorData> {
    let existing = match state.files.resolve_path(account_id, space_id, path).await {
        Ok(view) => Some(view),
        Err(ServiceError::NotFound(_)) => None,
        Err(error) => return Err(service_error(error)),
    };

    let target = match &existing {
        Some(view) => WriteTarget::Existing {
            node_id: view.node.id,
        },
        None => {
            if !create {
                return Err(service_error(ServiceError::NotFound(
                    "text does not exist; pass create=true to create it".to_owned(),
                )));
            }
            let (parent_path, name) = split_parent_name(path)?;
            let parent = state
                .files
                .resolve_path(account_id, space_id, &parent_path)
                .await
                .map_err(service_error)?;
            WriteTarget::Create {
                parent_node_id: parent.node.id,
                name,
            }
        }
    };

    Ok((target, existing))
}

pub async fn write(
    state: &AppState,
    parts: &Parts,
    target: String,
    content: String,
    create: bool,
    expected_sha256: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let (target, existing) =
        resolve_write_target(state, account_id, space_id, &path, create).await?;

    if let Some(view) = &existing {
        ensure_mcp_plain_text(state, account_id, space_id, view.node.id).await?;
    }

    let view = state
        .files
        .write_text(
            account_id,
            space_id,
            WriteText {
                target,
                body: WriteTextBody::Plain(content),
                expected_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "node": node_summary(&view.node),
        "content_sha256": view.text.content_sha256,
        "byte_len": view.text.byte_len,
        "line_count": view.text.line_count,
    })))
}

pub async fn append(
    state: &AppState,
    parts: &Parts,
    target: String,
    content: String,
    create: bool,
    ensure_newline: bool,
    expected_sha256: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let (target, _existing) =
        resolve_write_target(state, account_id, space_id, &path, create).await?;

    let view = state
        .files
        .append_text(
            account_id,
            space_id,
            AppendText {
                target,
                content,
                expected_sha256,
                ensure_newline,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "node": node_summary(&view.node),
        "appended": true,
        "content_sha256": view.text.content_sha256,
        "byte_len": view.text.byte_len,
        "line_count": view.text.line_count,
    })))
}

pub async fn patch(
    state: &AppState,
    parts: &Parts,
    target: String,
    edits: Vec<PatchEdit>,
    expected_sha256: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let node = state
        .files
        .resolve_path(account_id, space_id, &path)
        .await
        .map_err(service_error)?;

    let edits = edits
        .into_iter()
        .map(|edit| {
            Ok(ServiceEdit {
                old_text: edit.old_text,
                new_text: edit.new_text,
                mode: parse_patch_mode(edit.mode.as_deref())?,
                expected_count: edit.expected_count,
            })
        })
        .collect::<Result<Vec<_>, ErrorData>>()?;

    let result = state
        .files
        .patch_text(
            account_id,
            space_id,
            PatchText {
                node_id: node.node.id,
                edits,
                expected_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "path": result.node.path,
        "node": node_summary(&result.node),
        "patched": true,
        "edits_applied": result.edits_applied,
        "content_sha256": result.text.content_sha256,
        "previous_sha256": result.previous_sha256,
        "byte_len": result.text.byte_len,
        "line_count": result.text.line_count,
        "diff": result.diff,
    })))
}

pub async fn edit(
    state: &AppState,
    parts: &Parts,
    target: String,
    edits: Vec<LineEditInput>,
    expected_sha256: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let node = state
        .files
        .resolve_path(account_id, space_id, &path)
        .await
        .map_err(service_error)?;

    let edits = edits
        .into_iter()
        .map(parse_line_edit)
        .collect::<Result<Vec<_>, ErrorData>>()?;

    let result = state
        .files
        .edit_text(
            account_id,
            space_id,
            EditText {
                node_id: node.node.id,
                edits,
                expected_sha256,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "path": result.node.path,
        "node": node_summary(&result.node),
        "edited": true,
        "edits_applied": result.edits_applied,
        "content_sha256": result.text.content_sha256,
        "previous_sha256": result.previous_sha256,
        "byte_len": result.text.byte_len,
        "line_count": result.text.line_count,
        "diff": result.diff,
    })))
}

pub async fn mv(
    state: &AppState,
    parts: &Parts,
    source: String,
    destination: String,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (source_space, source_path) = resolve_target(state, caller, &source).await?;
    let (destination_space, destination_path) = resolve_target(state, caller, &destination).await?;
    let account_id = caller.account_id();
    let space_id = source_space.space_id();

    if destination_space.space_id() != space_id {
        return Err(invalid_input_error(
            "source and destination must be in the same space",
        ));
    }

    let source = state
        .files
        .resolve_path(account_id, space_id, &source_path)
        .await
        .map_err(service_error)?;

    let (dest_parent_path, new_name) = split_parent_name(&destination_path)?;
    let dest_parent = state
        .files
        .resolve_path(account_id, space_id, &dest_parent_path)
        .await
        .map_err(service_error)?;

    let view = state
        .files
        .move_node(
            account_id,
            space_id,
            MoveNode {
                node_id: source.node.id,
                new_parent_node_id: dest_parent.node.id,
                new_name: Some(new_name),
                expected_parent_id: None,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": source_space.name(),
        "node": node_summary(&view),
    })))
}

pub async fn copy(
    state: &AppState,
    parts: &Parts,
    source: String,
    destination: String,
    recursive: bool,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (source_space, source_path) = resolve_target(state, caller, &source).await?;
    let (destination_space, destination_path) = resolve_target(state, caller, &destination).await?;
    let account_id = caller.account_id();
    let space_id = source_space.space_id();

    if destination_space.space_id() != space_id {
        return Err(invalid_input_error(
            "source and destination must be in the same space",
        ));
    }

    let source = state
        .files
        .resolve_path(account_id, space_id, &source_path)
        .await
        .map_err(service_error)?;
    let (parent_path, new_name) = split_parent_name(&destination_path)?;
    let parent = state
        .files
        .resolve_path(account_id, space_id, &parent_path)
        .await
        .map_err(service_error)?;

    let result = state
        .files
        .copy_node(
            account_id,
            space_id,
            CopyNode {
                node_id: source.node.id,
                new_parent_node_id: parent.node.id,
                new_name,
                recursive,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": source_space.name(),
        "source_path": source_path,
        "node": node_summary(&result.node),
        "copied": true,
        "counts": {
            "nodes": result.counts.nodes,
            "texts": result.counts.texts,
            "files": result.counts.files,
        },
    })))
}

pub async fn rm(
    state: &AppState,
    parts: &Parts,
    target: String,
    recursive: bool,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let account_id = caller.account_id();
    let space_id = resolved.space_id();

    let node = state
        .files
        .resolve_path(account_id, space_id, &path)
        .await
        .map_err(service_error)?;

    let result = state
        .files
        .delete_node(
            account_id,
            space_id,
            DeleteNode {
                node_id: node.node.id,
                recursive,
            },
        )
        .await
        .map_err(service_error)?;

    Ok(Json(json!({
        "space": resolved.name(),
        "path": result.path,
        "deleted": true,
        "purge_after": result.purge_after,
    })))
}

async fn ensure_mcp_plain_text(
    state: &AppState,
    account_id: uuid::Uuid,
    space_id: uuid::Uuid,
    node_id: uuid::Uuid,
) -> Result<(), ErrorData> {
    let result = state
        .files
        .read_text(
            account_id,
            space_id,
            ReadText {
                node_id,
                start_line: None,
                max_lines: None,
                max_bytes: Some(1),
                if_none_match_sha256: None,
            },
        )
        .await
        .map_err(service_error)?;
    if result.storage_format == TextStorageFormat::Encrypted {
        return Err(service_error(ServiceError::InvalidInput(
            "encrypted text cannot be modified through MCP content tools".to_owned(),
        )));
    }
    Ok(())
}

fn parse_patch_mode(raw: Option<&str>) -> Result<PatchMode, ErrorData> {
    match raw.unwrap_or("unique") {
        "unique" => Ok(PatchMode::Unique),
        "first" => Ok(PatchMode::First),
        "all" => Ok(PatchMode::All),
        _ => Err(invalid_input_error(
            "mode must be 'unique', 'first', or 'all'",
        )),
    }
}

fn parse_line_edit(input: LineEditInput) -> Result<LineEdit, ErrorData> {
    match input.op.as_str() {
        "insert_before_line" => Ok(LineEdit::InsertBefore {
            line: required_i64(input.line, "line")?,
            content: required_string(input.content, "content")?,
        }),
        "insert_after_line" => Ok(LineEdit::InsertAfter {
            line: required_i64(input.line, "line")?,
            content: required_string(input.content, "content")?,
        }),
        "replace_lines" => Ok(LineEdit::ReplaceLines {
            start_line: required_i64(input.start_line, "start_line")?,
            end_line: required_i64(input.end_line, "end_line")?,
            content: required_string(input.content, "content")?,
        }),
        "delete_lines" => Ok(LineEdit::DeleteLines {
            start_line: required_i64(input.start_line, "start_line")?,
            end_line: required_i64(input.end_line, "end_line")?,
        }),
        _ => Err(invalid_input_error(
            "op must be insert_before_line, insert_after_line, replace_lines, or delete_lines",
        )),
    }
}

fn required_i64(value: Option<i64>, field: &'static str) -> Result<i64, ErrorData> {
    value.ok_or_else(|| invalid_input_error(format!("{field} is required")))
}

fn required_string(value: Option<String>, field: &'static str) -> Result<String, ErrorData> {
    value.ok_or_else(|| invalid_input_error(format!("{field} is required")))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::time::Duration;

    use notegate_core::Config;
    use notegate_core::security::PiiCrypto;
    use notegate_db::{AccountRepo, AgentRepo, ApiKeyRepo, SpaceRepo, test_support::TestDb};
    use notegate_model::{Caller, CallerIdentity, Channel, ResolveAttrs};
    use notegate_service::spaces::CreateSpace;
    use rmcp::model::ErrorCode;
    use serde_json::json;

    use super::*;

    fn test_state(pool: notegate_db::PgPool) -> Result<AppState, Box<dyn std::error::Error>> {
        let config = Arc::new(Config {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9191),
            database_url: "postgres://notegate:notegate@localhost/notegate".to_owned(),
            db_max_connections: 1,
            authgate_url: "https://auth.example.test".to_owned(),
            notegate_public_url: "http://localhost:9191".to_owned(),
            oauth_client_id: "notegate-web".to_owned(),
            mcp_oauth_client_id: "notegate-mcp".to_owned(),
            oauth_redirect_url: "http://localhost:9191/auth/callback".to_owned(),
            resource_url: "https://api.example.test".to_owned(),
            jwks_cache_ttl: Duration::from_secs(300),
            enc_root_key_id: "test-enc".to_owned(),
            enc_root_secret: secrecy::SecretString::from(
                "test-enc-root-secret-32-bytes-long".to_owned(),
            ),
            lookup_root_key_id: "test-lookup".to_owned(),
            lookup_root_secret: secrecy::SecretString::from(
                "test-lookup-root-secret-32-bytes-long".to_owned(),
            ),
            lookup_verify_0_key_id: None,
            lookup_verify_0_secret: None,
            browser_session_ttl: Duration::from_secs(3600),
            browser_session_max_ttl: Duration::from_secs(30 * 86_400),
            openapi_enabled: false,
            web_dist_dir: None,
            s3: crate::state::test_s3_config(),
            default_user_tier: notegate_core::tier::UserTier::DEFAULT,
            limits: notegate_core::limits::Limits::default(),
            secure_cookies: false,
        });
        let crypto = PiiCrypto::from_root_secrets(
            config.enc_root_key_id.clone(),
            &config.enc_root_secret,
            config.lookup_root_key_id.clone(),
            &config.lookup_root_secret,
        )?;
        let account_repo = AccountRepo::with_crypto_and_default_user_tier(
            pool.clone(),
            crypto.clone(),
            config.default_user_tier,
        );
        let api_key_repo =
            ApiKeyRepo::with_lookup_key(pool.clone(), crypto.lookup_key_id(), crypto.version());
        let resolver = notegate_service::identity::Resolver::new(
            account_repo,
            AgentRepo::new(pool.clone()),
            api_key_repo,
            crypto.clone(),
        );
        let jwt = Arc::new(crate::auth::jwt::JwtAuthority::from_url(
            &config,
            "https://auth.example.test/keys".to_owned(),
        ));
        let oidc = Arc::new(crate::auth::oidc::OidcProvider::new(
            &config,
            reqwest::Client::new(),
        ));
        Ok(AppState::new(
            pool,
            config,
            jwt,
            oidc,
            Arc::new(resolver),
            reqwest::Client::new(),
            crypto,
        ))
    }

    /// Create an owner caller with a fresh space, returning the caller, the
    /// space's MCP-visible name, the space id, and the space's root node id.
    async fn caller_with_space(
        state: &AppState,
    ) -> Result<(Caller, String, uuid::Uuid, uuid::Uuid), Box<dyn std::error::Error>> {
        let (account, user) = state
            .accounts
            .upsert_user_by_sub(&ResolveAttrs {
                sub: "files-tools-owner".to_owned(),
                email: "files-tools@example.test".to_owned(),
                name: "Files Tools Owner".to_owned(),
            })
            .await?;
        let space_name = "files-tools".to_owned();
        let space = SpaceRepo::new(state.db.clone())
            .create_space(
                account.id,
                &CreateSpace {
                    name: space_name.clone(),
                },
            )
            .await?;
        let root_id = SpaceRepo::new(state.db.clone())
            .root_node_id(space.id)
            .await?
            .expect("root node");
        let caller = Caller {
            account,
            identity: CallerIdentity::User(user),
            channel: Channel::Mcp,
        };
        Ok((caller, space_name, space.id, root_id))
    }

    /// Build a request-scoped [`Parts`] carrying the given [`Caller`] as an
    /// extension, mirroring how the MCP auth wrapper inserts it in production.
    fn parts_for(caller: Caller) -> Parts {
        let mut parts = axum::http::Request::new(()).into_parts().0;
        parts.extensions.insert(caller);
        parts
    }

    // --- resolve_write_target ---

    #[tokio::test]
    async fn resolve_write_target_finds_existing_node() -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, _space, space_id, root_id) = caller_with_space(&state).await?;
        let account_id = caller.account_id();
        let created = state
            .files
            .create_folder(
                account_id,
                space_id,
                CreateFolder {
                    parent_node_id: root_id,
                    name: "docs".to_owned(),
                },
            )
            .await?;

        let (target, existing) =
            resolve_write_target(&state, account_id, space_id, "/docs", false).await?;

        assert_eq!(
            target,
            WriteTarget::Existing {
                node_id: created.node.id
            }
        );
        assert_eq!(existing.expect("existing view").node.id, created.node.id);

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn resolve_write_target_missing_with_create_builds_create_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, _space, space_id, root_id) = caller_with_space(&state).await?;
        let account_id = caller.account_id();

        let (target, existing) =
            resolve_write_target(&state, account_id, space_id, "/note.md", true).await?;

        assert_eq!(
            target,
            WriteTarget::Create {
                parent_node_id: root_id,
                name: "note.md".to_owned(),
            }
        );
        assert!(existing.is_none());

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn resolve_write_target_missing_without_create_is_not_found()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, _space, space_id, _root_id) = caller_with_space(&state).await?;
        let account_id = caller.account_id();

        let error = resolve_write_target(&state, account_id, space_id, "/missing.md", false)
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(error.message.contains("create=true"));
        let data = error.data.expect("not_found carries data");
        assert_eq!(data["kind"], "not_found");

        db.cleanup().await;
        Ok(())
    }

    // --- mkdir ---

    #[tokio::test]
    async fn mkdir_without_parents_creates_folder() -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, space, _space_id, _root_id) = caller_with_space(&state).await?;
        let parts = parts_for(caller);

        let result = mkdir(&state, &parts, format!("{space}:/docs"), false)
            .await?
            .0;

        assert_eq!(result["space"], json!(space));
        assert_eq!(result["node"]["path"], json!("/docs"));
        assert_eq!(result["node"]["kind"], json!("folder"));

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn mkdir_with_parents_created_paths_excludes_pre_existing_ancestors()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, space, _space_id, _root_id) = caller_with_space(&state).await?;

        mkdir(
            &state,
            &parts_for(caller.clone()),
            format!("{space}:/a"),
            false,
        )
        .await?;

        let result = mkdir(&state, &parts_for(caller), format!("{space}:/a/b/c"), true)
            .await?
            .0;

        assert_eq!(result["node"]["path"], json!("/a/b/c"));
        assert_eq!(result["created_paths"], json!(["/a/b", "/a/b/c"]));

        db.cleanup().await;
        Ok(())
    }

    // --- write ---

    #[tokio::test]
    async fn write_creates_new_text_when_missing_and_create_true()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, space, _space_id, _root_id) = caller_with_space(&state).await?;
        let parts = parts_for(caller);

        let result = write(
            &state,
            &parts,
            format!("{space}:/note.md"),
            "hello".to_owned(),
            true,
            None,
        )
        .await?
        .0;

        assert_eq!(result["node"]["path"], json!("/note.md"));
        assert_eq!(result["byte_len"], json!(5));
        assert_eq!(result["line_count"], json!(1));
        assert!(result["content_sha256"].is_string());

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn write_missing_without_create_is_not_found() -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, space, _space_id, _root_id) = caller_with_space(&state).await?;
        let parts = parts_for(caller);

        let error = write(
            &state,
            &parts,
            format!("{space}:/missing.md"),
            "hello".to_owned(),
            false,
            None,
        )
        .await
        .err()
        .expect("missing target without create is an error");

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(error.message.contains("create=true"));

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn write_overwrites_existing_text_and_updates_hash()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, space, _space_id, _root_id) = caller_with_space(&state).await?;

        let first = write(
            &state,
            &parts_for(caller.clone()),
            format!("{space}:/note.md"),
            "v1".to_owned(),
            true,
            None,
        )
        .await?
        .0;
        let first_sha = first["content_sha256"]
            .as_str()
            .expect("sha string")
            .to_owned();

        let second = write(
            &state,
            &parts_for(caller),
            format!("{space}:/note.md"),
            "v2-longer".to_owned(),
            false,
            None,
        )
        .await?
        .0;

        assert_ne!(second["content_sha256"], json!(first_sha));
        assert_eq!(second["byte_len"], json!(9));

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn write_conflict_on_expected_sha256_mismatch() -> Result<(), Box<dyn std::error::Error>>
    {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, space, _space_id, _root_id) = caller_with_space(&state).await?;

        write(
            &state,
            &parts_for(caller.clone()),
            format!("{space}:/note.md"),
            "hello".to_owned(),
            true,
            None,
        )
        .await?;

        let error = write(
            &state,
            &parts_for(caller),
            format!("{space}:/note.md"),
            "world".to_owned(),
            false,
            Some("not-the-real-sha".to_owned()),
        )
        .await
        .err()
        .expect("expected_sha256 mismatch is a conflict");

        assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
        let data = error.data.expect("conflict carries data");
        assert_eq!(data["kind"], "conflict");

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn write_rejects_encrypted_existing_text() -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, space, space_id, root_id) = caller_with_space(&state).await?;
        let account_id = caller.account_id();

        // Create an encrypted text node directly through the service, bypassing
        // the MCP plain-text guard that `write()` enforces.
        state
            .files
            .write_text(
                account_id,
                space_id,
                WriteText {
                    target: WriteTarget::Create {
                        parent_node_id: root_id,
                        name: "secret.bin".to_owned(),
                    },
                    body: WriteTextBody::Encrypted(json!({"ct": "opaque"})),
                    expected_sha256: None,
                },
            )
            .await?;

        let error = write(
            &state,
            &parts_for(caller),
            format!("{space}:/secret.bin"),
            "hack".to_owned(),
            false,
            None,
        )
        .await
        .err()
        .expect("write to an encrypted text is rejected");

        assert!(
            error.message.contains("encrypted text cannot be modified"),
            "unexpected message: {}",
            error.message
        );

        db.cleanup().await;
        Ok(())
    }

    // --- append ---

    #[tokio::test]
    async fn append_creates_when_missing_then_appends_with_newline_guard()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, space, _space_id, _root_id) = caller_with_space(&state).await?;

        let created = append(
            &state,
            &parts_for(caller.clone()),
            format!("{space}:/log.md"),
            "line1".to_owned(),
            true,
            true,
            None,
        )
        .await?
        .0;
        assert_eq!(created["byte_len"], json!(5));

        let appended = append(
            &state,
            &parts_for(caller.clone()),
            format!("{space}:/log.md"),
            "line2".to_owned(),
            false,
            true,
            None,
        )
        .await?
        .0;
        // ensure_newline inserts a '\n' before appending since "line1" has none.
        assert_eq!(appended["byte_len"], json!(11));
        assert_eq!(appended["appended"], json!(true));

        let read_back = read(
            &state,
            &parts_for(caller),
            format!("{space}:/log.md"),
            None,
            None,
            None,
            None,
        )
        .await?
        .0;
        assert_eq!(read_back["content"], json!("line1\nline2"));

        db.cleanup().await;
        Ok(())
    }

    // --- read ---

    #[tokio::test]
    async fn read_returns_content_and_matches_write_hash() -> Result<(), Box<dyn std::error::Error>>
    {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (caller, space, _space_id, _root_id) = caller_with_space(&state).await?;

        let written = write(
            &state,
            &parts_for(caller.clone()),
            format!("{space}:/note.md"),
            "alpha\nbeta\n".to_owned(),
            true,
            None,
        )
        .await?
        .0;

        let result = read(
            &state,
            &parts_for(caller),
            format!("{space}:/note.md"),
            None,
            None,
            None,
            None,
        )
        .await?
        .0;

        assert_eq!(result["content"], json!("alpha\nbeta\n"));
        assert_eq!(result["content_sha256"], written["content_sha256"]);
        assert_eq!(result["byte_len"], written["byte_len"]);
        assert_eq!(result["truncated"], json!(false));

        db.cleanup().await;
        Ok(())
    }
}
