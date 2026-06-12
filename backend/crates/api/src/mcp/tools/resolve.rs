//! Shared MCP tool plumbing: space-name resolution, target parsing, the
//! request-scoped [`Caller`] lookup, and the service-error → [`ErrorData`] map.
//!
//! MCP/CLI callers select a space by its human-friendly **name** (the
//! canonical name), or with a compact `target` string (`<space>:/<path>`).
//! Resolution is stateless: every tool call resolves the target space name against the
//! caller's accessible spaces (`docs/spec/mcp/README.md`). Paths are resolved
//! inside the selected space only.

use std::borrow::Cow;

use axum::http::request::Parts;
use rmcp::ErrorData;
use serde_json::json;
use uuid::Uuid;

use notegate_core::validation::{normalize_path, validate_space_name};
use notegate_model::Caller;
use notegate_service::ServiceError;
use notegate_service::files::parse_target;
use notegate_service::spaces::SpaceView;

use crate::state::AppState;

/// The request-scoped authenticated caller, inserted by the MCP auth wrapper.
pub fn caller(parts: &Parts) -> Result<&Caller, ErrorData> {
    parts
        .extensions
        .get::<Caller>()
        .ok_or_else(|| invalid_input_error("authenticated caller extension missing"))
}

/// A resolved space selection: the chosen space view. The path (when a
/// `target` string carried one) is returned alongside by [`resolve_target`].
#[derive(Debug, Clone)]
pub struct ResolvedSpace {
    pub view: SpaceView,
}

impl ResolvedSpace {
    /// The selected space id.
    pub fn space_id(&self) -> Uuid {
        self.view.space.id
    }

    /// The selected space name.
    pub fn name(&self) -> &str {
        &self.view.space.name
    }
}

/// Resolve a space by its MCP-visible name.
pub async fn resolve_space(
    state: &AppState,
    caller: &Caller,
    name: &str,
) -> Result<ResolvedSpace, ErrorData> {
    let view = select_space(state, caller, name).await?;
    Ok(ResolvedSpace { view })
}

/// Resolve a compact MCP target string (`<space>:/<path>`) into a visible space
/// and a normalized absolute path inside that space.
pub async fn resolve_target(
    state: &AppState,
    caller: &Caller,
    target: &str,
) -> Result<(ResolvedSpace, String), ErrorData> {
    let parsed = parse_target(target).map_err(service_error)?;
    let view = select_space(state, caller, &parsed.space).await?;
    Ok((ResolvedSpace { view }, parsed.path))
}

/// Core name resolution against the caller's accessible spaces.
async fn select_space(
    state: &AppState,
    caller: &Caller,
    name: &str,
) -> Result<SpaceView, ErrorData> {
    validate_space_name(name).map_err(|error| invalid_input_error(error.to_string()))?;
    let mut matches = state
        .spaces
        .find_visible_by_name(caller.account_id(), name, 2)
        .await
        .map_err(service_error)?;
    match matches.len() {
        0 => Err(ErrorData::invalid_params(
            format!("no accessible space named '{name}'"),
            error_meta("not_found"),
        )),
        1 => Ok(matches.remove(0)),
        _ => Err(ambiguity_error(name, &matches)),
    }
}

/// Pure name selection over an already-loaded accessible-space list (the testable
/// core of [`select_space`]).
#[cfg(test)]
fn pick_space(accessible: Vec<SpaceView>, name: &str) -> Result<SpaceView, ErrorData> {
    validate_space_name(name).map_err(|error| invalid_input_error(error.to_string()))?;
    let mut matches: Vec<SpaceView> = accessible
        .into_iter()
        .filter(|view| view.space.name == name)
        .collect();
    match matches.len() {
        0 => Err(ErrorData::invalid_params(
            format!("no accessible space named '{name}'"),
            error_meta("not_found"),
        )),
        1 => Ok(matches.remove(0)),
        _ => Err(ambiguity_error(name, &matches)),
    }
}

/// Build the ambiguity error for a name that resolves to multiple accessible
/// spaces, embedding the matches and a `read op=spaces` hint in `data`.
fn ambiguity_error(name: &str, matches: &[SpaceView]) -> ErrorData {
    let spaces: Vec<_> = matches
        .iter()
        .map(|view| {
            json!({
                "name": view.space.name,
                "permission": view.permission.as_str(),
            })
        })
        .collect();
    ErrorData::invalid_params(
        format!("space name '{name}' is ambiguous; use a unique space name"),
        Some(json!({
            "kind": "invalid_input",
            "code": "space_ambiguous",
            "space": name,
            "matches": spaces,
            "hint": "rename spaces so MCP can select by name; use read op=spaces to inspect accessible spaces",
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
            ErrorData::internal_error("internal server error", error_meta("internal_error"))
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
            "path must name a node, not the space root",
        ));
    }
    let parent = if parent.is_empty() {
        "/".to_owned()
    } else {
        parent.to_owned()
    };
    Ok((parent, name.to_owned()))
}

/// The canonical space summary used by `read op=spaces`.
pub fn space_summary(view: &SpaceView) -> serde_json::Value {
    json!({
        "name": view.space.name,
        "sort_order": view.space.sort_order,
        "permission": view.permission.as_str(),
    })
}

/// A path-first node summary for file tools (`list`/`stat`/`find`/mutation
/// results). Path is the canonical derived absolute path for MCP/CLI callers.
pub fn node_summary(view: &notegate_service::files::NodeView) -> serde_json::Value {
    let mut value = json!({
        "path": view.path,
        "name": view.node.name,
        "kind": view.node.kind.as_str(),
        "has_children": view.has_children,
        "sort_order": view.node.sort_order,
        "created_at": view.node.created_at,
        "updated_at": view.node.updated_at,
    });
    if let Some(text) = &view.text
        && let Some(object) = value.as_object_mut()
    {
        object.insert("content_sha256".to_owned(), json!(text.content_sha256));
        object.insert("byte_len".to_owned(), json!(text.byte_len));
        object.insert("line_count".to_owned(), json!(text.line_count));
    }
    if let Some(file) = &view.file
        && let Some(object) = value.as_object_mut()
    {
        object.insert("content_sha256".to_owned(), json!(file.content_sha256));
        object.insert("byte_len".to_owned(), json!(file.byte_len));
        object.insert("media_type".to_owned(), json!(file.media_type));
        object.insert("storage_kind".to_owned(), json!(file.storage_kind.as_str()));
        object.insert(
            "encryption_mode".to_owned(),
            json!(file.encryption_mode.as_str()),
        );
        if let Some(name) = &file.original_filename {
            object.insert("original_filename".to_owned(), json!(name));
        }
        if let Some(metadata) = &file.encryption_metadata {
            object.insert("encryption_metadata".to_owned(), metadata.clone());
        }
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
    use notegate_model::{Permission, Space};
    use notegate_service::files::parse_target;
    use rmcp::model::ErrorCode;

    fn view(name: &str, owner: Uuid) -> SpaceView {
        SpaceView {
            space: Space {
                id: Uuid::new_v4(),
                name: name.to_owned(),
                sort_order: 0,
                owner_user_id: owner,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                deleted_at: None,
                deleted_by_user_id: None,
                purge_after: None,
            },
            permission: Permission::Read,
            root_node_id: Uuid::new_v4(),
        }
    }

    #[test]
    fn target_parses_space_and_absolute_path() {
        let parsed = parse_target("personal:/notes/test.md").unwrap();
        assert_eq!(parsed.space, "personal");
        assert_eq!(parsed.path, "/notes/test.md");
    }

    #[test]
    fn target_rejects_bad_grammar() {
        // Missing the ':' separator.
        assert!(parse_target("personal/notes.md").is_err());
        // Non-absolute path after the separator.
        assert!(parse_target("personal:notes.md").is_err());
        // Invalid space-name segment.
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
        assert_eq!(data["code"], "space_ambiguous");
        assert_eq!(data["matches"].as_array().unwrap().len(), 2);
        assert!(data["hint"].as_str().unwrap().contains("select by name"));
    }

    #[test]
    fn service_error_carries_structured_kind_data() {
        let missing = service_error(ServiceError::NotFound("missing".to_owned()));
        assert_eq!(missing.code, ErrorCode::INVALID_PARAMS);
        let missing_data = missing.data.expect("not_found carries data");
        assert_eq!(missing_data["kind"], "not_found");
        assert_eq!(missing_data["code"], "not_found");

        let invalid = service_error(ServiceError::InvalidInput("bad".to_owned()));
        assert_eq!(invalid.code, ErrorCode::INVALID_PARAMS);
        let invalid_data = invalid.data.expect("invalid_input carries data");
        assert_eq!(invalid_data["kind"], "invalid_input");
        assert_eq!(invalid_data["code"], "invalid_input");

        let forbidden = service_error(ServiceError::Forbidden("no".to_owned()));
        assert_eq!(forbidden.code, ErrorCode::INVALID_REQUEST);
        let forbidden_data = forbidden.data.expect("forbidden carries data");
        assert_eq!(forbidden_data["kind"], "forbidden");
        assert_eq!(forbidden_data["code"], "forbidden");

        let conflict = service_error(ServiceError::Conflict("stale".to_owned()));
        assert_eq!(conflict.code, ErrorCode::INVALID_REQUEST);
        let conflict_data = conflict.data.expect("conflict carries data");
        assert_eq!(conflict_data["kind"], "conflict");
        assert_eq!(conflict_data["code"], "conflict");

        let internal = service_error(ServiceError::Internal("db detail".to_owned()));
        assert_eq!(internal.code, ErrorCode::INTERNAL_ERROR);
        assert_eq!(internal.message, "internal server error");
        let internal_data = internal.data.expect("internal_error carries data");
        assert_eq!(internal_data["kind"], "internal_error");
        assert_eq!(internal_data["code"], "internal_error");
    }

    #[test]
    fn name_matching_two_accessible_spaces_is_ambiguous() {
        let accessible = vec![
            view("shared", Uuid::new_v4()),
            view("shared", Uuid::new_v4()),
        ];
        let error = pick_space(accessible, "shared").unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let data = error.data.expect("ambiguity carries data");
        assert_eq!(data["kind"], "invalid_input");
        assert_eq!(data["code"], "space_ambiguous");
        assert_eq!(data["matches"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn name_matching_one_accessible_space_resolves() {
        let accessible = vec![
            view("personal", Uuid::new_v4()),
            view("research", Uuid::new_v4()),
        ];
        let chosen = pick_space(accessible, "research").unwrap();
        assert_eq!(chosen.space.name, "research");
    }

    #[test]
    fn name_matching_no_accessible_space_is_not_found() {
        let accessible = vec![view("a", Uuid::new_v4())];
        let error = pick_space(accessible, "missing").unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let data = error.data.expect("missing name carries not_found data");
        assert_eq!(data["kind"], "not_found");
    }

    #[test]
    fn bad_space_name_grammar_is_rejected() {
        let error = pick_space(Vec::new(), ".secret").unwrap_err();
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
