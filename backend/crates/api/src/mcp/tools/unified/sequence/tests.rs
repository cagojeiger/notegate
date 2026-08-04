#![allow(clippy::expect_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;

use super::*;
use serde_json::json;

#[test]
fn operation_specific_required_fields_use_the_common_recovery_action() {
    let error = required::<String>(None, "target", "read").expect_err("target is required");
    let data = error.data.expect("missing field carries recovery data");

    assert_eq!(data["code"], "required_field_missing");
    assert_eq!(data["next_action"]["kind"], "add_fields");
    assert_eq!(data["next_action"]["fields"][0]["field"], "target");
}

#[test]
fn purpose_is_required_for_direct_and_sequence_calls() {
    let direct = serde_json::from_value::<SearchInput>(json!({
        "op": "find",
        "target": "daily:/",
        "q": "cache"
    }));
    assert!(direct.is_err());

    let sequence = serde_json::from_value::<RunSequenceInput>(json!({
        "commands": [{"tool": "read", "op": "spaces"}]
    }));
    assert!(sequence.is_err());
}

#[test]
fn sequence_command_rejects_unknown_fields() {
    let input = serde_json::from_value::<RunSequenceInput>(json!({
        "purpose": "test unknown sequence fields",
        "commands": [{
            "tool": "read",
            "op": "spaces",
            "unexpected": true
        }]
    }))
    .expect("raw sequence commands parse before preflight");
    let error = prepare_sequence_commands(input.commands, &input.purpose)
        .expect_err("unknown command field should be rejected by preflight");
    let data = error.data.expect("structured preflight data");

    assert_eq!(data["code"], "sequence_preflight_failed");
    assert_eq!(data["ok"], false);
    assert_eq!(data["phase"], "preflight");
    assert_eq!(data["executed"], false);
    assert_eq!(data["completed"], 0);
    assert!(data["failed_index"].is_null());
    assert_eq!(data["results"], json!([]));
    assert_eq!(data["next_action"]["kind"], "apply_error_actions");
    assert_eq!(data["next_action"]["errors_field"], "errors");
    assert_eq!(data["errors"][0]["code"], "sequence_command_unknown_fields");
    assert_eq!(
        data["errors"][0]["next_action"]["fields"][0],
        "commands[0].unexpected"
    );
}

#[test]
fn sequence_command_rejects_fields_from_other_tools() {
    let input = serde_json::from_value::<RunSequenceInput>(json!({
            "purpose": "reject fields that belong to another tool branch",
            "commands": [
                {"tool": "read", "op": "spaces", "q": "cache"},
                {"tool": "search", "op": "find", "target": "daily:/", "q": "cache", "content": "ignored"},
                {"tool": "write", "op": "write", "target": "daily:/note.md", "content": "body", "source": "daily:/old.md"},
                {"tool": "manage", "op": "mkdir", "target": "daily:/folder", "cursor": "ignored"}
            ]
        }))
        .expect("raw sequence commands parse before preflight");
    let error = prepare_sequence_commands(input.commands, &input.purpose)
        .expect_err("tool-specific command fields should be rejected by preflight");
    let data = error.data.expect("structured preflight data");

    assert_eq!(data["code"], "sequence_preflight_failed");
    assert_eq!(data["executed"], false);
    assert_eq!(data["errors"].as_array().map(Vec::len), Some(4));
    let disallowed_fields = data["errors"]
        .as_array()
        .expect("preflight errors")
        .iter()
        .map(|error| {
            assert_eq!(
                error["code"],
                "sequence_command_fields_not_allowed_for_tool"
            );
            error["next_action"]["fields"][0]
                .as_str()
                .expect("disallowed field")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        disallowed_fields,
        vec![
            "commands[0].q",
            "commands[1].content",
            "commands[2].source",
            "commands[3].cursor"
        ]
    );
}

#[test]
fn sequence_command_count_errors_use_common_preflight_status_fields() {
    let cases = [
        (0, "sequence_commands_required", "add_fields", "commands[0]"),
        (
            RUN_SEQUENCE_MAX_COMMANDS + 1,
            "sequence_commands_too_many",
            "choose_value",
            "commands.length",
        ),
    ];

    for (count, expected_code, expected_action, expected_field) in cases {
        let error = validate_sequence_command_count(count)
            .expect_err("invalid command count should fail preflight");
        let data = error.data.expect("structured preflight data");

        assert_eq!(data["code"], "sequence_preflight_failed");
        assert_eq!(data["ok"], false);
        assert_eq!(data["phase"], "preflight");
        assert_eq!(data["executed"], false);
        assert_eq!(data["completed"], 0);
        assert!(data["failed_index"].is_null());
        assert_eq!(data["results"], json!([]));
        assert_eq!(data["next_action"]["kind"], "apply_error_actions");
        assert_eq!(data["next_action"]["errors_field"], "errors");
        assert_eq!(data["errors"].as_array().map(Vec::len), Some(1));
        assert_eq!(data["errors"][0]["path"], "commands");
        assert_eq!(data["errors"][0]["code"], expected_code);
        assert_eq!(data["errors"][0]["next_action"]["kind"], expected_action);
        let action_field = if expected_action == "add_fields" {
            &data["errors"][0]["next_action"]["fields"][0]["field"]
        } else {
            &data["errors"][0]["next_action"]["field"]
        };
        assert_eq!(action_field, expected_field);
    }

    assert!(validate_sequence_command_count(1).is_ok());
    assert!(validate_sequence_command_count(RUN_SEQUENCE_MAX_COMMANDS).is_ok());
}

#[test]
fn sequence_runtime_failure_uses_common_status_fields_and_child_action() {
    let mut results = vec![json!({
        "index": 0,
        "tool": "read",
        "op": "spaces",
        "ok": true,
        "result": {"spaces": []}
    })];
    let outcome = SequenceOutcome {
        index: 1,
        tool: "read".to_owned(),
        op: "read".to_owned(),
        result: Err(actionable_input_error(
            "required_field_missing",
            "target is required",
            "Add target and retry.",
            McpAction::AddFields {
                fields: vec![crate::mcp::contract::RequiredField {
                    field: "target".to_owned(),
                    description: None,
                }],
            },
        )),
    };

    let response = append_sequence_outcomes(&mut results, vec![outcome])
        .expect("runtime failure returns a structured sequence result");

    assert_eq!(response["ok"], false);
    assert_eq!(response["phase"], "runtime");
    assert_eq!(response["executed"], true);
    assert_eq!(response["completed"], 1);
    assert_eq!(response["failed_index"], 1);
    assert_eq!(response["results"].as_array().map(Vec::len), Some(1));
    assert_eq!(response["error"]["data"]["code"], "required_field_missing");
    assert_eq!(
        response["error"]["data"]["next_action"]["fields"][0]["field"],
        "commands[1].target"
    );
    assert_eq!(response["next_action"]["kind"], "add_fields");
    assert_eq!(
        response["next_action"]["fields"][0]["field"],
        "commands[1].target"
    );
}

#[test]
fn sequence_preflight_allowlist_matches_the_public_command_schema() {
    let schema = json!(schemars::schema_for!(SequenceCommandSchema));
    let variants = schema["oneOf"]
        .as_array()
        .expect("sequence schema variants");

    for tool in ["read", "search", "write", "manage"] {
        let variant = variants
            .iter()
            .find(|variant| variant["properties"]["tool"]["const"] == tool)
            .expect("tool schema variant");
        let schema_fields = variant["properties"]
            .as_object()
            .expect("sequence variant properties")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let preflight_fields = sequence_tool_fields(tool)
            .expect("runtime tool field allowlist")
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();

        assert_eq!(preflight_fields, schema_fields, "field drift for {tool}");
    }
}

#[test]
fn sequence_preflight_explains_top_level_purpose_and_flat_commands() {
    let input = serde_json::from_value::<RunSequenceInput>(json!({
        "purpose": "read two notes",
        "commands": [
            {
                "tool": "read",
                "op": "read",
                "target": "daily:/one.md",
                "purpose": "incorrect nested purpose"
            },
            {
                "tool": "read",
                "args": {
                    "purpose": "incorrect direct-tool purpose",
                    "op": "read",
                    "target": "daily:/two.md"
                }
            }
        ]
    }))
    .expect("raw commands parse before preflight");
    let error = prepare_sequence_commands(input.commands, &input.purpose)
        .expect_err("both command shapes should fail preflight");
    let data = error.data.expect("structured preflight data");

    assert_eq!(data["executed"], false);
    assert_eq!(data["errors"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        data["errors"][0]["code"],
        "sequence_command_purpose_not_allowed"
    );
    assert_eq!(
        data["errors"][0]["next_action"]["fields"][0],
        "commands[0].purpose"
    );
    assert_eq!(
        data["errors"][1]["code"],
        "sequence_command_args_not_allowed"
    );
    assert_eq!(data["errors"][1]["next_action"]["kind"], "replace_field");
    assert_eq!(
        data["errors"][1]["next_action"]["value"],
        json!({"tool": "read", "op": "read", "target": "daily:/two.md"})
    );
}

#[test]
fn sequence_preflight_collects_later_static_errors_before_execution() {
    let input = serde_json::from_value::<RunSequenceInput>(json!({
        "purpose": "validate the entire sequence",
        "commands": [
            {
                "tool": "write",
                "op": "write",
                "target": "daily:/created.md",
                "content": "created"
            },
            {"tool": "search", "op": "find", "target": "daily:/"},
            {"tool": "manage", "op": "mv", "source": "daily:/from.md"}
        ]
    }))
    .expect("raw commands parse before preflight");
    let error = prepare_sequence_commands(input.commands, &input.purpose)
        .expect_err("missing q and destination should fail before execution");
    let data = error.data.expect("structured preflight data");

    assert_eq!(data["executed"], false);
    assert_eq!(data["errors"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        data["errors"][0]["next_action"]["fields"][0]["field"],
        "commands[1].q"
    );
    assert_eq!(
        data["errors"][1]["next_action"]["fields"][0]["field"],
        "commands[2].destination"
    );
}

#[test]
fn sequence_preflight_rejects_invalid_changes_before_a_prior_write_executes() {
    let input = serde_json::from_value::<RunSequenceInput>(json!({
        "purpose": "validate changes before updating a note",
        "commands": [
            {
                "tool": "write",
                "op": "write",
                "target": "daily:/created.md",
                "content": "created"
            },
            {
                "tool": "read",
                "op": "changes",
                "target": "daily:/",
                "direction": "latest"
            },
            {
                "tool": "read",
                "op": "changes",
                "target": "daily:/",
                "direction": "newer"
            },
            {
                "tool": "read",
                "op": "changes",
                "target": "daily:/folder"
            }
        ]
    }))
    .expect("raw commands parse before preflight");
    let error = prepare_sequence_commands(input.commands, &input.purpose)
        .expect_err("all invalid changes commands must fail before execution");
    let data = error.data.expect("structured preflight data");

    assert_eq!(data["executed"], false);
    assert_eq!(data["errors"].as_array().map(Vec::len), Some(3));
    assert_eq!(data["errors"][0]["index"], 1);
    assert_eq!(data["errors"][0]["code"], "changes_direction_invalid");
    assert_eq!(
        data["errors"][0]["next_action"]["field"],
        "commands[1].direction"
    );
    assert_eq!(data["errors"][1]["index"], 2);
    assert_eq!(data["errors"][1]["code"], "changes_cursor_required");
    assert_eq!(data["errors"][2]["index"], 3);
    assert_eq!(data["errors"][2]["code"], "changes_scope_invalid");
    assert_eq!(
        data["errors"][2]["next_action"]["field"],
        "commands[3].target"
    );
}

#[test]
fn sequence_preflight_rejects_input_only_errors_before_a_prior_write_executes() {
    let oversized_append = "x".repeat(notegate_core::limits::TEXT_MAX_BYTES + 1);
    let cases = vec![
        (
            json!({"tool": "search", "op": "find", "target": "daily:/", "q": ""}),
            "search query cannot be empty",
        ),
        (
            json!({"tool": "search", "op": "grep", "target": "daily:/", "q": "(", "match": "regex"}),
            "invalid regex pattern",
        ),
        (
            json!({"tool": "write", "op": "patch", "target": "daily:/note.md", "edits": [{"old_text": "before", "new_text": "after", "mode": "latest"}]}),
            "mode must be 'unique', 'first', or 'all'",
        ),
        (
            json!({"tool": "write", "op": "patch", "target": "daily:/note.md", "edits": []}),
            "edits must not be empty",
        ),
        (
            json!({"tool": "write", "op": "edit", "target": "daily:/note.md", "edits": [{"op": "delete_lines"}]}),
            "start_line is required",
        ),
        (
            json!({"tool": "write", "op": "edit", "target": "daily:/note.md", "edits": [{"op": "delete_lines", "start_line": 3, "end_line": 2}]}),
            "start_line must be less than or equal to end_line",
        ),
        (
            json!({"tool": "write", "op": "write", "target": "daily:/config.json", "content": "{\"ok\":}"}),
            "invalid json syntax in config.json",
        ),
        (
            json!({"tool": "write", "op": "append", "target": "daily:/note.md", "content": oversized_append}),
            "text exceeds the maximum",
        ),
        (
            json!({"tool": "manage", "op": "mv", "source": "daily:/from.md", "destination": "other:/to.md"}),
            "source and destination must be in the same space",
        ),
        (
            json!({"tool": "manage", "op": "cp", "source": "daily:/from.md", "destination": "other:/to.md"}),
            "source and destination must be in the same space",
        ),
        (
            json!({"tool": "manage", "op": "mkdir", "target": "daily:/"}),
            "path must name a node, not the space root",
        ),
        (
            json!({"tool": "manage", "op": "rm", "target": "daily:/", "recursive": true}),
            "path must name a node, not the space root",
        ),
        (
            json!({"tool": "manage", "op": "mv", "source": "daily:/", "destination": "daily:/moved"}),
            "path must name a node, not the space root",
        ),
        (
            json!({"tool": "manage", "op": "mv", "source": "daily:/source", "destination": "daily:/"}),
            "path must name a node, not the space root",
        ),
        (
            json!({"tool": "manage", "op": "cp", "source": "daily:/", "destination": "daily:/copied", "recursive": true}),
            "path must name a node, not the space root",
        ),
        (
            json!({"tool": "manage", "op": "cp", "source": "daily:/source", "destination": "daily:/", "recursive": true}),
            "path must name a node, not the space root",
        ),
        (
            json!({"tool": "read", "op": "tree", "target": "daily:/", "depth": 0}),
            "depth must be at least 1",
        ),
        (
            json!({"tool": "read", "op": "read", "target": "daily:/note.md", "max_bytes": 0}),
            "max_bytes must be at least 1",
        ),
    ];

    for (invalid_command, expected_message) in cases {
        let error = prepare_sequence_commands(
            vec![
                json!({
                    "tool": "write",
                    "op": "write",
                    "target": "daily:/created.md",
                    "content": "created",
                    "create": true
                }),
                invalid_command,
            ],
            "reject request-local errors before writing",
        )
        .expect_err("request-local errors must fail sequence preflight");
        let data = error.data.expect("structured preflight data");

        assert_eq!(data["executed"], false);
        assert_eq!(data["errors"].as_array().map(Vec::len), Some(1));
        assert_eq!(data["errors"][0]["index"], 1);
        assert!(
            data["errors"][0]["message"]
                .as_str()
                .is_some_and(|message| message.contains(expected_message)),
            "expected error message containing {expected_message:?}, got {}",
            data["errors"][0]["message"]
        );
    }
}

#[test]
fn recursive_mkdir_keeps_the_space_root_as_an_idempotent_target() {
    let commands = prepare_sequence_commands(
        vec![json!({
            "tool": "manage",
            "op": "mkdir",
            "target": "daily:/",
            "parents": true
        })],
        "keep recursive root mkdir behavior",
    )
    .expect("mkdir parents=true may target the existing space root");

    assert_eq!(commands.len(), 1);
}

#[test]
fn sequence_preflight_collects_recoverable_shape_and_value_errors_in_one_command() {
    let error = prepare_sequence_commands(
        vec![json!({
            "tool": "search",
            "op": "grep",
            "target": "daily:/",
            "q": "cache",
            "match": "glob",
            "purpose": "incorrect nested purpose",
            "unexpected": true
        })],
        "validate every static input error",
    )
    .expect_err("all recoverable static errors should be reported together");
    let data = error.data.expect("structured preflight data");
    let codes = data["errors"]
        .as_array()
        .expect("errors array")
        .iter()
        .map(|error| error["code"].as_str().expect("error code"))
        .collect::<Vec<_>>();

    assert_eq!(
        codes,
        vec![
            "sequence_command_purpose_not_allowed",
            "sequence_command_unknown_fields",
            "invalid_field_value"
        ]
    );
    assert_eq!(data["executed"], false);
}

#[test]
fn sequence_preflight_names_valid_tool_and_operation_choices() {
    let error = prepare_sequence_commands(
            vec![
                json!({"tool": "download", "op": "read"}),
                json!({"tool": "read", "op": "download", "target": "daily:/note.md"}),
                json!({"tool": "search", "op": "grep", "target": "daily:/", "q": "cache", "match": "glob"}),
            ],
            "validate command choices",
        )
        .expect_err("invalid tool and op should fail preflight");
    let data = error.data.expect("structured preflight data");

    assert_eq!(
        data["errors"][0]["next_action"]["field"],
        "commands[0].tool"
    );
    assert_eq!(data["errors"][1]["next_action"]["field"], "commands[1].op");
    assert_eq!(
        data["errors"][1]["next_action"]["choices"],
        json!(["spaces", "ls", "tree", "stat", "read", "changes"])
    );
    assert_eq!(
        data["errors"][2]["next_action"]["field"],
        "commands[2].match"
    );
    assert_eq!(
        data["errors"][2]["next_action"]["choices"],
        json!(["literal", "regex"])
    );
}

#[test]
fn direct_and_sequence_commands_share_operation_validation() {
    let read = serde_json::from_value::<ReadInput>(json!({
        "purpose": "validate a direct read",
        "op": "read",
        "target": "daily:/note.md",
        "direction": "older"
    }))
    .expect("read input parses");
    let search = serde_json::from_value::<SearchInput>(json!({
        "purpose": "validate a direct search",
        "op": "grep",
        "target": "daily:/",
        "q": "cache",
        "match": "glob"
    }))
    .expect("search input parses");
    let write = serde_json::from_value::<WriteInput>(json!({
        "purpose": "validate a direct write",
        "op": "write",
        "target": "daily:/note.md"
    }))
    .expect("write input parses");
    let manage = serde_json::from_value::<ManageInput>(json!({
        "purpose": "validate a direct move",
        "op": "mv",
        "source": "daily:/from.md"
    }))
    .expect("manage input parses");

    let cases = vec![
        (
            validate_read_operation(&read).expect_err("direction is changes-only"),
            json!({"tool": "read", "op": "read", "target": "daily:/note.md", "direction": "older"}),
        ),
        (
            validate_search_operation(&search).expect_err("glob is invalid for grep"),
            json!({"tool": "search", "op": "grep", "target": "daily:/", "q": "cache", "match": "glob"}),
        ),
        (
            validate_write_operation(&write).expect_err("write content is required"),
            json!({"tool": "write", "op": "write", "target": "daily:/note.md"}),
        ),
        (
            validate_manage_operation(&manage).expect_err("move destination is required"),
            json!({"tool": "manage", "op": "mv", "source": "daily:/from.md"}),
        ),
    ];

    for (direct_error, command) in cases {
        let direct = error_json(direct_error);
        let sequence_error =
            prepare_sequence_commands(vec![command], "validate the same operation in a sequence")
                .expect_err("sequence command uses the same validation");
        let sequence_data = sequence_error.data.expect("sequence error data");
        let sequence = &sequence_data["errors"][0];

        assert_eq!(sequence["code"], direct["data"]["code"]);
        let mut expected_action = direct["data"]["next_action"].clone();
        prefix_sequence_action_fields(&mut expected_action, 0);
        assert_eq!(sequence["next_action"], expected_action);
    }
}

#[test]
fn edit_entries_keep_op_specific_runtime_parsing() {
    let patch = parse_edits::<files::PatchEdit>(
        Some(vec![json!({
            "old_text": "before",
            "new_text": "after",
            "mode": "unique",
            "expected_count": 1
        })]),
        "patch",
    )
    .expect("patch edit parses");
    assert_eq!(patch.len(), 1);
    assert_eq!(patch[0].old_text, "before");

    let line = parse_edits::<files::LineEditInput>(
        Some(vec![json!({
            "op": "replace_lines",
            "start_line": 2,
            "end_line": 3,
            "content": "replacement"
        })]),
        "edit",
    )
    .expect("line edit parses");
    assert_eq!(line.len(), 1);
    assert_eq!(line[0].op, "replace_lines");

    let error = parse_edits::<files::PatchEdit>(
        Some(vec![
            json!({"op": "delete_lines", "start_line": 2, "end_line": 3}),
        ]),
        "patch",
    )
    .expect_err("line edit must not parse as a patch edit");
    assert!(error.message.contains("invalid edit entry for op=patch"));
}

#[test]
fn sequence_command_uses_direct_command_shape() {
    let input = serde_json::from_value::<RunSequenceInput>(json!({
        "purpose": "test direct sequence command shape",
        "commands": [{
            "tool": "manage",
            "op": "mkdir",
            "target": "main:/daily",
            "parents": true
        }]
    }))
    .expect("valid command sequence parses");

    let commands = prepare_sequence_commands(input.commands, &input.purpose)
        .expect("valid command sequence passes preflight");
    assert_eq!(commands.len(), 1);
    let command = &commands.first().expect("one command").command;
    assert_eq!(command.tool, "manage");
    assert_eq!(command.op, "mkdir");
    assert!(command.parents);
}

#[test]
fn changes_uses_the_shared_opaque_cursor_with_a_direction() {
    let input = serde_json::from_value::<ReadInput>(json!({
        "purpose": "test changes pagination",
        "op": "changes",
        "target": "daily:/",
        "direction": "newer",
        "cursor": "opaque-change-cursor",
        "limit": 25
    }))
    .expect("valid changes input parses");

    assert_eq!(input.direction.as_deref(), Some("newer"));
    assert_eq!(input.cursor.as_deref(), Some("opaque-change-cursor"));
}

#[test]
fn changes_fields_on_other_read_ops_name_the_fields_to_remove() {
    let input = serde_json::from_value::<ReadInput>(json!({
        "purpose": "test changes-only field validation",
        "op": "read",
        "target": "daily:/note.md",
        "direction": "older"
    }))
    .expect("known fields parse before operation validation");
    let error = validate_read_change_fields(&input)
        .expect_err("changes fields are rejected outside changes");

    let data = error.data.expect("structured recovery data");
    assert_eq!(data["code"], "changes_fields_not_allowed");
    assert_eq!(data["next_action"]["kind"], "remove_fields");
    assert_eq!(data["next_action"]["fields"], json!(["direction"]));
}

#[test]
fn run_sequence_accepts_a_changes_cursor() {
    let input = serde_json::from_value::<RunSequenceInput>(json!({
        "purpose": "test changes in a sequence",
        "commands": [{
            "tool": "read",
            "op": "changes",
            "target": "daily:/",
            "direction": "newer",
            "cursor": "opaque-change-cursor"
        }]
    }))
    .expect("valid changes sequence parses");

    let commands = prepare_sequence_commands(input.commands, &input.purpose)
        .expect("valid changes sequence passes preflight");
    let command = &commands.first().expect("one command").command;
    assert_eq!(command.direction.as_deref(), Some("newer"));
    assert_eq!(command.cursor.as_deref(), Some("opaque-change-cursor"));
}

#[test]
fn sequence_access_conflicts_are_path_and_scope_aware() {
    let purpose = "plan safe sequence concurrency";
    let commands = prepare_sequence_commands(
        vec![
            json!({"tool": "write", "op": "write", "target": "daily:/a/note.md", "content": "x"}),
            json!({"tool": "read", "op": "read", "target": "daily:/b/note.md"}),
            json!({"tool": "search", "op": "grep", "target": "daily:/a", "q": "x"}),
            json!({"tool": "read", "op": "changes", "target": "daily:/"}),
            json!({"tool": "read", "op": "read", "target": "other:/a/note.md"}),
        ],
        purpose,
    )
    .expect("commands pass preflight");

    assert!(!sequence_commands_conflict(&commands[0], &commands[1]));
    assert!(sequence_commands_conflict(&commands[0], &commands[2]));
    assert!(sequence_commands_conflict(&commands[0], &commands[3]));
    assert!(!sequence_commands_conflict(&commands[0], &commands[4]));
}

#[test]
fn parent_stat_depends_on_child_creation() {
    for mutation in [
        json!({"tool": "manage", "op": "mkdir", "target": "daily:/folder/child"}),
        json!({"tool": "write", "op": "write", "target": "daily:/folder/child.md", "content": "x", "create": true}),
    ] {
        let commands = prepare_sequence_commands(
            vec![
                mutation,
                json!({"tool": "read", "op": "stat", "target": "daily:/folder"}),
                json!({"tool": "read", "op": "read", "target": "daily:/folder/sibling.md"}),
            ],
            "preserve parent metadata after child creation",
        )
        .expect("commands pass preflight");
        let graph = build_sequence_dependency_graph(&commands);

        assert!(graph.depends_on(1, 0));
        assert!(!graph.depends_on(2, 0));
    }
}

#[test]
fn sequence_commands_are_classified_before_graph_construction() {
    let commands = prepare_sequence_commands(
            vec![
                json!({"tool": "read", "op": "read", "target": "daily:/note.md"}),
                json!({"tool": "search", "op": "grep", "target": "daily:/", "q": "cache"}),
                json!({"tool": "read", "op": "changes", "target": "daily:/"}),
                json!({"tool": "write", "op": "write", "target": "daily:/note.md", "content": "x"}),
                json!({"tool": "write", "op": "write", "target": "daily:/new.md", "content": "x", "create": true}),
                json!({"tool": "manage", "op": "mkdir", "target": "daily:/folder"}),
                json!({"tool": "manage", "op": "mkdir", "target": "daily:/a/b", "parents": true}),
                json!({"tool": "manage", "op": "mv", "source": "daily:/a", "destination": "daily:/b"}),
                json!({"tool": "manage", "op": "cp", "source": "daily:/a", "destination": "daily:/b"}),
                json!({"tool": "manage", "op": "rm", "target": "daily:/a", "recursive": true})
            ],
            "classify sequence execution",
        )
        .expect("commands pass preflight");

    assert_eq!(
        commands[0].execution_class,
        SequenceExecutionClass::PureRead
    );
    assert_eq!(
        commands[1].execution_class,
        SequenceExecutionClass::WideRead
    );
    assert_eq!(
        commands[2].execution_class,
        SequenceExecutionClass::ConsistencyRead
    );
    assert_eq!(
        commands[3].execution_class,
        SequenceExecutionClass::PointMutation
    );
    assert_eq!(
        commands[4].execution_class,
        SequenceExecutionClass::NamespaceMutation
    );
    assert_eq!(
        commands[5].execution_class,
        SequenceExecutionClass::NamespaceMutation
    );
    for command in &commands[6..] {
        assert_eq!(
            command.execution_class,
            SequenceExecutionClass::StructuralBarrier
        );
    }
    assert_eq!(commands[7].accesses.len(), 2);
    assert!(
        commands[7]
            .accesses
            .iter()
            .all(|access| access.mode == SequenceAccessMode::Write)
    );
    assert_eq!(commands[8].accesses.len(), 2);
    assert_eq!(commands[8].accesses[0].mode, SequenceAccessMode::Read);
    assert_eq!(commands[8].accesses[1].mode, SequenceAccessMode::Write);
}

#[test]
fn dependency_graph_is_explicit_and_structural_mutations_are_barriers() {
    let commands = prepare_sequence_commands(
            vec![
                json!({"tool": "read", "op": "read", "target": "daily:/before.md"}),
                json!({"tool": "write", "op": "write", "target": "daily:/point.md", "content": "x"}),
                json!({"tool": "read", "op": "read", "target": "daily:/other.md"}),
                json!({"tool": "manage", "op": "cp", "source": "daily:/source", "destination": "daily:/destination", "recursive": true}),
                json!({"tool": "read", "op": "read", "target": "other:/after.md"}),
                json!({"tool": "write", "op": "write", "target": "daily:/last.md", "content": "x"})
            ],
            "build a deterministic dependency graph",
        )
        .expect("commands pass preflight");
    let graph = build_sequence_dependency_graph(&commands);

    assert_eq!(
        graph.dependencies,
        vec![
            vec![],
            vec![0],
            vec![],
            vec![0, 1, 2],
            vec![3],
            vec![0, 1, 2, 3, 4],
        ]
    );
    for (index, dependencies) in graph.dependencies.iter().enumerate() {
        assert!(dependencies.iter().all(|dependency| *dependency < index));
    }
}
