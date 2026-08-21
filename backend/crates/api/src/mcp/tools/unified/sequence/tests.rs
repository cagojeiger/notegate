#![allow(clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[test]
fn purpose_is_required_for_direct_and_sequence_calls() {
    assert!(
        serde_json::from_value::<SearchInput>(json!({
            "op": "find",
            "target": "daily:/",
            "q": "cache"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<RunReadSequenceInput>(json!({
            "commands": [{"tool": "read", "op": "spaces"}]
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<RunWriteSequenceInput>(json!({
            "commands": [{"tool": "manage", "op": "mkdir", "target": "daily:/notes"}]
        }))
        .is_err()
    );
}

#[test]
fn sequence_kinds_accept_only_their_tool_family() {
    let read_error = prepare_sequence_commands(
        vec![json!({
            "tool": "write",
            "op": "write",
            "target": "daily:/note.md",
            "content": "body"
        })],
        "read notes",
        SequenceKind::Read,
    )
    .expect_err("read sequence must reject mutations");
    let write_error = prepare_sequence_commands(
        vec![json!({"tool": "read", "op": "spaces"})],
        "create notes",
        SequenceKind::Write,
    )
    .expect_err("write sequence must reject reads");

    assert_eq!(
        read_error.data.expect("read error data")["errors"][0]["code"],
        "invalid_sequence_tool"
    );
    assert_eq!(
        write_error.data.expect("write error data")["errors"][0]["code"],
        "invalid_sequence_tool"
    );
}

#[test]
fn valid_commands_preserve_input_order_and_fields() {
    let commands = prepare_sequence_commands(
        vec![
            json!({"tool": "read", "op": "spaces", "limit": 5}),
            json!({
                "tool": "search",
                "op": "grep",
                "target": "daily:/",
                "q": "cache",
                "lines": "first"
            }),
        ],
        "inspect cache notes",
        SequenceKind::Read,
    )
    .expect("valid read commands");

    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].index, 0);
    assert_eq!(commands[0].command.tool, "read");
    assert_eq!(commands[1].index, 1);
    assert_eq!(commands[1].command.q.as_deref(), Some("cache"));
}

#[test]
fn preflight_collects_unknown_and_cross_tool_fields() {
    let error = prepare_sequence_commands(
        vec![
            json!({"tool": "read", "op": "spaces", "unexpected": true}),
            json!({
                "tool": "search",
                "op": "find",
                "target": "daily:/",
                "q": "cache",
                "content": "wrong branch"
            }),
        ],
        "validate all commands",
        SequenceKind::Read,
    )
    .expect_err("preflight must reject both commands before execution");
    let data = error.data.expect("structured preflight data");

    assert_eq!(data["code"], "sequence_preflight_failed");
    assert_eq!(data["executed"], false);
    assert_eq!(data["completed"], 0);
    assert_eq!(data["failed"], 0);
    assert_eq!(data["skipped"], 0);
    assert_eq!(data["results"], json!([]));
    assert_eq!(data["errors"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        data["errors"][0]["next_action"]["fields"][0],
        "commands[0].unexpected"
    );
    assert_eq!(
        data["errors"][1]["next_action"]["fields"][0],
        "commands[1].content"
    );
}

#[test]
fn sequence_commands_are_flat_and_inherit_purpose() {
    let error = prepare_sequence_commands(
        vec![json!({
            "tool": "read",
            "op": "read",
            "purpose": "nested purpose",
            "args": {"target": "daily:/note.md"}
        })],
        "top-level purpose",
        SequenceKind::Read,
    )
    .expect_err("nested purpose and args wrapper are rejected");
    let errors = error.data.expect("structured preflight data")["errors"]
        .as_array()
        .expect("preflight errors")
        .clone();

    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0]["code"], "sequence_command_purpose_not_allowed");
    assert_eq!(errors[1]["code"], "sequence_command_args_not_allowed");
    assert_eq!(
        errors[1]["next_action"]["value"],
        json!({"tool": "read", "op": "read", "target": "daily:/note.md"})
    );
}

#[test]
fn command_count_is_bounded_for_each_sequence_kind() {
    for kind in [SequenceKind::Read, SequenceKind::Write] {
        assert!(validate_sequence_command_count(1, kind).is_ok());
        assert!(validate_sequence_command_count(SEQUENCE_MAX_COMMANDS, kind).is_ok());

        for count in [0, SEQUENCE_MAX_COMMANDS + 1] {
            let error = validate_sequence_command_count(count, kind)
                .expect_err("out-of-range command count must fail");
            let data = error.data.expect("structured preflight data");
            assert_eq!(data["code"], "sequence_preflight_failed");
            assert_eq!(data["executed"], false);
        }
    }
}

#[test]
fn changes_cursor_uses_the_direct_read_contract() {
    let commands = prepare_sequence_commands(
        vec![json!({
            "tool": "read",
            "op": "changes",
            "target": "daily:/",
            "direction": "newer",
            "cursor": "opaque-change-cursor"
        })],
        "continue change feed",
        SequenceKind::Read,
    )
    .expect("valid changes command");

    assert_eq!(commands[0].command.direction.as_deref(), Some("newer"));
    assert_eq!(
        commands[0].command.cursor.as_deref(),
        Some("opaque-change-cursor")
    );
}

#[test]
fn write_preflight_uses_direct_static_content_validation() {
    let error = prepare_sequence_commands(
        vec![json!({
            "tool": "write",
            "op": "write",
            "target": "daily:/note.md"
        })],
        "replace note",
        SequenceKind::Write,
    )
    .expect_err("write without content must fail before execution");
    let data = error.data.expect("structured preflight data");

    assert_eq!(data["executed"], false);
    assert_eq!(data["errors"][0]["code"], "required_field_missing");
    assert_eq!(
        data["errors"][0]["next_action"]["fields"][0]["field"],
        "commands[0].content"
    );
}

#[test]
fn response_counts_all_settled_results_in_index_order() {
    let response = sequence_response(
        vec![
            SequenceOutcome {
                index: 0,
                tool: "read".to_owned(),
                op: "spaces".to_owned(),
                result: Ok(Json(json!({"spaces": []}))),
            },
            SequenceOutcome {
                index: 1,
                tool: "read".to_owned(),
                op: "stat".to_owned(),
                result: Err(ErrorData::invalid_params("missing", None)),
            },
            SequenceOutcome {
                index: 2,
                tool: "search".to_owned(),
                op: "find".to_owned(),
                result: Ok(Json(json!({"items": []}))),
            },
        ],
        0,
    );

    assert_eq!(response["ok"], false);
    assert_eq!(response["completed"], 2);
    assert_eq!(response["failed"], 1);
    assert_eq!(response["skipped"], 0);
    assert_eq!(response["results"][0]["index"], 0);
    assert_eq!(response["results"][1]["index"], 1);
    assert_eq!(response["results"][2]["index"], 2);
}

#[test]
fn response_reports_commands_skipped_after_a_write_failure() {
    let response = sequence_response(
        vec![
            SequenceOutcome {
                index: 0,
                tool: "manage".to_owned(),
                op: "mkdir".to_owned(),
                result: Ok(Json(json!({"node": {"id": "folder"}}))),
            },
            SequenceOutcome {
                index: 1,
                tool: "write".to_owned(),
                op: "write".to_owned(),
                result: Err(ErrorData::invalid_params("conflict", None)),
            },
        ],
        3,
    );

    assert_eq!(response["completed"], 1);
    assert_eq!(response["failed"], 1);
    assert_eq!(response["skipped"], 3);
    assert_eq!(response["results"].as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn read_executor_is_bounded_and_restores_input_order() {
    let commands = prepare_sequence_commands(
        (0..10)
            .map(|index| {
                json!({
                    "tool": "read",
                    "op": "stat",
                    "target": format!("daily:/{index}.md")
                })
            })
            .collect(),
        "inspect notes concurrently",
        SequenceKind::Read,
    )
    .expect("valid read commands");
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));

    let outcomes = read::collect_read_outcomes(commands, {
        let active = Arc::clone(&active);
        let maximum = Arc::clone(&maximum);
        move |command| {
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis((10 - command.index) as u64)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                SequenceOutcome {
                    index: command.index,
                    tool: command.command.tool,
                    op: command.command.op,
                    result: Ok(Json(json!({"index": command.index}))),
                }
            }
        }
    })
    .await;

    assert_eq!(
        maximum.load(Ordering::SeqCst),
        read::READ_SEQUENCE_CONCURRENCY
    );
    assert_eq!(
        outcomes
            .iter()
            .map(|outcome| outcome.index)
            .collect::<Vec<_>>(),
        (0..10).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn write_executor_is_serial_and_stops_after_failure() {
    let commands = prepare_sequence_commands(
        (0..3)
            .map(|index| {
                json!({
                    "tool": "manage",
                    "op": "mkdir",
                    "target": format!("daily:/{index}")
                })
            })
            .collect(),
        "create folders in order",
        SequenceKind::Write,
    )
    .expect("valid write commands");
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let executed = Arc::new(Mutex::new(Vec::new()));

    let (outcomes, skipped) = write::collect_write_outcomes(commands, 3, {
        let active = Arc::clone(&active);
        let maximum = Arc::clone(&maximum);
        let executed = Arc::clone(&executed);
        move |command| {
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            let executed = Arc::clone(&executed);
            async move {
                let index = command.index;
                executed.lock().expect("execution log").push(index);
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(1)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                SequenceOutcome {
                    index,
                    tool: command.command.tool,
                    op: command.command.op,
                    result: if index == 1 {
                        Err(ErrorData::invalid_params("stop", None))
                    } else {
                        Ok(Json(json!({"index": index})))
                    },
                }
            }
        }
    })
    .await;

    assert_eq!(maximum.load(Ordering::SeqCst), 1);
    assert_eq!(*executed.lock().expect("execution log"), vec![0, 1]);
    assert_eq!(outcomes.len(), 2);
    assert_eq!(skipped, 1);
}
