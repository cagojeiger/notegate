//! Directional file-change reads for the unified MCP `read` tool.

use axum::http::request::Parts;
use notegate_service::ServiceError;
use notegate_service::cursor;
use notegate_service::files::{
    FileChangeEvent, FileChangeEventIdCursor, ListFileChangeEventsById, SyncFileChanges,
};
use rmcp::{ErrorData, Json};
use serde_json::{Value, json};
use uuid::Uuid;

use super::resolve::{actionable_input_error, caller, resolve_target, service_error};
use crate::file_change::FileChangeImpact;
use crate::state::AppState;

pub async fn call(
    state: &AppState,
    parts: &Parts,
    target: String,
    limit: Option<i64>,
    before: Option<String>,
    after: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    validate_change_request(before.as_deref(), after.as_deref())?;
    let caller = caller(parts)?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let root_target = format!("{}:/", resolved.name());
    require_space_root(&path, &root_target)?;

    match after {
        Some(after) => {
            newer(
                state,
                caller.account_id(),
                resolved.space_id(),
                resolved.name(),
                target,
                limit,
                after,
            )
            .await
        }
        None => {
            older(
                state,
                caller.account_id(),
                resolved.space_id(),
                resolved.name(),
                &target,
                limit,
                before,
            )
            .await
        }
    }
}

fn validate_change_request(before: Option<&str>, after: Option<&str>) -> Result<(), ErrorData> {
    if before.is_some() && after.is_some() {
        return Err(actionable_input_error(
            "changes_direction_conflict",
            "before and after cannot be used together",
            "Choose one direction: keep before for older events or keep after for newer events.",
            json!({
                "kind": "choose_direction",
                "choices": [
                    {"keep": "before", "remove": "after", "effect": "read older events"},
                    {"keep": "after", "remove": "before", "effect": "read newer events"},
                ],
            }),
        ));
    }
    Ok(())
}

async fn older(
    state: &AppState,
    account_id: Uuid,
    space_id: Uuid,
    space_name: &str,
    target: &str,
    limit: Option<i64>,
    before: Option<String>,
) -> Result<Json<Value>, ErrorData> {
    if let Some(before) = before.as_deref() {
        decode_change_cursor(before, space_id, ChangeCursorDirection::Before, target)?;
    }
    let page = state
        .files
        .list_file_change_events_by_id(
            account_id,
            space_id,
            ListFileChangeEventsById {
                limit,
                cursor: before.clone(),
            },
        )
        .await
        .map_err(service_error)?;
    let events = page.items.iter().map(event_json).collect::<Vec<_>>();
    let returned = events.len();
    let start_cursor = page
        .items
        .first()
        .map(|event| encode_change_cursor(space_id, event.id))
        .transpose()?;
    let end_cursor = page
        .items
        .last()
        .map(|event| encode_change_cursor(space_id, event.id))
        .transpose()?;
    let head_cursor = if before.is_none() {
        Some(
            start_cursor
                .clone()
                .unwrap_or(encode_change_cursor(space_id, 0)?),
        )
    } else {
        None
    };
    let next = page.next_cursor.as_ref().map(|cursor| {
        json!({
            "before": cursor,
        })
    });

    Ok(Json(json!({
        "space": space_name,
        "path": "/",
        "scope": {
            "kind": "space",
            "includes_descendants": true,
        },
        "direction": if before.is_some() { "before" } else { "latest" },
        "order": "event_id_desc",
        "events": events,
        "page": {
            "limit": page.limit,
            "returned": returned,
            "has_more": page.has_more,
            "start_cursor": start_cursor,
            "end_cursor": end_cursor,
            "next": next,
        },
        "head_cursor": head_cursor,
        "resync_required": false,
    })))
}

#[allow(clippy::too_many_arguments)]
async fn newer(
    state: &AppState,
    account_id: Uuid,
    space_id: Uuid,
    space_name: &str,
    target: String,
    limit: Option<i64>,
    after: String,
) -> Result<Json<Value>, ErrorData> {
    let after_cursor =
        decode_change_cursor(&after, space_id, ChangeCursorDirection::After, &target)?;

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
    let applied_cursor = (!page.resync_required).then(|| continuation_cursor.clone());
    let head_cursor = page.resync_required.then(|| continuation_cursor.clone());
    let start_cursor = page
        .items
        .first()
        .map(|event| encode_change_cursor(space_id, event.id))
        .transpose()?;
    let end_cursor = page
        .items
        .last()
        .map(|event| encode_change_cursor(space_id, event.id))
        .transpose()?;
    let next = page.has_more.then(|| {
        json!({
            "after": continuation_cursor,
        })
    });
    let next_action = changes_next_action(
        &target,
        &continuation_cursor,
        page.has_more,
        page.resync_required,
        page.limit,
    );
    let returned = events.len();

    Ok(Json(json!({
        "space": space_name,
        "path": "/",
        "scope": {
            "kind": "space",
            "includes_descendants": true,
        },
        "direction": "after",
        "order": "event_id_asc",
        "events": events,
        "page": {
            "limit": page.limit,
            "returned": returned,
            "has_more": page.has_more,
            "start_cursor": start_cursor,
            "end_cursor": end_cursor,
            "next": next,
        },
        "applied_cursor": applied_cursor,
        "head_cursor": head_cursor,
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
        json!({
            "kind": "replace_field",
            "field": "target",
            "value": root_target,
        }),
    ))
}

fn encode_change_cursor(space_id: Uuid, id: i64) -> Result<String, ErrorData> {
    cursor::encode(&FileChangeEventIdCursor { space_id, id }).map_err(|_error| {
        service_error(ServiceError::Internal(
            "failed to encode change cursor".to_owned(),
        ))
    })
}

#[derive(Debug, Clone, Copy)]
enum ChangeCursorDirection {
    Before,
    After,
}

fn decode_change_cursor(
    raw: &str,
    space_id: Uuid,
    direction: ChangeCursorDirection,
    target: &str,
) -> Result<FileChangeEventIdCursor, ErrorData> {
    let decoded = cursor::decode::<FileChangeEventIdCursor>(raw).map_err(|_error| {
        changes_cursor_error(
            "changes_cursor_invalid",
            "invalid changes cursor",
            direction,
            target,
        )
    })?;
    if decoded.space_id != space_id {
        return Err(changes_cursor_error(
            "changes_cursor_scope_mismatch",
            "changes cursor does not match this Space",
            direction,
            target,
        ));
    }
    Ok(decoded)
}

fn changes_cursor_error(
    code: &'static str,
    message: &'static str,
    direction: ChangeCursorDirection,
    target: &str,
) -> ErrorData {
    let (hint, next_action) = match direction {
        ChangeCursorDirection::Before => (
            "Discard this cursor and restart from the latest changes for the current Space.",
            json!({
                "kind": "call_tool",
                "tool": "read",
                "input": {"op": "changes", "target": target},
            }),
        ),
        ChangeCursorDirection::After => (
            "This cursor cannot continue cache replay. Obtain a new head_cursor and rebuild the current Space snapshot before reading after it.",
            json!({
                "kind": "rebuild_snapshot",
                "baseline_call": {
                    "tool": "read",
                    "input": {"op": "changes", "target": target, "limit": 1},
                },
            }),
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
    applied_cursor: &str,
    has_more: bool,
    resync_required: bool,
    limit: i64,
) -> Value {
    if resync_required {
        return json!({
            "kind": "resync_required",
            "reason": "The supplied cursor cannot prove continuous replay. Rebuild the current Space state and use new_head_cursor as the new baseline.",
            "new_head_cursor": applied_cursor,
        });
    }
    if has_more {
        return json!({
            "kind": "call_tool",
            "reason": "More changes are available. Apply this page in order, then continue after its applied_cursor.",
            "tool": "read",
            "input": {
                "op": "changes",
                "target": target,
                "limit": limit,
                "after": applied_cursor,
            },
        });
    }

    json!({
        "kind": "store_cursor",
        "reason": "All currently available changes were returned. Store applied_cursor after applying them and use it as after later.",
        "after": applied_cursor,
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
    fn changes_accepts_one_direction_at_a_time() {
        assert!(validate_change_request(None, None).is_ok());
        assert!(validate_change_request(Some("before"), None).is_ok());
        assert!(validate_change_request(None, Some("after")).is_ok());
        let error = validate_change_request(Some("before"), Some("after"))
            .expect_err("both directions are ambiguous");
        let data = error.data.expect("structured recovery data");
        assert_eq!(data["code"], "changes_direction_conflict");
        assert_eq!(data["recoverable"], true);
        assert_eq!(data["next_action"]["kind"], "choose_direction");
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
        let before = decode_change_cursor(
            "not-a-cursor",
            Uuid::nil(),
            ChangeCursorDirection::Before,
            "daily:/",
        )
        .expect_err("invalid before cursor");
        let before_data = before.data.expect("before recovery data");
        assert_eq!(before_data["code"], "changes_cursor_invalid");
        assert_eq!(before_data["next_action"]["kind"], "call_tool");

        let after = decode_change_cursor(
            "not-a-cursor",
            Uuid::nil(),
            ChangeCursorDirection::After,
            "daily:/",
        )
        .expect_err("invalid after cursor");
        let after_data = after.data.expect("after recovery data");
        assert_eq!(after_data["code"], "changes_cursor_invalid");
        assert_eq!(after_data["next_action"]["kind"], "rebuild_snapshot");
        assert_eq!(
            after_data["next_action"]["baseline_call"]["input"]["limit"],
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
            "rest-test:/".to_owned(),
            Some(1),
            None,
            None,
        )
        .await?
        .0;
        assert_eq!(latest["direction"], "latest");
        assert_eq!(latest["order"], "event_id_desc");
        assert_eq!(latest["events"][0]["operation"], "folder.create");
        assert_eq!(latest["page"]["limit"], 1);
        assert_eq!(latest["page"]["returned"], 1);
        assert_eq!(latest["page"]["has_more"], true);
        assert!(latest.get("next_action").is_none());
        let head_cursor = latest["head_cursor"]
            .as_str()
            .expect("latest response exposes a cache baseline")
            .to_owned();
        let first_event_id = latest["events"][0]["event_id"]
            .as_i64()
            .expect("latest event id");
        let before_cursor = latest["page"]["next"]["before"]
            .as_str()
            .expect("older changes cursor")
            .to_owned();
        let older_history = call(
            &state,
            &parts,
            "rest-test:/".to_owned(),
            Some(1),
            Some(before_cursor),
            None,
        )
        .await?
        .0;
        assert_eq!(older_history["direction"], "before");
        assert!(
            first_event_id
                > older_history["events"][0]["event_id"]
                    .as_i64()
                    .expect("older history event id")
        );

        let subtree_error = call(
            &state,
            &parts,
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
            "rest-test:/".to_owned(),
            Some(1),
            None,
            Some(head_cursor),
        )
        .await?
        .0;
        assert_eq!(first_newer["direction"], "after");
        assert_eq!(first_newer["order"], "event_id_asc");
        assert_eq!(first_newer["events"][0]["operation"], "folder.create");
        assert_eq!(first_newer["page"]["limit"], 1);
        assert_eq!(first_newer["page"]["returned"], 1);
        assert_eq!(first_newer["page"]["has_more"], true);
        assert_eq!(first_newer["next_action"]["input"]["limit"], 1);
        assert!(first_newer["head_cursor"].is_null());
        let first_newer_event_id = first_newer["events"][0]["event_id"]
            .as_i64()
            .expect("newer event id");
        assert!(first_newer_event_id > first_event_id);
        let applied_cursor = first_newer["applied_cursor"]
            .as_str()
            .expect("applied cursor")
            .to_owned();

        let second_newer = call(
            &state,
            &parts,
            "rest-test:/".to_owned(),
            Some(1),
            None,
            Some(applied_cursor),
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
            "rest-test:/".to_owned(),
            Some(1),
            None,
            Some(invalid_cursor),
        )
        .await?
        .0;
        assert_eq!(invalid_continuation["events"], json!([]));
        assert_eq!(invalid_continuation["resync_required"], true);
        assert!(invalid_continuation["applied_cursor"].is_null());
        assert!(invalid_continuation["head_cursor"].is_string());
        assert_eq!(
            invalid_continuation["next_action"]["kind"],
            "resync_required"
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
    fn continuation_action_uses_the_last_applied_cursor() {
        let action = changes_next_action("daily:/", "opaque-41", true, false, 25);

        assert_eq!(action["kind"], "call_tool");
        assert_eq!(action["tool"], "read");
        assert_eq!(action["input"]["op"], "changes");
        assert_eq!(action["input"]["target"], "daily:/");
        assert_eq!(action["input"]["limit"], 25);
        assert_eq!(action["input"]["after"], "opaque-41");
    }

    #[test]
    fn completed_action_tells_the_caller_to_store_the_cursor() {
        let action = changes_next_action("daily:/", "opaque-41", false, false, 25);

        assert_eq!(action["kind"], "store_cursor");
        assert_eq!(action["after"], "opaque-41");
    }

    #[test]
    fn invalid_cursor_action_requires_a_full_resync() {
        let action = changes_next_action("daily:/", "opaque-99", false, true, 25);

        assert_eq!(action["kind"], "resync_required");
        assert_eq!(action["new_head_cursor"], "opaque-99");
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
