//! Protocol-neutral directional file-change commands.

use notegate_command::{CommandError, READ_OP_CHANGES, RecoveryAction, ToolCallSpec};
use notegate_service::ServiceError;
use notegate_service::cursor;
use notegate_service::files::{
    FileChangeEvent, FileChangeEventIdCursor, ListFileChangeEventsById, SyncFileChanges,
    parse_target,
};
use serde_json::{Value, json};
use uuid::Uuid;

use super::CommandContext;
use super::resolve::{actionable_input_error, invalid_input_error, resolve_target, service_error};
use super::support::page_json;
use crate::file_change::FileChangeImpact;
use crate::state::AppState;

pub async fn call(
    state: &AppState,
    context: &CommandContext,
    purpose: &str,
    target: String,
    limit: Option<i64>,
    direction: Option<String>,
    cursor: Option<String>,
) -> Result<Value, CommandError> {
    let direction = validate_input(&target, direction.as_deref(), cursor.as_deref(), purpose)?;
    let caller = context.caller();
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
) -> Result<ChangeDirection, CommandError> {
    let direction = parse_change_direction(direction)?;
    let target = parse_target(target).map_err(|error| invalid_input_error(error.to_string()))?;
    let root_target = format!("{}:/", target.space);
    require_space_root(&target.path, &root_target)?;
    if direction == ChangeDirection::Newer {
        require_newer_cursor(cursor.map(str::to_owned), &root_target, purpose)?;
    }
    Ok(direction)
}

fn parse_change_direction(raw: Option<&str>) -> Result<ChangeDirection, CommandError> {
    match raw.unwrap_or("older") {
        "older" => Ok(ChangeDirection::Older),
        "newer" => Ok(ChangeDirection::Newer),
        _ => Err(actionable_input_error(
            "changes_direction_invalid",
            "direction must be 'older' or 'newer'",
            "Choose older for history pagination or newer for checkpoint replay.",
            RecoveryAction::ChooseValue {
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
) -> Result<String, CommandError> {
    cursor.ok_or_else(|| {
        actionable_input_error(
            "changes_cursor_required",
            "direction=newer requires cursor",
            "Capture checkpoint_cursor from the current changes page, build the Space snapshot, then replay newer changes from that cursor.",
            RecoveryAction::RebuildSnapshot {
                reason: None,
                cursor: None,
                baseline_call: Some(ToolCallSpec::new(
                    "read",
                    json!({"purpose": purpose, "op": READ_OP_CHANGES, "target": target, "limit": 1}),
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
) -> Result<Value, CommandError> {
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

    Ok(json!({
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
    }))
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
) -> Result<Value, CommandError> {
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

    Ok(json!({
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
    }))
}

fn require_space_root(path: &str, root_target: &str) -> Result<(), CommandError> {
    if path == "/" {
        return Ok(());
    }
    Err(actionable_input_error(
        "changes_scope_invalid",
        "op=changes requires a Space-root target",
        "Replace target with the resolved Space root; node and subtree filters are not supported.",
        RecoveryAction::ReplaceField {
            field: "target".to_owned(),
            value: json!(root_target),
        },
    ))
}

fn encode_change_cursor(space_id: Uuid, id: i64) -> Result<String, CommandError> {
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
) -> Result<FileChangeEventIdCursor, CommandError> {
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
) -> CommandError {
    let (hint, next_action) = match direction {
        ChangeDirection::Older => (
            "Discard this cursor and restart from the latest changes for the current Space.",
            RecoveryAction::CallTool {
                call: ToolCallSpec::new(
                    "read",
                    json!({"purpose": purpose, "op": READ_OP_CHANGES, "target": target}),
                ),
                reason: None,
                instruction: None,
            },
        ),
        ChangeDirection::Newer => (
            "This cursor cannot continue cache replay. Obtain a new checkpoint_cursor and rebuild the current Space snapshot before reading newer changes.",
            RecoveryAction::RebuildSnapshot {
                reason: None,
                cursor: None,
                baseline_call: Some(ToolCallSpec::new(
                    "read",
                    json!({"purpose": purpose, "op": READ_OP_CHANGES, "target": target, "limit": 1}),
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
) -> RecoveryAction {
    if resync_required {
        return RecoveryAction::RebuildSnapshot {
            reason: Some("The supplied cursor cannot prove continuous replay. Rebuild the current Space state and use checkpoint_cursor as the new baseline.".to_owned()),
            cursor: Some(checkpoint_cursor.to_owned()),
            baseline_call: None,
        };
    }
    if has_more {
        return RecoveryAction::CallTool {
            call: ToolCallSpec::new("read", json!({
                "purpose": purpose,
                "op": READ_OP_CHANGES,
                "target": target,
                "limit": limit,
                "direction": "newer",
                "cursor": checkpoint_cursor,
            })),
            reason: Some("More changes are available. Apply this page in order, then continue from its checkpoint_cursor.".to_owned()),
            instruction: None,
        };
    }

    RecoveryAction::StoreCursor {
        reason: "All currently available changes were returned. Store checkpoint_cursor after applying them and use it as cursor later.".to_owned(),
        cursor: checkpoint_cursor.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use chrono::{TimeZone, Utc};

    use super::*;

    const TEST_PURPOSE: &str = "synchronize test changes";

    #[test]
    fn changes_direction_defaults_to_older_and_rejects_unknown_values() {
        assert_eq!(
            parse_change_direction(None).expect("default direction"),
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
        assert_eq!(newer_data["next_action"]["kind"], "rebuild_snapshot");
        assert_eq!(
            newer_data["next_action"]["baseline_call"]["input"]["limit"],
            1
        );
    }

    #[test]
    fn change_event_keeps_public_fields_and_cache_impact() {
        let before = Uuid::from_u128(10);
        let after = Uuid::from_u128(11);
        let event = FileChangeEvent {
            id: 41,
            created_at: Utc
                .with_ymd_and_hms(2026, 8, 2, 3, 4, 5)
                .single()
                .expect("valid test timestamp"),
            space_id: Uuid::from_u128(1),
            node_id: Some(Uuid::from_u128(2)),
            actor_account_id: Some(Uuid::from_u128(3)),
            op_type: "item.move".to_owned(),
            metadata: json!({
                "item_kind": "folder",
                "parent_node_id_before": before,
                "parent_node_id_after": after,
            }),
        };

        let output = event_json(&event);
        assert_eq!(output["event_id"], 41);
        assert_eq!(output["created_at"], "2026-08-02T03:04:05Z");
        assert_eq!(output["affected_parent_ids"], json!([before, after]));
        assert_eq!(output["path_changed"], true);
        assert_eq!(output["subtree_changed"], true);
    }

    #[test]
    fn changes_next_action_keeps_cursor_protocol() {
        let continuation = json!(changes_next_action(
            "daily:/",
            "opaque-41",
            TEST_PURPOSE,
            true,
            false,
            25
        ));
        assert_eq!(continuation["kind"], "call_tool");
        assert_eq!(continuation["tool"], "read");
        assert_eq!(continuation["input"]["cursor"], "opaque-41");
        assert_eq!(continuation["input"]["purpose"], TEST_PURPOSE);

        let completed = json!(changes_next_action(
            "daily:/",
            "opaque-41",
            TEST_PURPOSE,
            false,
            false,
            25
        ));
        assert_eq!(completed["kind"], "store_cursor");
        assert_eq!(completed["cursor"], "opaque-41");

        let resync = json!(changes_next_action(
            "daily:/",
            "opaque-99",
            TEST_PURPOSE,
            false,
            true,
            25
        ));
        assert_eq!(resync["kind"], "rebuild_snapshot");
        assert_eq!(resync["cursor"], "opaque-99");
    }
}
