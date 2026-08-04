//! Directional file-change reads for the unified MCP `read` tool.

use axum::http::request::Parts;
use notegate_service::ServiceError;
use notegate_service::cursor;
use notegate_service::files::{
    FileChangeEvent, FileChangeEventIdCursor, ListFileChangeEventsById, SyncFileChanges,
    parse_target,
};
use rmcp::{ErrorData, Json};
use serde_json::{Value, json};
use uuid::Uuid;

use super::resolve::{
    actionable_input_error, caller, invalid_input_error, resolve_target, service_error,
};
use super::support::page_json;
use crate::file_change::FileChangeImpact;
use crate::mcp::contract::{McpAction, ToolCallSpec};
use crate::state::AppState;

pub async fn call(
    state: &AppState,
    parts: &Parts,
    purpose: &str,
    target: String,
    limit: Option<i64>,
    direction: Option<String>,
    cursor: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    let direction = validate_input(&target, direction.as_deref(), cursor.as_deref(), purpose)?;
    let caller = caller(parts)?;
    let (resolved, _path) = resolve_target(state, caller, &target).await?;
    let root_target = format!("{}:/", resolved.name());

    match direction {
        ChangeDirection::Newer => {
            newer(
                state,
                caller.account_id(),
                resolved.space_id(),
                resolved.name(),
                purpose,
                target,
                limit,
                require_newer_cursor(cursor, &root_target, purpose)?,
            )
            .await
        }
        ChangeDirection::Older => {
            older(
                state,
                caller.account_id(),
                resolved.space_id(),
                resolved.name(),
                purpose,
                &target,
                limit,
                cursor,
            )
            .await
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChangeDirection {
    Older,
    Newer,
}

pub(super) fn validate_input(
    target: &str,
    direction: Option<&str>,
    cursor: Option<&str>,
    purpose: &str,
) -> Result<ChangeDirection, ErrorData> {
    let direction = parse_change_direction(direction)?;
    let target = parse_target(target).map_err(|error| invalid_input_error(error.to_string()))?;
    let root_target = format!("{}:/", target.space);
    require_space_root(&target.path, &root_target)?;
    if direction == ChangeDirection::Newer {
        require_newer_cursor(cursor.map(str::to_owned), &root_target, purpose)?;
    }
    Ok(direction)
}

fn parse_change_direction(raw: Option<&str>) -> Result<ChangeDirection, ErrorData> {
    match raw.unwrap_or("older") {
        "older" => Ok(ChangeDirection::Older),
        "newer" => Ok(ChangeDirection::Newer),
        _ => Err(actionable_input_error(
            "changes_direction_invalid",
            "direction must be 'older' or 'newer'",
            "Choose older for history pagination or newer for checkpoint replay.",
            McpAction::ChooseValue {
                field: "direction".to_owned(),
                choices: vec![json!("older"), json!("newer")],
            },
        )),
    }
}

fn require_newer_cursor(
    cursor: Option<String>,
    target: &str,
    purpose: &str,
) -> Result<String, ErrorData> {
    cursor.ok_or_else(|| {
        actionable_input_error(
            "changes_cursor_required",
            "direction=newer requires cursor",
            "Capture checkpoint_cursor from the current changes page, build the Space snapshot, then replay newer changes from that cursor.",
            McpAction::RebuildSnapshot {
                reason: None,
                cursor: None,
                baseline_call: Some(ToolCallSpec::new(
                    "read",
                    json!({"purpose": purpose, "op": "changes", "target": target, "limit": 1}),
                )),
            },
        )
    })
}

#[allow(clippy::too_many_arguments)]
async fn older(
    state: &AppState,
    account_id: Uuid,
    space_id: Uuid,
    space_name: &str,
    purpose: &str,
    target: &str,
    limit: Option<i64>,
    cursor: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    if let Some(cursor) = cursor.as_deref() {
        decode_change_cursor(cursor, space_id, ChangeDirection::Older, target, purpose)?;
    }
    let page = state
        .files
        .list_file_change_events_by_id(
            account_id,
            space_id,
            ListFileChangeEventsById {
                limit,
                cursor: cursor.clone(),
            },
        )
        .await
        .map_err(service_error)?;
    let events = page.items.iter().map(event_json).collect::<Vec<_>>();
    let returned = events.len();
    let checkpoint_cursor = if cursor.is_none() {
        Some(
            page.items
                .first()
                .map(|event| encode_change_cursor(space_id, event.id))
                .transpose()?
                .unwrap_or(encode_change_cursor(space_id, 0)?),
        )
    } else {
        None
    };
    let page_json = page_json(
        page.limit,
        returned,
        page.has_more,
        page.next_cursor.as_deref(),
    );

    Ok(Json(json!({
        "space": space_name,
        "path": "/",
        "scope": {
            "kind": "space",
            "includes_descendants": true,
        },
        "direction": "older",
        "order": "event_id_desc",
        "events": events,
        "page": page_json,
        "checkpoint_cursor": checkpoint_cursor,
        "resync_required": false,
    })))
}

#[allow(clippy::too_many_arguments)]
async fn newer(
    state: &AppState,
    account_id: Uuid,
    space_id: Uuid,
    space_name: &str,
    purpose: &str,
    target: String,
    limit: Option<i64>,
    cursor: String,
) -> Result<Json<Value>, ErrorData> {
    let after_cursor =
        decode_change_cursor(&cursor, space_id, ChangeDirection::Newer, &target, purpose)?;

    let page = state
        .files
        .sync_file_changes(
            account_id,
            space_id,
            SyncFileChanges {
                after_id: Some(after_cursor.id),
                limit,
            },
        )
        .await
        .map_err(service_error)?;
    let events = page.items.iter().map(event_json).collect::<Vec<_>>();
    let continuation_cursor = encode_change_cursor(space_id, page.next_after_id)?;
    let next_cursor = page.has_more.then_some(continuation_cursor.as_str());
    let page_json = page_json(page.limit, events.len(), page.has_more, next_cursor);
    let next_action = changes_next_action(
        &target,
        &continuation_cursor,
        purpose,
        page.has_more,
        page.resync_required,
        page.limit,
    );

    Ok(Json(json!({
        "space": space_name,
        "path": "/",
        "scope": {
            "kind": "space",
            "includes_descendants": true,
        },
        "direction": "newer",
        "order": "event_id_asc",
        "events": events,
        "page": page_json,
        "checkpoint_cursor": continuation_cursor,
        "resync_required": page.resync_required,
        "next_action": next_action,
    })))
}

fn require_space_root(path: &str, root_target: &str) -> Result<(), ErrorData> {
    if path == "/" {
        return Ok(());
    }
    Err(actionable_input_error(
        "changes_scope_invalid",
        "op=changes requires a Space-root target",
        "Replace target with the resolved Space root; node and subtree filters are not supported.",
        McpAction::ReplaceField {
            field: "target".to_owned(),
            value: json!(root_target),
        },
    ))
}

fn encode_change_cursor(space_id: Uuid, id: i64) -> Result<String, ErrorData> {
    cursor::encode(&FileChangeEventIdCursor { space_id, id }).map_err(|_error| {
        service_error(ServiceError::Internal(
            "failed to encode change cursor".to_owned(),
        ))
    })
}

fn decode_change_cursor(
    raw: &str,
    space_id: Uuid,
    direction: ChangeDirection,
    target: &str,
    purpose: &str,
) -> Result<FileChangeEventIdCursor, ErrorData> {
    let decoded = cursor::decode::<FileChangeEventIdCursor>(raw).map_err(|_error| {
        changes_cursor_error(
            "changes_cursor_invalid",
            "invalid changes cursor",
            direction,
            target,
            purpose,
        )
    })?;
    if decoded.space_id != space_id {
        return Err(changes_cursor_error(
            "changes_cursor_scope_mismatch",
            "changes cursor does not match this Space",
            direction,
            target,
            purpose,
        ));
    }
    Ok(decoded)
}

fn changes_cursor_error(
    code: &'static str,
    message: &'static str,
    direction: ChangeDirection,
    target: &str,
    purpose: &str,
) -> ErrorData {
    let (hint, next_action) = match direction {
        ChangeDirection::Older => (
            "Discard this cursor and restart from the latest changes for the current Space.",
            McpAction::CallTool {
                call: ToolCallSpec::new(
                    "read",
                    json!({"purpose": purpose, "op": "changes", "target": target}),
                ),
                reason: None,
                instruction: None,
            },
        ),
        ChangeDirection::Newer => (
            "This cursor cannot continue cache replay. Obtain a new checkpoint_cursor and rebuild the current Space snapshot before reading newer changes.",
            McpAction::RebuildSnapshot {
                reason: None,
                cursor: None,
                baseline_call: Some(ToolCallSpec::new(
                    "read",
                    json!({"purpose": purpose, "op": "changes", "target": target, "limit": 1}),
                )),
            },
        ),
    };
    actionable_input_error(code, message, hint, next_action)
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

fn changes_next_action(
    target: &str,
    checkpoint_cursor: &str,
    purpose: &str,
    has_more: bool,
    resync_required: bool,
    limit: i64,
) -> McpAction {
    if resync_required {
        return McpAction::RebuildSnapshot {
            reason: Some("The supplied cursor cannot prove continuous replay. Rebuild the current Space state and use checkpoint_cursor as the new baseline.".to_owned()),
            cursor: Some(checkpoint_cursor.to_owned()),
            baseline_call: None,
        };
    }
    if has_more {
        return McpAction::CallTool {
            call: ToolCallSpec::new("read", json!({
                "purpose": purpose,
                "op": "changes",
                "target": target,
                "limit": limit,
                "direction": "newer",
                "cursor": checkpoint_cursor,
            })),
            reason: Some("More changes are available. Apply this page in order, then continue from its checkpoint_cursor.".to_owned()),
            instruction: None,
        };
    }

    McpAction::StoreCursor {
        reason: "All currently available changes were returned. Store checkpoint_cursor after applying them and use it as cursor later.".to_owned(),
        cursor: checkpoint_cursor.to_owned(),
    }
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

    const TEST_PURPOSE: &str = "synchronize test changes";

    #[test]
    fn changes_direction_defaults_to_older_and_rejects_unknown_values() {
        assert_eq!(
            parse_change_direction(None).expect("default direction"),
            ChangeDirection::Older
        );
        assert_eq!(
            parse_change_direction(Some("older")).expect("older direction"),
            ChangeDirection::Older
        );
        assert_eq!(
            parse_change_direction(Some("newer")).expect("newer direction"),
            ChangeDirection::Newer
        );
        let error = parse_change_direction(Some("latest")).expect_err("unknown direction");
        let data = error.data.expect("structured recovery data");
        assert_eq!(data["code"], "changes_direction_invalid");
        assert_eq!(data["recoverable"], true);
        assert_eq!(data["next_action"]["kind"], "choose_value");
    }

    #[test]
    fn newer_direction_requires_a_checkpoint_cursor() {
        let error =
            require_newer_cursor(None, "daily:/", TEST_PURPOSE).expect_err("cursor is required");
        let data = error.data.expect("structured recovery data");
        assert_eq!(data["code"], "changes_cursor_required");
        assert_eq!(data["next_action"]["kind"], "rebuild_snapshot");
    }

    #[test]
    fn changes_requires_a_space_root_path() {
        assert!(require_space_root("/", "daily:/").is_ok());
        let error =
            require_space_root("/folder", "daily:/").expect_err("filtered changes are rejected");
        let data = error.data.expect("structured recovery data");
        assert_eq!(data["code"], "changes_scope_invalid");
        assert_eq!(data["next_action"]["field"], "target");
        assert_eq!(data["next_action"]["value"], "daily:/");
    }

    #[test]
    fn invalid_cursors_describe_direction_specific_recovery() {
        let older = decode_change_cursor(
            "not-a-cursor",
            Uuid::nil(),
            ChangeDirection::Older,
            "daily:/",
            TEST_PURPOSE,
        )
        .expect_err("invalid older cursor");
        let older_data = older.data.expect("older recovery data");
        assert_eq!(older_data["code"], "changes_cursor_invalid");
        assert_eq!(older_data["next_action"]["kind"], "call_tool");

        let newer = decode_change_cursor(
            "not-a-cursor",
            Uuid::nil(),
            ChangeDirection::Newer,
            "daily:/",
            TEST_PURPOSE,
        )
        .expect_err("invalid newer cursor");
        let newer_data = newer.data.expect("newer recovery data");
        assert_eq!(newer_data["code"], "changes_cursor_invalid");
        assert_eq!(newer_data["next_action"]["kind"], "rebuild_snapshot");
        assert_eq!(
            newer_data["next_action"]["baseline_call"]["input"]["limit"],
            1
        );
    }

    #[tokio::test]
    async fn changes_reads_latest_older_and_newer_from_one_cursor_stream()
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

        let latest = call(
            &state,
            &parts,
            TEST_PURPOSE,
            "rest-test:/".to_owned(),
            Some(1),
            None,
            None,
        )
        .await?
        .0;
        assert_eq!(latest["direction"], "older");
        assert_eq!(latest["order"], "event_id_desc");
        assert_eq!(latest["events"][0]["operation"], "folder.create");
        assert_eq!(latest["page"]["limit"], 1);
        assert_eq!(latest["page"]["returned"], 1);
        assert_eq!(latest["page"]["has_more"], true);
        assert!(latest.get("next_action").is_none());
        let checkpoint_cursor = latest["checkpoint_cursor"]
            .as_str()
            .expect("latest response exposes a cache baseline")
            .to_owned();
        let first_event_id = latest["events"][0]["event_id"]
            .as_i64()
            .expect("latest event id");
        let older_cursor = latest["page"]["next_cursor"]
            .as_str()
            .expect("older changes cursor")
            .to_owned();
        let older_history = call(
            &state,
            &parts,
            TEST_PURPOSE,
            "rest-test:/".to_owned(),
            Some(1),
            Some("older".to_owned()),
            Some(older_cursor),
        )
        .await?
        .0;
        assert_eq!(older_history["direction"], "older");
        assert!(
            first_event_id
                > older_history["events"][0]["event_id"]
                    .as_i64()
                    .expect("older history event id")
        );

        let subtree_error = call(
            &state,
            &parts,
            TEST_PURPOSE,
            "rest-test:/before-a".to_owned(),
            None,
            None,
            None,
        )
        .await
        .err()
        .expect("changes requires a Space root");
        assert!(subtree_error.message.contains("Space-root"));

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
        let first_newer = call(
            &state,
            &parts,
            TEST_PURPOSE,
            "rest-test:/".to_owned(),
            Some(1),
            Some("newer".to_owned()),
            Some(checkpoint_cursor),
        )
        .await?
        .0;
        assert_eq!(first_newer["direction"], "newer");
        assert_eq!(first_newer["order"], "event_id_asc");
        assert_eq!(first_newer["events"][0]["operation"], "folder.create");
        assert_eq!(first_newer["page"]["limit"], 1);
        assert_eq!(first_newer["page"]["returned"], 1);
        assert_eq!(first_newer["page"]["has_more"], true);
        assert_eq!(first_newer["next_action"]["input"]["limit"], 1);
        assert!(first_newer["checkpoint_cursor"].is_string());
        let first_newer_event_id = first_newer["events"][0]["event_id"]
            .as_i64()
            .expect("newer event id");
        assert!(first_newer_event_id > first_event_id);
        let checkpoint_cursor = first_newer["checkpoint_cursor"]
            .as_str()
            .expect("checkpoint cursor")
            .to_owned();

        let second_newer = call(
            &state,
            &parts,
            TEST_PURPOSE,
            "rest-test:/".to_owned(),
            Some(1),
            Some("newer".to_owned()),
            Some(checkpoint_cursor),
        )
        .await?
        .0;
        assert_eq!(second_newer["page"]["limit"], 1);
        assert_eq!(second_newer["page"]["returned"], 1);
        assert_eq!(second_newer["page"]["has_more"], false);
        let second_newer_event_id = second_newer["events"][0]["event_id"]
            .as_i64()
            .expect("second newer event id");
        assert!(first_newer_event_id < second_newer_event_id);

        let invalid_cursor = encode_change_cursor(space_id, second_newer_event_id + 1000)?;
        let invalid_continuation = call(
            &state,
            &parts,
            TEST_PURPOSE,
            "rest-test:/".to_owned(),
            Some(1),
            Some("newer".to_owned()),
            Some(invalid_cursor),
        )
        .await?
        .0;
        assert_eq!(invalid_continuation["events"], json!([]));
        assert_eq!(invalid_continuation["resync_required"], true);
        assert!(invalid_continuation["checkpoint_cursor"].is_string());
        assert_eq!(
            invalid_continuation["next_action"]["kind"],
            "rebuild_snapshot"
        );

        db.cleanup().await;
        Ok(())
    }

    #[test]
    fn change_event_names_the_event_id_and_time_explicitly() {
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
    fn continuation_action_uses_the_checkpoint_cursor() {
        let action = json!(changes_next_action(
            "daily:/",
            "opaque-41",
            TEST_PURPOSE,
            true,
            false,
            25
        ));

        assert_eq!(action["kind"], "call_tool");
        assert_eq!(action["tool"], "read");
        assert_eq!(action["input"]["op"], "changes");
        assert_eq!(action["input"]["target"], "daily:/");
        assert_eq!(action["input"]["limit"], 25);
        assert_eq!(action["input"]["direction"], "newer");
        assert_eq!(action["input"]["cursor"], "opaque-41");
        assert_eq!(action["input"]["purpose"], TEST_PURPOSE);
    }

    #[test]
    fn completed_action_tells_the_caller_to_store_the_cursor() {
        let action = json!(changes_next_action(
            "daily:/",
            "opaque-41",
            TEST_PURPOSE,
            false,
            false,
            25
        ));

        assert_eq!(action["kind"], "store_cursor");
        assert_eq!(action["cursor"], "opaque-41");
    }

    #[test]
    fn invalid_cursor_action_requires_a_full_resync() {
        let action = json!(changes_next_action(
            "daily:/",
            "opaque-99",
            TEST_PURPOSE,
            false,
            true,
            25
        ));

        assert_eq!(action["kind"], "rebuild_snapshot");
        assert_eq!(action["cursor"], "opaque-99");
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
