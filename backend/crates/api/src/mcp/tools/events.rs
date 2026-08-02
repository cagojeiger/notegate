//! File change history and forward-sync handlers for the unified MCP `read` tool.

use axum::http::request::Parts;
use notegate_service::files::{FileChangeEvent, ListFileChangeEventsById, SyncFileChanges};
use rmcp::{ErrorData, Json};
use serde_json::{Value, json};

use super::resolve::{caller, invalid_input_error, resolve_target, service_error};
use super::support::page_json;
use super::unified::ChangeMode;
use crate::file_change::FileChangeImpact;
use crate::state::AppState;

pub async fn call(
    state: &AppState,
    parts: &Parts,
    target: String,
    mode: ChangeMode,
    limit: Option<i64>,
    cursor: Option<String>,
    after_event_id: Option<i64>,
) -> Result<Json<Value>, ErrorData> {
    validate_change_request(mode, cursor.as_deref(), after_event_id)?;
    match mode {
        ChangeMode::History => history(state, parts, target, limit, cursor).await,
        ChangeMode::Sync => sync(state, parts, target, after_event_id, limit).await,
    }
}

fn validate_change_request(
    mode: ChangeMode,
    cursor: Option<&str>,
    after_event_id: Option<i64>,
) -> Result<(), ErrorData> {
    match mode {
        ChangeMode::History if after_event_id.is_some() => Err(invalid_input_error(
            "changes mode=history uses cursor, not after_event_id",
        )),
        ChangeMode::Sync if cursor.is_some() => Err(invalid_input_error(
            "changes mode=sync uses after_event_id, not cursor",
        )),
        ChangeMode::Sync if after_event_id.is_some_and(|event_id| event_id < 0) => Err(
            invalid_input_error("after_event_id must be zero or greater"),
        ),
        _ => Ok(()),
    }
}

async fn history(
    state: &AppState,
    parts: &Parts,
    target: String,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let node_id = if path == "/" {
        None
    } else {
        Some(
            state
                .files
                .resolve_path(caller.account_id(), resolved.space_id(), &path)
                .await
                .map_err(service_error)?
                .node
                .id,
        )
    };
    let page = state
        .files
        .list_file_change_events_by_id(
            caller.account_id(),
            resolved.space_id(),
            ListFileChangeEventsById {
                node_id,
                limit,
                cursor,
            },
        )
        .await
        .map_err(service_error)?;
    let events = page.items.iter().map(event_json).collect::<Vec<_>>();
    let returned = events.len();
    Ok(Json(json!({
        "mode": "history",
        "space": resolved.name(),
        "path": path,
        "scope": {
            "kind": if node_id.is_some() { "node" } else { "space" },
            "includes_descendants": node_id.is_none(),
        },
        "order": "event_id_desc",
        "events": events,
        "page": page_json(
            page.limit,
            returned,
            page.has_more,
            page.next_cursor.as_deref(),
        ),
    })))
}

async fn sync(
    state: &AppState,
    parts: &Parts,
    target: String,
    after_event_id: Option<i64>,
    limit: Option<i64>,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    require_sync_root(&path)?;

    let page = state
        .files
        .sync_file_changes(
            caller.account_id(),
            resolved.space_id(),
            SyncFileChanges {
                after_id: after_event_id,
                limit,
            },
        )
        .await
        .map_err(service_error)?;
    let events = page.items.iter().map(event_json).collect::<Vec<_>>();
    let next_action = sync_next_action(
        &target,
        page.next_after_id,
        page.has_more,
        page.resync_required,
        after_event_id.is_none(),
        page.limit,
    );
    let returned = events.len();

    Ok(Json(json!({
        "mode": "sync",
        "space": resolved.name(),
        "path": path,
        "scope": {
            "kind": "space",
            "includes_descendants": true,
        },
        "order": "event_id_asc",
        "events": events,
        "batch": {
            "limit": page.limit,
            "returned": returned,
            "has_more": page.has_more,
        },
        "checkpoint": {
            "input_after_event_id": after_event_id,
            "next_after_event_id": page.next_after_id,
        },
        "resync_required": page.resync_required,
        "next_action": next_action,
    })))
}

fn require_sync_root(path: &str) -> Result<(), ErrorData> {
    if path == "/" {
        return Ok(());
    }
    Err(invalid_input_error(
        "changes mode=sync requires a Space-root target such as `my-space:/`; subtree sync is not supported",
    ))
}

fn event_json(event: &FileChangeEvent) -> Value {
    let impact = FileChangeImpact::from_event(event);
    json!({
        "event_id": event.id,
        "created_at": event.created_at,
        "node_id": event.node_id,
        "actor_account_id": event.actor_account_id,
        "operation": event.op_type,
        "metadata": event.metadata,
        "item_kind": impact.item_kind,
        "affected_parent_ids": impact.affected_parent_ids,
        "parent_scope_known": impact.parent_scope_known,
        "path_changed": impact.path_changed,
        "subtree_changed": impact.subtree_changed,
        "write_lock_changed": impact.write_lock_changed,
    })
}

fn sync_next_action(
    target: &str,
    next_after_event_id: i64,
    has_more: bool,
    resync_required: bool,
    established_baseline: bool,
    limit: i64,
) -> Value {
    if resync_required {
        return json!({
            "kind": "resync_required",
            "reason": "The supplied checkpoint cannot prove lossless continuity. Rebuild the current Space state, then store checkpoint.next_after_event_id as the new baseline.",
            "new_baseline_event_id": next_after_event_id,
        });
    }
    if has_more {
        return json!({
            "kind": "call_tool",
            "reason": "More changes are available. Apply this page in order, then continue from its checkpoint.",
            "tool": "read",
            "input": {
                "op": "changes",
                "target": target,
                "mode": "sync",
                "limit": limit,
                "after_event_id": next_after_event_id,
            },
        });
    }
    if established_baseline {
        return json!({
            "kind": "store_checkpoint",
            "reason": "No past events are returned when establishing a baseline. Store this event id. If initializing a cache, read the current Space state now, then call changes with this id to catch changes that occurred while reading that snapshot.",
            "after_event_id": next_after_event_id,
        });
    }

    json!({
        "kind": "store_checkpoint",
        "reason": "All currently available changes were returned. Store this event id after applying them and use it as after_event_id later.",
        "after_event_id": next_after_event_id,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use chrono::{TimeZone, Utc};
    use notegate_db::{SpaceRepo, test_support::TestDb};
    use notegate_service::files::CreateFolder;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn change_modes_reject_the_other_modes_continuation_token() {
        let history = validate_change_request(ChangeMode::History, None, Some(41))
            .expect_err("history rejects a sync checkpoint");
        assert!(history.message.contains("uses cursor"));

        let sync = validate_change_request(ChangeMode::Sync, Some("cursor"), None)
            .expect_err("sync rejects a history cursor");
        assert!(sync.message.contains("uses after_event_id"));

        let negative = validate_change_request(ChangeMode::Sync, None, Some(-1))
            .expect_err("sync rejects a negative checkpoint");
        assert!(negative.message.contains("zero or greater"));
    }

    #[test]
    fn sync_requires_a_space_root_path() {
        assert!(require_sync_root("/").is_ok());
        let error = require_sync_root("/folder").expect_err("subtree sync is rejected");
        assert!(error.message.contains("mode=sync"));
    }

    #[tokio::test]
    async fn history_and_sync_read_the_same_mutation_event_stream()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, space_id, root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;
        SpaceRepo::new(state.db.clone())
            .update_space(space_id, caller.account_id(), None, None, Some(true))
            .await?;
        let mut parts = axum::http::Request::new(()).into_parts().0;
        parts.extensions.insert(caller.clone());

        for name in ["before-a", "before-b"] {
            state
                .files
                .create_folder(
                    caller.account_id(),
                    space_id,
                    CreateFolder {
                        parent_node_id: root_id,
                        name: name.to_owned(),
                    },
                )
                .await?;
        }

        let history = call(
            &state,
            &parts,
            "rest-test:/".to_owned(),
            ChangeMode::History,
            Some(1),
            None,
            None,
        )
        .await?
        .0;
        assert_eq!(history["mode"], "history");
        assert_eq!(history["order"], "event_id_desc");
        assert_eq!(history["events"][0]["operation"], "folder.create");
        assert_eq!(history["page"]["limit"], 1);
        assert_eq!(history["page"]["returned"], 1);
        assert_eq!(history["page"]["has_more"], true);
        assert!(history.get("next_action").is_none());
        let history_cursor = history["page"]["next_cursor"]
            .as_str()
            .expect("history cursor")
            .to_owned();
        let first_history_id = history["events"][0]["event_id"]
            .as_i64()
            .expect("history event id");
        let mismatched_history_cursor = call(
            &state,
            &parts,
            "rest-test:/before-a".to_owned(),
            ChangeMode::History,
            Some(1),
            Some(history_cursor.clone()),
            None,
        )
        .await
        .err()
        .expect("history cursor is bound to its Space and node scope");
        assert!(
            mismatched_history_cursor
                .message
                .contains("does not match this scope")
        );
        let older_history = call(
            &state,
            &parts,
            "rest-test:/".to_owned(),
            ChangeMode::History,
            Some(1),
            Some(history_cursor),
            None,
        )
        .await?
        .0;
        assert!(
            first_history_id
                > older_history["events"][0]["event_id"]
                    .as_i64()
                    .expect("older history event id")
        );

        let history_checkpoint_error = call(
            &state,
            &parts,
            "rest-test:/".to_owned(),
            ChangeMode::History,
            None,
            None,
            Some(first_history_id),
        )
        .await
        .err()
        .expect("history rejects sync checkpoints");
        assert!(history_checkpoint_error.message.contains("uses cursor"));

        let sync_cursor_error = call(
            &state,
            &parts,
            "rest-test:/".to_owned(),
            ChangeMode::Sync,
            None,
            Some("history-cursor".to_owned()),
            None,
        )
        .await
        .err()
        .expect("sync rejects history cursors");
        assert!(sync_cursor_error.message.contains("uses after_event_id"));

        let subtree_sync_error = call(
            &state,
            &parts,
            "rest-test:/before-a".to_owned(),
            ChangeMode::Sync,
            None,
            None,
            None,
        )
        .await
        .err()
        .expect("sync requires a Space root");
        assert!(subtree_sync_error.message.contains("mode=sync"));

        let baseline = call(
            &state,
            &parts,
            "rest-test:/".to_owned(),
            ChangeMode::Sync,
            Some(1),
            None,
            None,
        )
        .await?
        .0;
        assert_eq!(baseline["events"], json!([]));
        assert_eq!(baseline["batch"]["limit"], 1);
        assert_eq!(baseline["batch"]["returned"], 0);
        assert_eq!(baseline["batch"]["has_more"], false);
        let baseline_id = baseline["checkpoint"]["next_after_event_id"]
            .as_i64()
            .expect("baseline event id");

        for name in ["after-a", "after-b"] {
            state
                .files
                .create_folder(
                    caller.account_id(),
                    space_id,
                    CreateFolder {
                        parent_node_id: root_id,
                        name: name.to_owned(),
                    },
                )
                .await?;
        }
        let first_sync = call(
            &state,
            &parts,
            "rest-test:/".to_owned(),
            ChangeMode::Sync,
            Some(1),
            None,
            Some(baseline_id),
        )
        .await?
        .0;
        assert_eq!(first_sync["mode"], "sync");
        assert_eq!(first_sync["order"], "event_id_asc");
        assert_eq!(first_sync["events"][0]["operation"], "folder.create");
        assert_eq!(first_sync["batch"]["limit"], 1);
        assert_eq!(first_sync["batch"]["returned"], 1);
        assert_eq!(first_sync["batch"]["has_more"], true);
        assert_eq!(first_sync["next_action"]["input"]["limit"], 1);
        let first_synced_event_id = first_sync["events"][0]["event_id"]
            .as_i64()
            .expect("synced event id");
        assert!(first_synced_event_id > baseline_id);

        let second_sync = call(
            &state,
            &parts,
            "rest-test:/".to_owned(),
            ChangeMode::Sync,
            Some(1),
            None,
            Some(first_synced_event_id),
        )
        .await?
        .0;
        assert_eq!(second_sync["batch"]["limit"], 1);
        assert_eq!(second_sync["batch"]["returned"], 1);
        assert_eq!(second_sync["batch"]["has_more"], false);
        let second_synced_event_id = second_sync["events"][0]["event_id"]
            .as_i64()
            .expect("second synced event id");
        assert!(first_synced_event_id < second_synced_event_id);

        let invalid_checkpoint = call(
            &state,
            &parts,
            "rest-test:/".to_owned(),
            ChangeMode::Sync,
            Some(1),
            None,
            Some(second_synced_event_id + 1000),
        )
        .await?
        .0;
        assert_eq!(invalid_checkpoint["events"], json!([]));
        assert_eq!(invalid_checkpoint["resync_required"], true);
        assert_eq!(invalid_checkpoint["next_action"]["kind"], "resync_required");

        db.cleanup().await;
        Ok(())
    }

    #[test]
    fn history_event_names_the_event_id_and_time_explicitly() {
        let event = event("text.write", json!({ "item_kind": "text" }));

        let output = event_json(&event);

        assert_eq!(output["event_id"], 41);
        assert_eq!(output["created_at"], "2026-08-02T03:04:05Z");
        assert_eq!(output["operation"], "text.write");
        assert_eq!(output["metadata"]["item_kind"], "text");
    }

    #[test]
    fn change_event_exposes_cache_invalidation_impact() {
        let before = Uuid::from_u128(10);
        let after = Uuid::from_u128(11);
        let event = event(
            "item.move",
            json!({
                "item_kind": "folder",
                "parent_node_id_before": before,
                "parent_node_id_after": after,
            }),
        );

        let output = event_json(&event);

        assert_eq!(output["event_id"], 41);
        assert_eq!(output["affected_parent_ids"], json!([before, after]));
        assert_eq!(output["parent_scope_known"], true);
        assert_eq!(output["path_changed"], true);
        assert_eq!(output["subtree_changed"], true);
        assert_eq!(output["write_lock_changed"], false);
    }

    #[test]
    fn baseline_action_tells_the_caller_to_store_the_checkpoint() {
        let action = sync_next_action("daily:/", 41, false, false, true, 25);

        assert_eq!(action["kind"], "store_checkpoint");
        assert_eq!(action["after_event_id"], 41);
        assert!(
            action["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("current Space state"))
        );
    }

    #[test]
    fn continuation_action_uses_the_last_returned_event_id() {
        let action = sync_next_action("daily:/", 41, true, false, false, 25);

        assert_eq!(action["kind"], "call_tool");
        assert_eq!(action["tool"], "read");
        assert_eq!(action["input"]["op"], "changes");
        assert_eq!(action["input"]["mode"], "sync");
        assert_eq!(action["input"]["target"], "daily:/");
        assert_eq!(action["input"]["limit"], 25);
        assert_eq!(action["input"]["after_event_id"], 41);
    }

    #[test]
    fn invalid_checkpoint_action_requires_a_full_resync() {
        let action = sync_next_action("daily:/", 99, false, true, false, 25);

        assert_eq!(action["kind"], "resync_required");
        assert_eq!(action["new_baseline_event_id"], 99);
    }

    fn event(operation: &str, metadata: Value) -> FileChangeEvent {
        FileChangeEvent {
            id: 41,
            created_at: Utc
                .with_ymd_and_hms(2026, 8, 2, 3, 4, 5)
                .single()
                .expect("valid test timestamp"),
            space_id: Uuid::from_u128(1),
            node_id: Some(Uuid::from_u128(2)),
            actor_account_id: Some(Uuid::from_u128(3)),
            op_type: operation.to_owned(),
            metadata,
        }
    }
}
