//! Shared MCP tool plumbing: space-name resolution, target parsing, the
//! request-scoped [`Caller`] lookup, and the service-error → [`ErrorData`] map.
//!
//! MCP/CLI callers select a space by its human-friendly **name** (the
//! canonical selector), or with a compact `target` string (`<space>:/<path>`).
//! Resolution is stateless: every tool call resolves the selector against the
//! caller's accessible spaces (`docs/spec/mcp/README.md`). Paths are resolved
//! inside the selected space only.

use std::borrow::Cow;

use axum::http::request::Parts;
use rmcp::ErrorData;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use notegate_core::validation::{normalize_path, validate_space_name};
use notegate_model::Caller;
use notegate_service::ServiceError;
use notegate_service::files::parse_target;
use notegate_service::spaces::{ListSpaces, SpaceView};

use crate::state::AppState;

/// The space-selector fields every file tool accepts.
///
/// Selection uses a `target` string (which also carries the path) or a `space`
/// name. When none is given and the caller has exactly one accessible space,
/// that space is used.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SpaceSelector {
    /// Human-friendly space name (the canonical selector).
    #[serde(default)]
    pub space: Option<String>,
}

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

/// Resolve a space from the structured selector (`space`).
///
/// With no selector, exactly one accessible space is used and any other count
/// is an error. A name matching more than one accessible space returns an
/// ambiguity error.
pub async fn resolve_space(
    state: &AppState,
    caller: &Caller,
    selector: &SpaceSelector,
) -> Result<ResolvedSpace, ErrorData> {
    let view = select_space(state, caller, selector.space.as_deref()).await?;
    Ok(ResolvedSpace { view })
}

/// Resolve a space and an absolute path from either a `target` string or the
/// structured `space` + an explicit `path`.
///
/// `target` (`<ws>:/<path>`) takes precedence; it supplies both the space
/// name and the path. Otherwise the space is resolved from the selector and
/// the path is taken from `path`.
pub async fn resolve_target(
    state: &AppState,
    caller: &Caller,
    selector: &SpaceSelector,
    target: Option<&str>,
    path: Option<&str>,
) -> Result<(ResolvedSpace, String), ErrorData> {
    if let Some(target) = target {
        let parsed = parse_target(target).map_err(service_error)?;
        let view = select_space(state, caller, Some(&parsed.space)).await?;
        return Ok((ResolvedSpace { view }, parsed.path));
    }

    let path = path.ok_or_else(|| invalid_input_error("provide a 'path' or a 'target' string"))?;
    let path = normalize_path(path).map_err(|error| invalid_input_error(error.to_string()))?;
    let resolved = resolve_space(state, caller, selector).await?;
    Ok((resolved, path))
}

/// Core name resolution against the caller's accessible spaces.
async fn select_space(
    state: &AppState,
    caller: &Caller,
    name: Option<&str>,
) -> Result<SpaceView, ErrorData> {
    if let Some(name) = name {
        validate_space_name(name).map_err(|error| invalid_input_error(error.to_string()))?;
        let mut matches = state
            .spaces
            .find_visible_by_name(caller.account_id(), name, 2)
            .await
            .map_err(service_error)?;
        return match matches.len() {
            0 => Err(ErrorData::invalid_params(
                format!("no accessible space named '{name}'"),
                error_meta("not_found"),
            )),
            1 => Ok(matches.remove(0)),
            _ => Err(ambiguity_error(name, &matches)),
        };
    }

    let page = state
        .spaces
        .list(
            caller.account_id(),
            ListSpaces {
                limit: Some(2),
                cursor: None,
            },
        )
        .await
        .map_err(service_error)?;
    match page.items.len() {
        0 => Err(invalid_input_error(
            "this caller has no accessible spaces; user callers may call spaces_create, agent callers need a space connection",
        )),
        1 if !page.has_more => page.items.into_iter().next().ok_or_else(|| {
            ErrorData::internal_error("failed to select space", error_meta("internal_error"))
        }),
        _ => Err(invalid_input_error(
            "multiple spaces are accessible; pass 'space' (see spaces_list)",
        )),
    }
}

/// Pure selection over an already-loaded accessible-space list (the testable
/// core of [`select_space`]).
///
/// Order: `name` (exactly one match; many ⇒ ambiguity) → the single
/// accessible space when neither is given.
#[cfg(test)]
fn pick_space(accessible: Vec<SpaceView>, name: Option<&str>) -> Result<SpaceView, ErrorData> {
    // Name selector: must match exactly one accessible space.
    if let Some(name) = name {
        validate_space_name(name).map_err(|error| invalid_input_error(error.to_string()))?;
        let mut matches: Vec<SpaceView> = accessible
            .into_iter()
            .filter(|view| view.space.name == name)
            .collect();
        return match matches.len() {
            0 => Err(ErrorData::invalid_params(
                format!("no accessible space named '{name}'"),
                error_meta("not_found"),
            )),
            1 => Ok(matches.remove(0)),
            _ => Err(ambiguity_error(name, &matches)),
        };
    }

    // No selector: use the single accessible space, if exactly one.
    let count = accessible.len();
    let mut iter = accessible.into_iter();
    match (count, iter.next()) {
        (1, Some(view)) => Ok(view),
        (0, _) => Err(invalid_input_error(
            "this caller has no accessible spaces; user callers may call spaces_create, agent callers need a space connection",
        )),
        _ => Err(invalid_input_error(
            "multiple spaces are accessible; pass 'space' (see spaces_list)",
        )),
    }
}

/// Build the ambiguity error for a name that resolves to multiple accessible
/// spaces, embedding the matches and a `spaces_list` hint in `data`.
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
            "hint": "rename spaces so MCP can select by name",
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

/// The canonical space summary used by `spaces_list` and `spaces_get`.
pub fn space_summary(view: &SpaceView) -> serde_json::Value {
    json!({
        "name": view.space.name,
        "sort_order": view.space.sort_order,
        "permission": view.permission.as_str(),
    })
}

/// A path-first node summary for file tools (`ls`/`stat`/`find`/mutation
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
        let error = pick_space(accessible, Some("shared")).unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let data = error.data.expect("ambiguity carries data");
        assert_eq!(data["kind"], "invalid_input");
        assert_eq!(data["code"], "space_ambiguous");
        assert_eq!(data["matches"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn single_accessible_space_used_when_selector_omitted() {
        let only = view("personal", Uuid::new_v4());
        let expected = only.space.id;
        let chosen = pick_space(vec![only], None).unwrap();
        assert_eq!(chosen.space.id, expected);
    }

    #[test]
    fn name_matching_one_accessible_space_resolves() {
        let accessible = vec![
            view("personal", Uuid::new_v4()),
            view("research", Uuid::new_v4()),
        ];
        let chosen = pick_space(accessible, Some("research")).unwrap();
        assert_eq!(chosen.space.name, "research");
    }

    #[test]
    fn omitted_selector_with_many_accessible_requires_a_choice() {
        let accessible = vec![view("a", Uuid::new_v4()), view("b", Uuid::new_v4())];
        let error = pick_space(accessible, None).unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let data = error.data.expect("invalid selection carries data");
        assert_eq!(data["kind"], "invalid_input");
    }

    #[test]
    fn name_matching_no_accessible_space_is_not_found() {
        let accessible = vec![view("a", Uuid::new_v4())];
        let error = pick_space(accessible, Some("missing")).unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let data = error.data.expect("missing name carries not_found data");
        assert_eq!(data["kind"], "not_found");
    }

    #[test]
    fn bad_space_name_grammar_is_rejected() {
        let error = pick_space(Vec::new(), Some(".secret")).unwrap_err();
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
