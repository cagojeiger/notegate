//! Space and path resolution shared by command transports.

use notegate_command::CommandError;
#[cfg(test)]
use notegate_command::CommandErrorClass;
use notegate_core::validation::{normalize_path, validate_space_name};
use notegate_model::Caller;
use notegate_service::files::parse_target;
use notegate_service::spaces::SpaceView;
use serde_json::json;
use uuid::Uuid;

pub use super::error::{
    actionable_input_error, invalid_input_error, required_input, search_error, service_error,
};
use crate::state::AppState;

const SPACE_SUGGESTION_LIMIT: i64 = 5;

/// A selected space visible to the authenticated caller.
#[derive(Debug, Clone)]
pub struct ResolvedSpace {
    pub view: SpaceView,
}

impl ResolvedSpace {
    pub fn space_id(&self) -> Uuid {
        self.view.space.id
    }

    pub fn name(&self) -> &str {
        &self.view.space.name
    }
}

/// Resolve a space by its command-visible, case-sensitive name.
pub async fn resolve_space(
    state: &AppState,
    caller: &Caller,
    name: &str,
) -> Result<ResolvedSpace, CommandError> {
    let view = select_space(state, caller, name).await?;
    Ok(ResolvedSpace { view })
}

/// Resolve `<space>:/<path>` into a visible space and normalized absolute path.
pub async fn resolve_target(
    state: &AppState,
    caller: &Caller,
    target: &str,
) -> Result<(ResolvedSpace, String), CommandError> {
    let parsed = parse_target(target).map_err(service_error)?;
    let view = select_space(state, caller, &parsed.space).await?;
    Ok((ResolvedSpace { view }, parsed.path))
}

async fn select_space(
    state: &AppState,
    caller: &Caller,
    name: &str,
) -> Result<SpaceView, CommandError> {
    validate_space_name(name).map_err(|error| invalid_input_error(error.to_string()))?;
    let mut matches = state
        .spaces
        .find_mcp_visible_by_name(caller.account_id(), name, 2)
        .await
        .map_err(service_error)?;
    match matches.len() {
        0 => {
            let suggestions = state
                .spaces
                .find_mcp_visible_by_name_case_insensitive(
                    caller.account_id(),
                    name,
                    SPACE_SUGGESTION_LIMIT,
                )
                .await
                .map_err(service_error)?;
            Err(space_not_found_error(name, &suggestions))
        }
        1 => Ok(matches.remove(0)),
        _ => Err(ambiguity_error(name, &matches)),
    }
}

#[cfg(test)]
fn pick_space(accessible: Vec<SpaceView>, name: &str) -> Result<SpaceView, CommandError> {
    validate_space_name(name).map_err(|error| invalid_input_error(error.to_string()))?;
    let mut matches: Vec<SpaceView> = accessible
        .iter()
        .filter(|view| view.space.name == name)
        .cloned()
        .collect();
    match matches.len() {
        0 => {
            let needle = name.to_lowercase();
            let suggestions: Vec<SpaceView> = accessible
                .iter()
                .filter(|view| view.space.name.to_lowercase() == needle)
                .take(SPACE_SUGGESTION_LIMIT as usize)
                .cloned()
                .collect();
            Err(space_not_found_error(name, &suggestions))
        }
        1 => Ok(matches.remove(0)),
        _ => Err(ambiguity_error(name, &matches)),
    }
}

fn space_not_found_error(name: &str, suggestions: &[SpaceView]) -> CommandError {
    let suggestions: Vec<_> = suggestions
        .iter()
        .map(|view| view.space.name.as_str())
        .collect();
    let mut message = format!("no accessible space named '{name}'");
    if let [suggestion] = suggestions.as_slice() {
        message.push_str(&format!("; did you mean '{suggestion}'?"));
    }
    CommandError::invalid_params(message).with_data(json!({
        "kind": "not_found",
        "code": "not_found",
        "resource": "space",
        "space": name,
        "suggestions": suggestions,
        "hint": "use read op=spaces to inspect accessible spaces and use the exact space name",
    }))
}

fn ambiguity_error(name: &str, matches: &[SpaceView]) -> CommandError {
    let spaces: Vec<_> = matches
        .iter()
        .map(|view| {
            json!({
                "name": view.space.name,
                "permission": view.permission.as_str(),
            })
        })
        .collect();
    CommandError::invalid_params(format!(
        "space name '{name}' is ambiguous; use a unique space name"
    ))
    .with_data(json!({
        "kind": "invalid_input",
        "code": "space_ambiguous",
        "space": name,
        "matches": spaces,
        "hint": "rename spaces so MCP can select by name; use read op=spaces to inspect accessible spaces",
    }))
}

/// Split an absolute path into its parent path and basename.
pub fn split_parent_name(path: &str) -> Result<(String, String), CommandError> {
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
        "default_search_enabled": view.space.default_search_enabled,
        "default_text_encryption_enabled": view.space.default_text_encryption_enabled,
        "features": {
            "text_encryption": view.features.text_encryption,
        },
    })
}

/// A path-first node summary for read, search, and mutation results.
pub fn node_summary(view: &notegate_service::files::NodeView) -> serde_json::Value {
    json!(crate::path_node_summary::PathNodeSummary::from(view))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::unwrap_in_result
    )]

    use chrono::Utc;
    use notegate_core::tier::UserTier;
    use notegate_model::{Permission, Space};

    use super::*;

    fn view(name: &str, owner: Uuid) -> SpaceView {
        SpaceView {
            space: Space {
                id: Uuid::new_v4(),
                name: name.to_owned(),
                sort_order: 0,
                navigation_pinned_at: None,
                user_mcp_enabled_at: None,
                default_search_enabled: true,
                default_text_encryption_enabled: false,
                owner_user_id: owner,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                deleted_at: None,
                deleted_by_user_id: None,
                purge_after: None,
            },
            permission: Permission::Read,
            root_node_id: Uuid::new_v4(),
            features: UserTier::Tier0.features(),
        }
    }

    #[test]
    fn ambiguity_error_lists_matches_and_hint() {
        let matches = vec![
            view("shared", Uuid::new_v4()),
            view("shared", Uuid::new_v4()),
        ];
        let error = ambiguity_error("shared", &matches);
        assert_eq!(error.class, CommandErrorClass::InvalidParams);
        let data = error.data.expect("ambiguity carries data");
        assert_eq!(data["kind"], "invalid_input");
        assert_eq!(data["code"], "space_ambiguous");
        assert_eq!(data["matches"].as_array().unwrap().len(), 2);
        assert!(data["hint"].as_str().unwrap().contains("select by name"));
    }

    #[test]
    fn exact_name_selects_one_accessible_space() {
        let accessible = vec![
            view("personal", Uuid::new_v4()),
            view("research", Uuid::new_v4()),
        ];
        let chosen = pick_space(accessible, "research").unwrap();
        assert_eq!(chosen.space.name, "research");
    }

    #[test]
    fn missing_name_has_not_found_metadata() {
        let error = pick_space(vec![view("a", Uuid::new_v4())], "missing").unwrap_err();
        assert_eq!(error.class, CommandErrorClass::InvalidParams);
        let data = error.data.expect("missing name carries data");
        assert_eq!(data["kind"], "not_found");
        assert_eq!(data["resource"], "space");
        assert_eq!(data["suggestions"], json!([]));
    }

    #[test]
    fn wrong_case_suggests_exact_name_without_resolving() {
        let error = pick_space(vec![view("Beringlab", Uuid::new_v4())], "beringlab").unwrap_err();
        assert!(error.message.contains("did you mean 'Beringlab'?"));
        let data = error.data.expect("wrong-case name carries suggestion data");
        assert_eq!(data["suggestions"], json!(["Beringlab"]));
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
