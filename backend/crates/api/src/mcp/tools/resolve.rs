//! Shared MCP tool plumbing: space-name resolution, target parsing, the
//! request-scoped [`Caller`] lookup, and the service-error → [`ErrorData`] map.
//!
//! MCP callers select a space by its human-friendly **name** (the
//! canonical name), or with a compact `target` string (`<space>:/<path>`).
//! Resolution is stateless: every tool call resolves the target space name against the
//! caller's accessible spaces (`docs/spec/mcp/README.md`). Paths are resolved
//! inside the selected space only.

use std::borrow::Cow;

use axum::http::request::Parts;
use rmcp::ErrorData;
use serde_json::json;
use uuid::Uuid;

use notegate_core::WriteLockScope;
use notegate_core::validation::{normalize_path, validate_space_name};
use notegate_model::Caller;
use notegate_search::SearchError;
use notegate_service::ServiceError;
use notegate_service::files::parse_target;
use notegate_service::spaces::SpaceView;

use crate::error::write_lock_code;
use crate::mcp::contract::{
    McpAction, McpErrorData, RequiredField, TEMPORARY_UNAVAILABLE_ERROR_CODE,
};
use crate::state::AppState;

const SPACE_SUGGESTION_LIMIT: i64 = 5;
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

/// Pure name selection over an already-loaded accessible-space list (the testable
/// core of [`select_space`]).
#[cfg(test)]
fn pick_space(accessible: Vec<SpaceView>, name: &str) -> Result<SpaceView, ErrorData> {
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

fn space_not_found_error(name: &str, suggestions: &[SpaceView]) -> ErrorData {
    let suggestions: Vec<_> = suggestions
        .iter()
        .map(|view| view.space.name.as_str())
        .collect();
    let mut message = format!("no accessible space named '{name}'");
    if let [suggestion] = suggestions.as_slice() {
        message.push_str(&format!("; did you mean '{suggestion}'?"));
    }
    ErrorData::invalid_params(
        message,
        Some(json!({
            "kind": "not_found",
            "code": "not_found",
            "resource": "space",
            "space": name,
            "suggestions": suggestions,
            "hint": "use read op=spaces to inspect accessible spaces and use the exact space name",
        })),
    )
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
        ServiceError::WriteLocked { scope } => write_locked_error(scope),
        ServiceError::UsageRecalculationInProgress {
            retry_after_seconds,
        } => ErrorData::new(
            TEMPORARY_UNAVAILABLE_ERROR_CODE,
            "space usage is being recalculated; retry shortly",
            Some(json!({
                "kind": "usage_recalculation_in_progress",
                "code": "usage_recalculation_in_progress",
                "retryable": true,
                "retry_after_seconds": retry_after_seconds,
            })),
        ),
        ServiceError::Internal(message) => {
            tracing::error!(event = "mcp.error.internal", detail = %message);
            ErrorData::internal_error("internal server error", error_meta("internal_error"))
        }
    }
}

/// Map a search-layer error through the same public MCP error contract as service failures.
pub fn search_error(error: SearchError) -> ErrorData {
    let error = match error {
        SearchError::NotFound(message) => ServiceError::NotFound(message),
        SearchError::InvalidInput(message) => ServiceError::InvalidInput(message),
        SearchError::Forbidden(message) => ServiceError::Forbidden(message),
        SearchError::Conflict(message) => ServiceError::Conflict(message),
        SearchError::WriteLocked { scope } => ServiceError::WriteLocked { scope },
        SearchError::UsageRecalculationInProgress {
            retry_after_seconds,
        } => ServiceError::UsageRecalculationInProgress {
            retry_after_seconds,
        },
        SearchError::Internal(message) => ServiceError::Internal(message),
    };
    service_error(error)
}

fn write_locked_error(scope: WriteLockScope) -> ErrorData {
    let (scope_name, hint) = match scope {
        WriteLockScope::TargetOrAncestor => (
            "target_or_ancestor",
            "Use read op=stat on the target to inspect write_lock_sources. Only the space owner can unlock it in the Dashboard. If file_upload begin_upload was rejected, unlock the target and call begin_upload again; no upload handle was created.",
        ),
        WriteLockScope::Descendant => (
            "descendant",
            "Inspect the subtree for direct write locks. Only the space owner can unlock them in the Dashboard.",
        ),
    };
    ErrorData::invalid_request(
        Cow::Owned(scope.to_string()),
        Some(json!({
            "kind": "write_locked",
            "code": write_lock_code(scope),
            "scope": scope_name,
            "retryable": false,
            "hint": hint,
        })),
    )
}

fn error_meta(kind: &'static str) -> Option<serde_json::Value> {
    Some(McpErrorData::basic(kind, kind).into_value())
}

pub fn invalid_input_error(message: impl Into<Cow<'static, str>>) -> ErrorData {
    ErrorData::invalid_params(message, error_meta("invalid_input"))
}

/// Build an invalid-input error that a caller can correct without parsing the
/// human-readable message.
pub fn actionable_input_error(
    code: &'static str,
    message: impl Into<Cow<'static, str>>,
    hint: &'static str,
    next_action: McpAction,
) -> ErrorData {
    ErrorData::invalid_params(
        message,
        Some(McpErrorData::actionable_input(code, hint, next_action).into_value()),
    )
}

/// Require an operation-specific field that cannot be expressed as globally
/// required in a unified tool's JSON Schema.
pub fn required_input<T>(value: Option<T>, field: &str, context: &str) -> Result<T, ErrorData> {
    value.ok_or_else(|| {
        actionable_input_error(
            "required_field_missing",
            format!("{context} requires {field}; retry with field `{field}` set"),
            "Add the field described by next_action.fields and retry the same tool.",
            McpAction::AddFields {
                fields: vec![RequiredField {
                    field: field.to_owned(),
                    description: None,
                }],
            },
        )
    })
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
        "default_search_enabled": view.space.default_search_enabled,
        "default_text_encryption_enabled": view.space.default_text_encryption_enabled,
        "features": {
            "text_encryption": view.features.text_encryption,
        },
    })
}

/// A path-first node summary for file tools (`list`/`stat`/`find`/mutation
/// results). Path is the canonical derived absolute path for MCP callers.
pub fn node_summary(view: &notegate_service::files::NodeView) -> serde_json::Value {
    json!(crate::path_node_summary::PathNodeSummary::from(view))
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
    use notegate_core::Config;
    use notegate_core::security::PiiCrypto;
    use notegate_core::tier::UserTier;
    use notegate_db::{AccountRepo, ApiKeyRepo, SpaceRepo, test_support::TestDb};
    use notegate_model::{CallerIdentity, Channel, CreateSpace, Permission, ResolveAttrs, Space};
    use notegate_service::files::parse_target;
    use rmcp::model::ErrorCode;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::time::Duration;

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

    fn test_state(pool: notegate_db::PgPool) -> Result<AppState, Box<dyn std::error::Error>> {
        let config = Arc::new(Config {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9191),
            search_bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9192),
            search_service_url: None,
            process_mode: notegate_core::ProcessMode::All,
            database_url: "postgres://notegate:notegate@localhost/notegate".to_owned(),
            db_max_connections: 1,
            read_database_url: None,
            read_db_max_connections: 1,
            background_jobs: notegate_core::BackgroundJobsConfig::default(),
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
            metrics_enabled: false,
            web_dist_dir: None,
            s3: crate::state::test_s3_config(),
            default_user_tier: notegate_core::tier::UserTier::DEFAULT,
            limits: notegate_core::limits::Limits::default(),
            http_rate_limits: notegate_core::HttpRateLimitsConfig::default(),
            search_body_cache: notegate_core::SearchBodyCacheConfig::default(),
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
        let resolver =
            notegate_service::identity::Resolver::new(account_repo, api_key_repo, crypto.clone());
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
        assert!(parse_target("bad/name:/notes.md").is_err());
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

        let locked = service_error(ServiceError::WriteLocked {
            scope: WriteLockScope::TargetOrAncestor,
        });
        assert_eq!(locked.code, ErrorCode::INVALID_REQUEST);
        let locked_data = locked.data.expect("write lock carries data");
        assert_eq!(locked_data["kind"], "write_locked");
        assert_eq!(locked_data["code"], "node_write_locked");
        assert_eq!(locked_data["scope"], "target_or_ancestor");
        assert_eq!(locked_data["retryable"], false);
        assert!(
            locked_data["hint"]
                .as_str()
                .is_some_and(|hint| hint.contains("begin_upload"))
        );

        let locked_subtree = service_error(ServiceError::WriteLocked {
            scope: WriteLockScope::Descendant,
        });
        let locked_subtree_data = locked_subtree.data.expect("subtree lock carries data");
        assert_eq!(locked_subtree_data["code"], "subtree_write_locked");
        assert_eq!(locked_subtree_data["scope"], "descendant");

        let internal = service_error(ServiceError::Internal("db detail".to_owned()));
        assert_eq!(internal.code, ErrorCode::INTERNAL_ERROR);
        assert_eq!(internal.message, "internal server error");
        let internal_data = internal.data.expect("internal_error carries data");
        assert_eq!(internal_data["kind"], "internal_error");
        assert_eq!(internal_data["code"], "internal_error");
    }

    #[test]
    fn search_errors_preserve_the_service_error_contract() {
        let cases = [
            (
                SearchError::NotFound("missing".to_owned()),
                ServiceError::NotFound("missing".to_owned()),
            ),
            (
                SearchError::InvalidInput("bad".to_owned()),
                ServiceError::InvalidInput("bad".to_owned()),
            ),
            (
                SearchError::Forbidden("no".to_owned()),
                ServiceError::Forbidden("no".to_owned()),
            ),
            (
                SearchError::Conflict("stale".to_owned()),
                ServiceError::Conflict("stale".to_owned()),
            ),
            (
                SearchError::WriteLocked {
                    scope: WriteLockScope::TargetOrAncestor,
                },
                ServiceError::WriteLocked {
                    scope: WriteLockScope::TargetOrAncestor,
                },
            ),
            (
                SearchError::UsageRecalculationInProgress {
                    retry_after_seconds: 5,
                },
                ServiceError::UsageRecalculationInProgress {
                    retry_after_seconds: 5,
                },
            ),
            (
                SearchError::Internal("detail".to_owned()),
                ServiceError::Internal("detail".to_owned()),
            ),
        ];

        for (search, service) in cases {
            let actual = search_error(search);
            let expected = service_error(service);
            assert_eq!(actual.code, expected.code);
            assert_eq!(actual.message, expected.message);
            assert_eq!(actual.data, expected.data);
        }
    }

    #[test]
    fn actionable_input_error_extends_the_common_error_contract() {
        let error = actionable_input_error(
            "field_not_allowed",
            "field is not allowed",
            "Remove the field and retry.",
            McpAction::RemoveFields {
                fields: vec!["field".to_owned()],
            },
        );

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let data = error.data.expect("actionable input error carries data");
        assert_eq!(data["kind"], "invalid_input");
        assert_eq!(data["code"], "field_not_allowed");
        assert_eq!(data["retryable"], false);
        assert_eq!(data["recoverable"], true);
        assert_eq!(data["hint"], "Remove the field and retry.");
        assert_eq!(data["next_action"]["kind"], "remove_fields");
    }

    #[test]
    fn usage_recalculation_is_a_retryable_server_error() {
        let error = service_error(ServiceError::UsageRecalculationInProgress {
            retry_after_seconds: 5,
        });
        assert_eq!(error.code, TEMPORARY_UNAVAILABLE_ERROR_CODE);
        let data = error.data.expect("temporary error carries retry metadata");
        assert_eq!(data["kind"], "usage_recalculation_in_progress");
        assert_eq!(data["retryable"], true);
        assert_eq!(data["retry_after_seconds"], 5);
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
        assert_eq!(data["code"], "not_found");
        assert_eq!(data["resource"], "space");
        assert_eq!(data["suggestions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn wrong_case_space_name_suggests_exact_name_without_resolving() {
        let accessible = vec![view("Beringlab", Uuid::new_v4())];
        let error = pick_space(accessible, "beringlab").unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(
            error.message.contains("did you mean 'Beringlab'?"),
            "error message should include the exact space-name suggestion: {}",
            error.message
        );
        let data = error.data.expect("wrong-case name carries suggestion data");
        assert_eq!(data["kind"], "not_found");
        assert_eq!(data["code"], "not_found");
        assert_eq!(data["resource"], "space");
        assert_eq!(data["space"], "beringlab");
        assert_eq!(data["suggestions"], json!(["Beringlab"]));
    }

    #[tokio::test]
    async fn resolve_target_wrong_case_space_name_suggests_exact_name_from_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = test_state(db.pool.clone())?;
        let (account, user) = state
            .accounts
            .upsert_user_by_sub(&ResolveAttrs {
                sub: "space-suggest-user".to_owned(),
                email: "space-suggest@example.test".to_owned(),
                name: "Space Suggest".to_owned(),
            })
            .await?;
        let space_repo = SpaceRepo::new(db.pool.clone());
        let account_id = account.id;
        let space = space_repo
            .create_space(
                account_id,
                &CreateSpace {
                    name: "Beringlab".to_owned(),
                },
            )
            .await?;
        space_repo
            .update_space(space.id, account_id, None, None, Some(true))
            .await?;
        let caller = Caller {
            account,
            identity: CallerIdentity::User(user),
            channel: Channel::Mcp,
        };

        let error = resolve_target(&state, &caller, "beringlab:/")
            .await
            .unwrap_err();
        assert!(
            error.message.contains("did you mean 'Beringlab'?"),
            "wrong-case target should suggest the exact space name: {}",
            error.message
        );
        let data = error
            .data
            .expect("wrong-case target carries suggestion data");
        assert_eq!(data["kind"], "not_found");
        assert_eq!(data["code"], "not_found");
        assert_eq!(data["resource"], "space");
        assert_eq!(data["space"], "beringlab");
        assert_eq!(data["suggestions"], json!(["Beringlab"]));

        let (resolved, path) = resolve_target(&state, &caller, "Beringlab:/").await?;
        assert_eq!(resolved.name(), "Beringlab");
        assert_eq!(path, "/");

        space_repo
            .update_space(space.id, account_id, None, None, Some(false))
            .await?;
        let error = resolve_target(&state, &caller, "Beringlab:/")
            .await
            .unwrap_err();
        let data = error.data.expect("unpinned target carries not-found data");
        assert_eq!(data["kind"], "not_found");
        assert_eq!(data["suggestions"], json!([]));

        db.cleanup().await;
        Ok(())
    }

    #[test]
    fn bad_space_name_grammar_is_rejected() {
        let error = pick_space(Vec::new(), "bad/name").unwrap_err();
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
