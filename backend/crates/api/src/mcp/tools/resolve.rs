//! Shared MCP tool plumbing: workspace-name resolution, target parsing, the
//! request-scoped [`Caller`] lookup, and the service-error → [`ErrorData`] map.
//!
//! MCP/CLI callers select a workspace by its human-friendly **name** (the
//! canonical selector), or with a compact `target` string (`<ws>:/<path>`), or
//! — as an explicit fallback — by `workspace_id`. Resolution is stateless: every
//! tool call resolves the selector against the caller's accessible workspaces
//! (`docs/spec/mcp/README.md`). Paths are resolved inside the selected workspace
//! only.

use std::borrow::Cow;

use axum::http::request::Parts;
use rmcp::ErrorData;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use notegate_core::validation::{normalize_path, validate_workspace_name};
use notegate_model::Caller;
use notegate_service::ServiceError;
use notegate_service::files::parse_target;
use notegate_service::workspaces::{ListWorkspaces, WorkspaceView};

use crate::state::AppState;

/// The workspace-selector fields every file tool accepts.
///
/// Exactly one selection path is taken, in priority order: a `target` string
/// (which also carries the path), then an explicit `workspace_id`, then a
/// `workspace` name. When none is given and the caller has exactly one
/// accessible workspace, that workspace is used.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct WorkspaceSelector {
    /// Human-friendly workspace name (the canonical selector).
    #[serde(default)]
    pub workspace: Option<String>,
    /// Explicit workspace id (UUID string), accepted as a fallback for name
    /// ambiguity or debugging.
    #[serde(default)]
    pub workspace_id: Option<String>,
}

/// The request-scoped authenticated caller, inserted by the MCP auth wrapper.
pub fn caller(parts: &Parts) -> Result<&Caller, ErrorData> {
    parts
        .extensions
        .get::<Caller>()
        .ok_or_else(|| invalid_input_error("authenticated caller extension missing"))
}

/// A resolved workspace selection: the chosen workspace view. The path (when a
/// `target` string carried one) is returned alongside by [`resolve_target`].
#[derive(Debug, Clone)]
pub struct ResolvedWorkspace {
    pub view: WorkspaceView,
}

impl ResolvedWorkspace {
    /// The selected workspace id.
    pub fn workspace_id(&self) -> Uuid {
        self.view.workspace.id
    }

    /// The selected workspace name.
    pub fn name(&self) -> &str {
        &self.view.workspace.name
    }
}

/// Resolve a workspace from the structured selector (`workspace` / `workspace_id`).
///
/// Resolution order: an explicit `workspace_id` (must be accessible), then a
/// `workspace` name. With neither, exactly one accessible workspace is used and
/// any other count is an error. A name matching more than one accessible
/// workspace (an agent with access across owners) returns an ambiguity error
/// listing the matches and a `workspaces_list` hint.
pub async fn resolve_workspace(
    state: &AppState,
    caller: &Caller,
    selector: &WorkspaceSelector,
) -> Result<ResolvedWorkspace, ErrorData> {
    let workspace_id = parse_workspace_id(selector.workspace_id.as_deref())?;
    let view = select_workspace(state, caller, selector.workspace.as_deref(), workspace_id).await?;
    Ok(ResolvedWorkspace { view })
}

/// Parse the optional `workspace_id` selector string into a [`Uuid`].
fn parse_workspace_id(raw: Option<&str>) -> Result<Option<Uuid>, ErrorData> {
    match raw {
        None => Ok(None),
        Some(value) => Uuid::parse_str(value)
            .map(Some)
            .map_err(|_error| invalid_input_error("workspace_id must be a UUID")),
    }
}

/// Resolve a workspace and an absolute path from either a `target` string or the
/// structured `workspace`/`workspace_id` + an explicit `path`.
///
/// `target` (`<ws>:/<path>`) takes precedence; it supplies both the workspace
/// name and the path. Otherwise the workspace is resolved from the selector and
/// the path is taken from `path`. A path must be present from exactly one source.
pub async fn resolve_target(
    state: &AppState,
    caller: &Caller,
    selector: &WorkspaceSelector,
    target: Option<&str>,
    path: Option<&str>,
) -> Result<(ResolvedWorkspace, String), ErrorData> {
    if let Some(target) = target {
        let parsed = parse_target(target).map_err(service_error)?;
        let view = select_workspace(state, caller, Some(&parsed.workspace), None).await?;
        return Ok((ResolvedWorkspace { view }, parsed.path));
    }

    let path = path.ok_or_else(|| invalid_input_error("provide a 'path' or a 'target' string"))?;
    let path = normalize_path(path).map_err(|error| invalid_input_error(error.to_string()))?;
    let resolved = resolve_workspace(state, caller, selector).await?;
    Ok((resolved, path))
}

/// Core name/id resolution against the caller's accessible workspaces.
async fn select_workspace(
    state: &AppState,
    caller: &Caller,
    name: Option<&str>,
    workspace_id: Option<Uuid>,
) -> Result<WorkspaceView, ErrorData> {
    if let Some(id) = workspace_id {
        return state
            .workspaces
            .find_visible_by_id(caller.account_id(), id)
            .await
            .map_err(service_error)?
            .ok_or_else(|| {
                ErrorData::invalid_params(
                    "workspace_id is not accessible to this caller",
                    error_meta("not_found"),
                )
            });
    }

    if let Some(name) = name {
        validate_workspace_name(name).map_err(|error| invalid_input_error(error.to_string()))?;
        let mut matches = state
            .workspaces
            .find_visible_by_name(caller.account_id(), name, 2)
            .await
            .map_err(service_error)?;
        return match matches.len() {
            0 => Err(ErrorData::invalid_params(
                format!("no accessible workspace named '{name}'"),
                error_meta("not_found"),
            )),
            1 => Ok(matches.remove(0)),
            _ => Err(ambiguity_error(name, &matches)),
        };
    }

    let page = state
        .workspaces
        .list(
            caller.account_id(),
            ListWorkspaces {
                limit: Some(2),
                cursor: None,
            },
        )
        .await
        .map_err(service_error)?;
    match page.items.len() {
        0 => Err(invalid_input_error(
            "this caller has no accessible workspaces; user callers may call workspaces_create, agent callers need a workspace grant",
        )),
        1 if !page.has_more => page
            .items
            .into_iter()
            .next()
            .ok_or_else(|| ErrorData::internal_error("failed to select workspace", None)),
        _ => Err(invalid_input_error(
            "multiple workspaces are accessible; pass 'workspace' (see workspaces_list)",
        )),
    }
}

/// Pure selection over an already-loaded accessible-workspace list (the testable
/// core of [`select_workspace`]).
///
/// Order: explicit `workspace_id` (must be accessible) → `name` (exactly one
/// match; many ⇒ ambiguity) → the single accessible workspace when neither is
/// given.
#[cfg(test)]
fn pick_workspace(
    accessible: Vec<WorkspaceView>,
    name: Option<&str>,
    workspace_id: Option<Uuid>,
) -> Result<WorkspaceView, ErrorData> {
    // Explicit id fallback: must be one the caller can access.
    if let Some(id) = workspace_id {
        return accessible
            .into_iter()
            .find(|view| view.workspace.id == id)
            .ok_or_else(|| {
                ErrorData::invalid_params(
                    "workspace_id is not accessible to this caller",
                    error_meta("not_found"),
                )
            });
    }

    // Name selector: must match exactly one accessible workspace.
    if let Some(name) = name {
        validate_workspace_name(name).map_err(|error| invalid_input_error(error.to_string()))?;
        let mut matches: Vec<WorkspaceView> = accessible
            .into_iter()
            .filter(|view| view.workspace.name == name)
            .collect();
        return match matches.len() {
            0 => Err(ErrorData::invalid_params(
                format!("no accessible workspace named '{name}'"),
                error_meta("not_found"),
            )),
            1 => Ok(matches.remove(0)),
            _ => Err(ambiguity_error(name, &matches)),
        };
    }

    // No selector: use the single accessible workspace, if exactly one.
    let count = accessible.len();
    let mut iter = accessible.into_iter();
    match (count, iter.next()) {
        (1, Some(view)) => Ok(view),
        (0, _) => Err(invalid_input_error(
            "this caller has no accessible workspaces; user callers may call workspaces_create, agent callers need a workspace grant",
        )),
        _ => Err(invalid_input_error(
            "multiple workspaces are accessible; pass 'workspace' (see workspaces_list)",
        )),
    }
}

/// Build the ambiguity error for a name that resolves to multiple accessible
/// workspaces, embedding the matches and a `workspaces_list` hint in `data`.
fn ambiguity_error(name: &str, matches: &[WorkspaceView]) -> ErrorData {
    let workspaces: Vec<_> = matches
        .iter()
        .map(|view| {
            json!({
                "id": view.workspace.id,
                "name": view.workspace.name,
                "role": view.role.as_str(),
            })
        })
        .collect();
    ErrorData::invalid_params(
        format!("workspace name '{name}' is ambiguous; pass 'workspace_id'"),
        Some(json!({
            "kind": "invalid_input",
            "code": "workspace_ambiguous",
            "workspace": name,
            "matches": workspaces,
            "hint": "call workspaces_list and select a workspace_id",
        })),
    )
}

/// Map a service-layer error to an MCP [`ErrorData`], preserving the status
/// class (validation/not-found vs. conflict vs. internal) and redacting internal
/// detail.
pub fn service_error(error: ServiceError) -> ErrorData {
    match error {
        ServiceError::NotFound(message) => {
            ErrorData::invalid_params(Cow::Owned(message), error_meta("not_found"))
        }
        ServiceError::InvalidInput(message) => {
            ErrorData::invalid_params(Cow::Owned(message), error_meta("invalid_input"))
        }
        ServiceError::Forbidden(message) => {
            ErrorData::invalid_request(Cow::Owned(message), error_meta("forbidden"))
        }
        ServiceError::Conflict(message) => {
            ErrorData::invalid_request(Cow::Owned(message), error_meta("conflict"))
        }
        ServiceError::Internal(message) => {
            tracing::error!(event = "mcp.error.internal", detail = %message);
            ErrorData::internal_error("internal server error", error_meta("internal"))
        }
    }
}

fn error_meta(kind: &'static str) -> Option<serde_json::Value> {
    Some(json!({
        "kind": kind,
        "code": kind,
    }))
}

pub fn invalid_input_error(message: impl Into<Cow<'static, str>>) -> ErrorData {
    ErrorData::invalid_params(message, error_meta("invalid_input"))
}

/// Split an absolute path into its parent path and basename.
///
/// `/projects/note.md` → (`/projects`, `note.md`); `/note.md` → (`/`, `note.md`).
/// The root path (`/`) and empty/relative paths have no basename and are an
/// error (the caller cannot create or address "root" by basename).
pub fn split_parent_name(path: &str) -> Result<(String, String), ErrorData> {
    let normalized =
        normalize_path(path).map_err(|error| invalid_input_error(error.to_string()))?;
    let Some((parent, name)) = normalized.rsplit_once('/') else {
        return Err(invalid_input_error("path must start with '/'"));
    };
    if name.is_empty() {
        return Err(invalid_input_error(
            "path must name a node, not the workspace root",
        ));
    }
    let parent = if parent.is_empty() {
        "/".to_owned()
    } else {
        parent.to_owned()
    };
    Ok((parent, name.to_owned()))
}

/// The canonical `{id, name, role, root_node_id}` workspace summary used by
/// `workspaces_list` and `workspaces_get`.
pub fn workspace_summary(view: &WorkspaceView) -> serde_json::Value {
    json!({
        "id": view.workspace.id,
        "name": view.workspace.name,
        "role": view.role.as_str(),
        "root_node_id": view.root_node_id,
    })
}

/// A path-first node summary for file tools (`ls`/`stat`/`find`/mutation
/// results). Path is the canonical derived absolute path; `node_id` is included
/// for callers that need a stable identity but is never required as input.
pub fn node_summary(view: &notegate_service::files::NodeView) -> serde_json::Value {
    let mut value = json!({
        "path": view.path,
        "name": view.node.name,
        "kind": view.node.kind.as_str(),
        "node_id": view.node.id,
        "has_children": view.has_children,
        "sort_order": view.node.sort_order,
        "created_at": view.node.created_at,
        "updated_at": view.node.updated_at,
    });
    if let Some(document) = &view.document
        && let Some(object) = value.as_object_mut()
    {
        object.insert("content_sha256".to_owned(), json!(document.content_sha256));
        object.insert("byte_len".to_owned(), json!(document.byte_len));
        object.insert("line_count".to_owned(), json!(document.line_count));
    }
    value
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
    use super::*;
    use chrono::Utc;
    use notegate_model::{Role, Workspace};
    use notegate_service::files::parse_target;
    use rmcp::model::ErrorCode;

    fn view(name: &str, owner: Uuid) -> WorkspaceView {
        WorkspaceView {
            workspace: Workspace {
                id: Uuid::new_v4(),
                owner_account_id: owner,
                name: name.to_owned(),
                created_by: owner,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            role: Role::Viewer,
            root_node_id: Uuid::new_v4(),
        }
    }

    #[test]
    fn target_parses_workspace_and_absolute_path() {
        let parsed = parse_target("personal:/notes/test.md").unwrap();
        assert_eq!(parsed.workspace, "personal");
        assert_eq!(parsed.path, "/notes/test.md");
    }

    #[test]
    fn target_rejects_bad_grammar() {
        // Missing the ':' separator.
        assert!(parse_target("personal/notes.md").is_err());
        // Non-absolute path after the separator.
        assert!(parse_target("personal:notes.md").is_err());
        // Invalid workspace-name segment.
        assert!(parse_target(".secret:/notes.md").is_err());
    }

    #[test]
    fn ambiguity_error_lists_matches_and_hint() {
        let matches = vec![
            view("shared", Uuid::new_v4()),
            view("shared", Uuid::new_v4()),
        ];
        let error = ambiguity_error("shared", &matches);
        let data = error.data.expect("ambiguity carries data");
        assert_eq!(data["kind"], "invalid_input");
        assert_eq!(data["code"], "workspace_ambiguous");
        assert_eq!(data["matches"].as_array().unwrap().len(), 2);
        assert!(data["hint"].as_str().unwrap().contains("workspaces_list"));
    }

    #[test]
    fn service_error_carries_structured_kind_data() {
        let missing = service_error(ServiceError::NotFound("missing".to_owned()));
        assert_eq!(missing.code, ErrorCode::INVALID_PARAMS);
        let missing_data = missing.data.expect("not_found carries data");
        assert_eq!(missing_data["kind"], "not_found");
        assert_eq!(missing_data["code"], "not_found");

        let conflict = service_error(ServiceError::Conflict("stale".to_owned()));
        assert_eq!(conflict.code, ErrorCode::INVALID_REQUEST);
        let conflict_data = conflict.data.expect("conflict carries data");
        assert_eq!(conflict_data["kind"], "conflict");
        assert_eq!(conflict_data["code"], "conflict");
    }

    #[test]
    fn name_matching_two_accessible_workspaces_is_ambiguous() {
        let accessible = vec![
            view("shared", Uuid::new_v4()),
            view("shared", Uuid::new_v4()),
        ];
        let error = pick_workspace(accessible, Some("shared"), None).unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let data = error.data.expect("ambiguity carries data");
        assert_eq!(data["kind"], "invalid_input");
        assert_eq!(data["code"], "workspace_ambiguous");
        assert_eq!(data["matches"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn single_accessible_workspace_used_when_selector_omitted() {
        let only = view("personal", Uuid::new_v4());
        let expected = only.workspace.id;
        let chosen = pick_workspace(vec![only], None, None).unwrap();
        assert_eq!(chosen.workspace.id, expected);
    }

    #[test]
    fn name_matching_one_accessible_workspace_resolves() {
        let accessible = vec![
            view("personal", Uuid::new_v4()),
            view("research", Uuid::new_v4()),
        ];
        let chosen = pick_workspace(accessible, Some("research"), None).unwrap();
        assert_eq!(chosen.workspace.name, "research");
    }

    #[test]
    fn omitted_selector_with_many_accessible_requires_a_choice() {
        let accessible = vec![view("a", Uuid::new_v4()), view("b", Uuid::new_v4())];
        let error = pick_workspace(accessible, None, None).unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let data = error.data.expect("invalid selection carries data");
        assert_eq!(data["kind"], "invalid_input");
    }

    #[test]
    fn explicit_workspace_id_must_be_accessible() {
        let accessible = vec![view("a", Uuid::new_v4())];
        let error = pick_workspace(accessible, None, Some(Uuid::new_v4())).unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let data = error.data.expect("inaccessible id carries not_found data");
        assert_eq!(data["kind"], "not_found");
    }

    #[test]
    fn name_matching_no_accessible_workspace_is_not_found() {
        let accessible = vec![view("a", Uuid::new_v4())];
        let error = pick_workspace(accessible, Some("missing"), None).unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let data = error.data.expect("missing name carries not_found data");
        assert_eq!(data["kind"], "not_found");
    }

    #[test]
    fn explicit_workspace_id_selects_the_match() {
        let target = view("a", Uuid::new_v4());
        let id = target.workspace.id;
        let accessible = vec![target, view("b", Uuid::new_v4())];
        let chosen = pick_workspace(accessible, None, Some(id)).unwrap();
        assert_eq!(chosen.workspace.id, id);
    }

    #[test]
    fn bad_workspace_name_grammar_is_rejected() {
        let error = pick_workspace(Vec::new(), Some(".secret"), None).unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let data = error.data.expect("invalid name carries data");
        assert_eq!(data["kind"], "invalid_input");
    }

    #[test]
    fn split_parent_name_separates_dirname_and_basename() {
        assert_eq!(
            split_parent_name("/projects/note.md").unwrap(),
            ("/projects".to_owned(), "note.md".to_owned())
        );
        assert_eq!(
            split_parent_name("/note.md").unwrap(),
            ("/".to_owned(), "note.md".to_owned())
        );
        assert_eq!(
            split_parent_name("/projects//note.md/").unwrap(),
            ("/projects".to_owned(), "note.md".to_owned())
        );
    }

    #[test]
    fn split_parent_name_rejects_root_and_relative() {
        for path in ["/", "relative.md", "/a/../b.md"] {
            let error = split_parent_name(path).unwrap_err();
            let data = error.data.expect("invalid path carries data");
            assert_eq!(data["kind"], "invalid_input");
        }
    }
}
