//! Fail-closed snapshots for MCP invocation history.
//!
//! These helpers never mutate the request or response sent to the MCP client.
//! They build a separate, bounded copy for persistence and omit every field
//! that has not been explicitly classified.

use rmcp::ErrorData;
use rmcp::model::{CallToolResponse, CallToolResult};
use serde_json::{Map, Value, json};

use super::tool_identity::KnownMcpTool;

const PURPOSE_MAX_CHARS: usize = 200;
const SAFE_STRING_MAX_CHARS: usize = 2_048;
const SAFE_INPUT_ARRAY_MAX_ITEMS: usize = 100;
const SAFE_OUTPUT_ARRAY_MAX_ITEMS: usize = 1_000;
const SNAPSHOT_MAX_BYTES: usize = 256 * 1_024;

type FieldSet = &'static str;
type SensitiveFields = &'static [(&'static str, &'static str)];

#[derive(Clone, Copy)]
enum InputSpecial {
    None,
    Commands,
    CompletedParts,
    LineEdits,
    PatchEdits,
    UnsupportedOperation,
}

const EMPTY: FieldSet = "";
const NO_SENSITIVE: SensitiveFields = &[];
const CURSOR: SensitiveFields = &[("cursor", "opaque_cursor")];
const SEARCH_SENSITIVE: SensitiveFields = &[("q", "search_query"), ("cursor", "opaque_cursor")];
const CONTENT: SensitiveFields = &[("content", "document_content")];
const UPLOAD_METADATA: SensitiveFields = &[
    ("encryption_metadata", "encryption_metadata"),
    ("original_filename", "original_filename"),
];

const SAFE_OUTPUT_KEYS: FieldSet = "\
account actor_account_id affected_parent_ids agent appended baseline_call byte_len \
can_create_space can_manage_agents capabilities code collect_response_header completed \
content_blocks_omitted content_length content_returned content_sha256 copied counts created_at data \
created_paths default_search_enabled default_text_encryption_enabled deleted depth description \
direction edited edits edits_applied effective_write_locked end_line error errors errors_field event_id events executed \
expected_count expires_in_seconds failed features field fields files has_children has_more \
id includes_descendants index input item_kind items kind limit line_count matches \
max_concurrency media_type method mode name next_action next_start_line node node_id nodes ok op \
operation order page parent_node_id_after parent_node_id_before parent_scope_known part_count \
part_number part_numbers part_size parts patched path path_changed permission previous_sha256 \
purge_after purpose recoverable repeat requires resource result results resync_required \
retry_after_ms retry_after_seconds retryable returned returned_lines scope search_enabled \
server_version skipped sort_order source_path space spaces start_line status subtree_changed suggestions \
target text_at_rest_encryption text_encryption text_storage_format texts then tool transfer \
transfer_field transfers_field truncated unchanged updated_at upload_id user when encryption_mode \
write_lock_changed write_lock_sources write_locked";

pub(super) fn redact_input(tool: &str, input: &Value) -> Value {
    bounded_snapshot(redact_tool_input(tool, input, true, false))
}

pub(super) fn redact_response(
    tool: &str,
    input: &Value,
    result: &Result<CallToolResponse, ErrorData>,
) -> Value {
    let snapshot = match result {
        Err(error) => json!({"kind": "error", "error": redact_error(error)}),
        Ok(CallToolResponse::Complete(result)) => complete_response(tool, input, result),
        Ok(CallToolResponse::InputRequired(_)) => unsupported_response("input_required"),
        Ok(CallToolResponse::Task(_)) => unsupported_response("task"),
        Ok(_) => unsupported_response("unknown"),
    };
    bounded_snapshot(snapshot)
}

fn complete_response(tool: &str, input: &Value, result: &CallToolResult) -> Value {
    let mut snapshot = Map::new();
    snapshot.insert("kind".to_owned(), Value::String("complete".to_owned()));
    snapshot.insert(
        "is_error".to_owned(),
        Value::Bool(result.is_error.unwrap_or(false)),
    );
    snapshot.insert(
        "content_blocks_omitted".to_owned(),
        json!(result.content.len()),
    );
    if let Some(structured) = result.structured_content.as_ref() {
        snapshot.insert(
            "result".to_owned(),
            redact_structured_response(tool, input, structured),
        );
    }
    Value::Object(snapshot)
}

fn unsupported_response(kind: &str) -> Value {
    json!({
        "kind": kind,
        "details": redaction_marker("unsupported_response_variant", &Value::Null),
    })
}

fn redact_error(error: &ErrorData) -> Value {
    let mut output = Map::new();
    output.insert("code".to_owned(), json!(error.code.0));
    output.insert(
        "message".to_owned(),
        redaction_marker(
            "untrusted_error_text",
            &Value::String(error.message.to_string()),
        ),
    );
    if let Some(data) = error.data.as_ref() {
        output.insert("data".to_owned(), redact_output_value(data));
    }
    Value::Object(output)
}

fn redact_tool_input(
    tool: &str,
    input: &Value,
    include_purpose: bool,
    include_tool: bool,
) -> Value {
    let Some(input) = input.as_object() else {
        return redaction_marker("unrecognized_input_shape", input);
    };

    let known_tool = KnownMcpTool::parse(tool);

    if known_tool == Some(KnownMcpTool::Me) {
        return select_input_fields(
            input,
            EMPTY,
            NO_SENSITIVE,
            false,
            include_tool,
            InputSpecial::None,
        );
    }

    if known_tool.is_some_and(KnownMcpTool::is_sequence) {
        return select_input_fields(
            input,
            EMPTY,
            NO_SENSITIVE,
            include_purpose,
            include_tool,
            InputSpecial::Commands,
        );
    }

    if known_tool == Some(KnownMcpTool::FileDownload) {
        return select_input_fields(
            input,
            "target",
            NO_SENSITIVE,
            include_purpose,
            include_tool,
            InputSpecial::None,
        );
    }

    let Some(op) = input.get("op").and_then(Value::as_str) else {
        return classify_unknown_input(tool, input, include_purpose, include_tool);
    };

    input_policy(tool, op).map_or_else(
        || classify_unknown_input(tool, input, include_purpose, include_tool),
        |(safe, sensitive, special)| {
            select_input_fields(
                input,
                safe,
                sensitive,
                include_purpose,
                include_tool,
                special,
            )
        },
    )
}

fn input_policy(tool: &str, op: &str) -> Option<(FieldSet, SensitiveFields, InputSpecial)> {
    let policy = match (tool, op) {
        ("read", "spaces") => ("op name limit", CURSOR, InputSpecial::None),
        ("read", "ls") => ("op target limit", CURSOR, InputSpecial::None),
        ("read", "tree") => ("op target depth limit", CURSOR, InputSpecial::None),
        ("read", "stat") => ("op target", NO_SENSITIVE, InputSpecial::None),
        ("read", "read") => (
            "op target start_line max_lines max_bytes if_none_match_sha256",
            NO_SENSITIVE,
            InputSpecial::None,
        ),
        ("read", "changes") => ("op target limit direction", CURSOR, InputSpecial::None),
        ("search", "find") => (
            "op target kind match include exclude limit",
            SEARCH_SENSITIVE,
            InputSpecial::None,
        ),
        ("search", "grep") => (
            "op target match lines include exclude limit",
            SEARCH_SENSITIVE,
            InputSpecial::None,
        ),
        ("write", "write") => (
            "op target create expected_sha256",
            CONTENT,
            InputSpecial::None,
        ),
        ("write", "append") => (
            "op target create ensure_newline expected_sha256",
            CONTENT,
            InputSpecial::None,
        ),
        ("write", "patch") => (
            "op target expected_sha256",
            NO_SENSITIVE,
            InputSpecial::PatchEdits,
        ),
        ("write", "edit") => (
            "op target expected_sha256",
            NO_SENSITIVE,
            InputSpecial::LineEdits,
        ),
        ("manage", "mkdir") => ("op target parents", NO_SENSITIVE, InputSpecial::None),
        ("manage", "mv") => ("op source destination", NO_SENSITIVE, InputSpecial::None),
        ("manage", "cp") => (
            "op source destination recursive",
            NO_SENSITIVE,
            InputSpecial::None,
        ),
        ("manage", "rm") => ("op target recursive", NO_SENSITIVE, InputSpecial::None),
        ("file_upload", "begin_upload") => (
            "op target byte_len media_type encryption_mode",
            UPLOAD_METADATA,
            InputSpecial::None,
        ),
        ("file_upload", "prepare_parts") => (
            "op upload_id part_numbers",
            NO_SENSITIVE,
            InputSpecial::None,
        ),
        ("file_upload", "complete_upload") => {
            ("op upload_id", NO_SENSITIVE, InputSpecial::CompletedParts)
        }
        ("file_upload", "abort_upload") => ("op upload_id", NO_SENSITIVE, InputSpecial::None),
        _ => return None,
    };
    Some(policy)
}

fn classify_unknown_input(
    tool: &str,
    input: &Map<String, Value>,
    include_purpose: bool,
    include_tool: bool,
) -> Value {
    if known_op_tool(tool) {
        unsupported_operation(input, include_purpose, include_tool)
    } else {
        unrecognized_tool_input(input, include_purpose)
    }
}

fn known_op_tool(tool: &str) -> bool {
    KnownMcpTool::parse(tool).is_some_and(KnownMcpTool::accepts_op)
}

fn field_set_contains(fields: FieldSet, key: &str) -> bool {
    fields.split_ascii_whitespace().any(|field| field == key)
}

fn select_input_fields(
    input: &Map<String, Value>,
    safe: FieldSet,
    sensitive: SensitiveFields,
    include_purpose: bool,
    include_tool: bool,
    special: InputSpecial,
) -> Value {
    let mut output = Map::new();
    let mut omitted = 0_usize;

    for (key, value) in input {
        if key == "purpose" && include_purpose {
            output.insert(key.clone(), redact_purpose(value));
        } else if key == "tool" && include_tool {
            output.insert(key.clone(), redact_known_tool(value));
        } else if field_set_contains(safe, key) {
            output.insert(key.clone(), safe_input_value(value));
        } else if let Some((_, category)) = sensitive.iter().find(|(field, _)| field == key) {
            output.insert(key.clone(), redaction_marker(category, value));
        } else if let Some(redacted) = redact_special_input(special, key, value) {
            output.insert(key.clone(), redacted);
        } else {
            omitted = omitted.saturating_add(1);
        }
    }

    if omitted > 0 {
        output.insert("_omitted_field_count".to_owned(), json!(omitted));
    }
    Value::Object(output)
}

fn redact_special_input(special: InputSpecial, key: &str, value: &Value) -> Option<Value> {
    match (special, key) {
        (InputSpecial::Commands, "commands") => Some(redact_commands(value)),
        (InputSpecial::CompletedParts, "completed_parts") => Some(redact_completed_parts(value)),
        (InputSpecial::LineEdits, "edits") => Some(redact_line_edits(value)),
        (InputSpecial::PatchEdits, "edits") => Some(redact_patch_edits(value)),
        (InputSpecial::UnsupportedOperation, "op") => {
            Some(redaction_marker("unsupported_operation", value))
        }
        _ => None,
    }
}

fn unsupported_operation(
    input: &Map<String, Value>,
    include_purpose: bool,
    include_tool: bool,
) -> Value {
    select_input_fields(
        input,
        EMPTY,
        NO_SENSITIVE,
        include_purpose,
        include_tool,
        InputSpecial::UnsupportedOperation,
    )
}

fn unrecognized_tool_input(input: &Map<String, Value>, include_purpose: bool) -> Value {
    let mut output = Map::new();
    if include_purpose && let Some(purpose) = input.get("purpose") {
        output.insert("purpose".to_owned(), redact_purpose(purpose));
    }
    output.insert(
        "_redacted_payload".to_owned(),
        redaction_marker("unrecognized_tool_input", &Value::Object(input.clone())),
    );
    Value::Object(output)
}

fn redact_commands(value: &Value) -> Value {
    let Some(commands) = value.as_array() else {
        return redaction_marker("unrecognized_commands_shape", value);
    };
    if commands.len() > 20 {
        return redaction_marker("commands_too_large", value);
    }
    Value::Array(
        commands
            .iter()
            .map(|command| {
                let Some(command) = command.as_object() else {
                    return redaction_marker("unrecognized_command_shape", command);
                };
                let Some(tool) = command.get("tool").and_then(Value::as_str) else {
                    return redaction_marker(
                        "unrecognized_command_tool",
                        &Value::Object(command.clone()),
                    );
                };
                if !matches!(tool, "read" | "search" | "write" | "manage") {
                    return redaction_marker(
                        "unrecognized_command_tool",
                        &Value::Object(command.clone()),
                    );
                }
                redact_tool_input(tool, &Value::Object(command.clone()), false, true)
            })
            .collect(),
    )
}

fn redact_patch_edits(value: &Value) -> Value {
    redact_edits(value, |edit| {
        select_input_fields(
            edit,
            "mode expected_count",
            &[
                ("old_text", "document_content"),
                ("new_text", "document_content"),
            ],
            false,
            false,
            InputSpecial::None,
        )
    })
}

fn redact_line_edits(value: &Value) -> Value {
    redact_edits(value, |edit| {
        let known_op = edit.get("op").and_then(Value::as_str).is_some_and(|op| {
            matches!(
                op,
                "insert_before_line" | "insert_after_line" | "replace_lines" | "delete_lines"
            )
        });
        if known_op {
            select_input_fields(
                edit,
                "op line start_line end_line",
                &[("content", "document_content")],
                false,
                false,
                InputSpecial::None,
            )
        } else {
            redaction_marker("unrecognized_line_edit", &Value::Object(edit.clone()))
        }
    })
}

fn redact_edits(value: &Value, redact: impl Fn(&Map<String, Value>) -> Value) -> Value {
    redact_object_array(
        value,
        SAFE_INPUT_ARRAY_MAX_ITEMS,
        "unrecognized_edits_shape",
        "edits_too_large",
        "unrecognized_edit_shape",
        redact,
    )
}

fn redact_completed_parts(value: &Value) -> Value {
    redact_object_array(
        value,
        SAFE_INPUT_ARRAY_MAX_ITEMS,
        "unrecognized_completed_parts_shape",
        "completed_parts_too_large",
        "unrecognized_completed_part",
        |part| {
            select_input_fields(
                part,
                "part_number",
                &[("etag", "object_etag")],
                false,
                false,
                InputSpecial::None,
            )
        },
    )
}

fn redact_object_array(
    value: &Value,
    max_items: usize,
    shape_category: &str,
    size_category: &str,
    item_category: &str,
    redact: impl Fn(&Map<String, Value>) -> Value,
) -> Value {
    let Some(items) = value.as_array() else {
        return redaction_marker(shape_category, value);
    };
    if items.len() > max_items {
        return redaction_marker(size_category, value);
    }
    Value::Array(
        items
            .iter()
            .map(|item| {
                item.as_object()
                    .map_or_else(|| redaction_marker(item_category, item), &redact)
            })
            .collect(),
    )
}

fn redact_structured_response(tool: &str, input: &Value, value: &Value) -> Value {
    let op = input.get("op").and_then(Value::as_str);
    response_fields(tool, op).map_or_else(
        || redaction_marker("unrecognized_response_contract", value),
        |allowed| redact_output_object(value, allowed),
    )
}

fn response_fields(tool: &str, op: Option<&str>) -> Option<FieldSet> {
    match (tool, op) {
        ("me", _) => Some("account user agent capabilities server_version"),
        ("read", Some("spaces")) => Some("spaces page"),
        ("read", Some("ls" | "tree")) => Some("space path depth items page"),
        ("read", Some("stat")) => Some("space node"),
        ("read", Some("read")) => Some(
            "space path unchanged content_returned content content_sha256 byte_len line_count \
             start_line end_line returned_lines truncated next_start_line",
        ),
        ("read", Some("changes")) => Some(
            "space path scope direction order events page checkpoint_cursor resync_required \
             next_action",
        ),
        ("search", Some("find" | "grep")) => Some("space items page"),
        ("write", Some("write")) => Some("space node content_sha256 byte_len line_count"),
        ("write", Some("append")) => Some("space node appended content_sha256 byte_len line_count"),
        ("write", Some("patch")) => Some(
            "space path node patched edits_applied content_sha256 previous_sha256 byte_len \
             line_count diff",
        ),
        ("write", Some("edit")) => Some(
            "space path node edited edits_applied content_sha256 previous_sha256 byte_len \
             line_count diff",
        ),
        ("manage", Some("mkdir")) => Some("space node created_paths"),
        ("manage", Some("mv")) => Some("space node"),
        ("manage", Some("cp")) => Some("space source_path node copied counts"),
        ("manage", Some("rm")) => Some("space path deleted purge_after"),
        ("file_download", None) => Some("target transfer node next_action"),
        ("file_upload", Some("begin_upload")) => Some("upload_id target transfer next_action"),
        ("file_upload", Some("prepare_parts")) => Some("upload_id parts next_action"),
        ("file_upload", Some("complete_upload")) => Some("upload_id node next_action"),
        ("file_upload", Some("abort_upload")) => Some("upload_id status next_action"),
        ("run_read_sequence" | "run_write_sequence", _) => {
            Some("ok completed failed skipped results")
        }
        _ => None,
    }
}

fn redact_output_object(value: &Value, allowed: FieldSet) -> Value {
    let Some(object) = value.as_object() else {
        return redaction_marker("unrecognized_output_shape", value);
    };
    let mut output = Map::new();
    let mut omitted = 0_usize;
    for (key, value) in object {
        if field_set_contains(allowed, key) {
            output.insert(key.clone(), redact_output_field(key, value));
        } else {
            omitted = omitted.saturating_add(1);
        }
    }
    if omitted > 0 {
        output.insert("_omitted_field_count".to_owned(), json!(omitted));
    }
    Value::Object(output)
}

fn redact_output_value(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(value) => bounded_string(value),
        Value::Array(values) if values.len() <= SAFE_OUTPUT_ARRAY_MAX_ITEMS => {
            Value::Array(values.iter().map(redact_output_value).collect())
        }
        Value::Array(_) => redaction_marker("output_array_too_large", value),
        Value::Object(object) => redact_known_output_fields(object),
    }
}

fn redact_known_output_fields(object: &Map<String, Value>) -> Value {
    let mut output = Map::new();
    let mut omitted = 0_usize;
    for (key, value) in object {
        if sensitive_output_category(key).is_some() || field_set_contains(SAFE_OUTPUT_KEYS, key) {
            output.insert(key.clone(), redact_output_field(key, value));
        } else {
            omitted = omitted.saturating_add(1);
        }
    }
    if omitted > 0 {
        output.insert("_omitted_field_count".to_owned(), json!(omitted));
    }
    Value::Object(output)
}

fn redact_output_field(key: &str, value: &Value) -> Value {
    if key == "data" {
        return value.as_object().map_or_else(
            || redaction_marker("unrecognized_error_data", value),
            redact_known_output_fields,
        );
    }
    sensitive_output_category(key).map_or_else(
        || redact_output_value(value),
        |category| redaction_marker(category, value),
    )
}

fn sensitive_output_category(key: &str) -> Option<&'static str> {
    match key {
        "content" | "old_text" | "new_text" | "diff" | "match_lines" => Some("document_content"),
        "q" => Some("search_query"),
        "cursor" | "next_cursor" | "checkpoint_cursor" => Some("opaque_cursor"),
        "url" => Some("presigned_url"),
        "headers" => Some("transfer_headers"),
        "etag" => Some("object_etag"),
        "email" | "display_name" => Some("pii"),
        "encryption_metadata" | "metadata" => Some("opaque_metadata"),
        "original_filename" => Some("original_filename"),
        "message" | "hint" | "reason" | "instruction" => Some("untrusted_text"),
        "value" | "choices" => Some("untrusted_action_value"),
        "_meta" | "details" => Some("opaque_protocol_metadata"),
        _ => None,
    }
}

fn redact_purpose(value: &Value) -> Value {
    let Some(purpose) = value.as_str() else {
        return redaction_marker("invalid_purpose", value);
    };
    let chars = purpose.chars().count();
    if chars == 0 || chars > PURPOSE_MAX_CHARS || purpose.trim() != purpose {
        redaction_marker("invalid_purpose", value)
    } else {
        Value::String(purpose.to_owned())
    }
}

fn redact_known_tool(value: &Value) -> Value {
    value
        .as_str()
        .and_then(KnownMcpTool::parse)
        .filter(|tool| tool.is_sequence_command())
        .map_or_else(
            || redaction_marker("unrecognized_command_tool", value),
            |tool| Value::String(tool.as_str().to_owned()),
        )
}

fn safe_input_value(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(value) => bounded_string(value),
        Value::Array(values) if values.len() <= SAFE_INPUT_ARRAY_MAX_ITEMS => {
            Value::Array(values.iter().map(safe_input_value).collect())
        }
        Value::Array(_) => redaction_marker("input_array_too_large", value),
        Value::Object(_) => redaction_marker("unrecognized_input_value", value),
    }
}

fn bounded_string(value: &str) -> Value {
    if value.chars().count() <= SAFE_STRING_MAX_CHARS {
        Value::String(value.to_owned())
    } else {
        redaction_marker("string_too_large", &Value::String(value.to_owned()))
    }
}

fn bounded_snapshot(value: Value) -> Value {
    let byte_len = serialized_len(&value);
    if byte_len <= SNAPSHOT_MAX_BYTES {
        value
    } else {
        json!({
            "_truncated": true,
            "category": "snapshot_size_limit",
            "byte_len": byte_len,
            "limit_bytes": SNAPSHOT_MAX_BYTES,
        })
    }
}

fn redaction_marker(category: &str, value: &Value) -> Value {
    let mut marker = Map::new();
    marker.insert("_redacted".to_owned(), Value::Bool(true));
    marker.insert("category".to_owned(), Value::String(category.to_owned()));
    marker.insert(
        "value_type".to_owned(),
        Value::String(value_type(value).to_owned()),
    );
    match value {
        Value::String(value) => {
            marker.insert("byte_len".to_owned(), json!(value.len()));
            marker.insert("char_len".to_owned(), json!(value.chars().count()));
        }
        Value::Array(values) => {
            marker.insert("item_count".to_owned(), json!(values.len()));
            marker.insert("byte_len".to_owned(), json!(serialized_len(value)));
        }
        Value::Object(values) => {
            marker.insert("field_count".to_owned(), json!(values.len()));
            marker.insert("byte_len".to_owned(), json!(serialized_len(value)));
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Value::Object(marker)
}

fn serialized_len(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use rmcp::model::CallToolResult;

    use super::*;

    fn serialized(value: &Value) -> String {
        serde_json::to_string(value).expect("redacted value serializes")
    }

    #[test]
    fn write_inputs_keep_structure_without_document_content_or_unknown_values() {
        let input = json!({
            "purpose": "update the selected note",
            "op": "patch",
            "target": "daily:/note.md",
            "expected_sha256": "hash",
            "edits": [{
                "old_text": "SECRET_OLD",
                "new_text": "SECRET_NEW",
                "mode": "unique",
                "expected_count": 1,
                "payload": "SECRET_UNKNOWN"
            }],
            "body": "SECRET_TOP_LEVEL"
        });

        let redacted = redact_input("write", &input);
        let text = serialized(&redacted);

        assert_eq!(redacted["target"], "daily:/note.md");
        assert_eq!(redacted["edits"][0]["mode"], "unique");
        assert_eq!(redacted["edits"][0]["_omitted_field_count"], 1);
        assert_eq!(redacted["_omitted_field_count"], 1);
        assert!(!text.contains("SECRET_OLD"));
        assert!(!text.contains("SECRET_NEW"));
        assert!(!text.contains("SECRET_UNKNOWN"));
        assert!(!text.contains("SECRET_TOP_LEVEL"));
    }

    #[test]
    fn sequence_inputs_delegate_to_child_tool_policies() {
        let read_input = json!({
            "purpose": "find note updates",
            "commands": [
                {"tool": "search", "op": "grep", "target": "daily:/", "q": "SECRET_QUERY"}
            ]
        });
        let write_input = json!({
            "purpose": "apply note updates",
            "commands": [
                {"tool": "write", "op": "write", "target": "daily:/note.md", "content": "SECRET_BODY"}
            ]
        });

        let read = redact_input("run_read_sequence", &read_input);
        let write = redact_input("run_write_sequence", &write_input);
        let text = format!("{}{}", serialized(&read), serialized(&write));

        assert_eq!(read["commands"][0]["tool"], "search");
        assert_eq!(write["commands"][0]["target"], "daily:/note.md");
        assert!(!text.contains("SECRET_QUERY"));
        assert!(!text.contains("SECRET_BODY"));
    }

    #[test]
    fn responses_drop_wire_text_and_redact_nested_sensitive_values() {
        let input = json!({"purpose": "read a note", "op": "read", "target": "daily:/note.md"});
        let result = Ok(CallToolResult::structured(json!({
            "space": "daily",
            "path": "/note.md",
            "content": "SECRET_BODY",
            "content_sha256": "hash",
            "byte_len": 11,
            "line_count": 1,
            "start_line": 1,
            "end_line": 1,
            "returned_lines": 1,
            "truncated": false,
            "next_start_line": null,
            "future_payload": "SECRET_FUTURE"
        }))
        .into());

        let redacted = redact_response("read", &input, &result);
        let text = serialized(&redacted);

        assert_eq!(redacted["result"]["path"], "/note.md");
        assert_eq!(redacted["result"]["content"]["_redacted"], true);
        assert_eq!(redacted["result"]["_omitted_field_count"], 1);
        assert!(!text.contains("SECRET_BODY"));
        assert!(!text.contains("SECRET_FUTURE"));
        assert!(!text.contains("content\\\":\\\"SECRET_BODY"));
    }

    #[test]
    fn grep_match_lines_are_redacted_directly_and_inside_sequences() {
        let grep_input = json!({
            "purpose": "find cache references",
            "op": "grep",
            "target": "daily:/",
            "q": "cache",
            "lines": "all"
        });
        let grep_result = Ok(CallToolResult::structured(json!({
            "space": "daily",
            "items": [{
                "path": "/note.md",
                "name": "note.md",
                "kind": "text",
                "match_lines": ["42: SECRET_MATCH_LINE"]
            }],
            "page": {"limit": 20, "returned": 1, "has_more": false}
        }))
        .into());
        let sequence_input = json!({
            "purpose": "grep notes in a sequence",
            "commands": [{
                "tool": "search",
                "op": "grep",
                "target": "daily:/",
                "q": "cache",
                "lines": "all"
            }]
        });
        let sequence_result = Ok(CallToolResult::structured(json!({
            "ok": true,
            "completed": 1,
            "failed": 0,
            "skipped": 0,
            "results": [{
                "index": 0,
                "tool": "search",
                "op": "grep",
                "ok": true,
                "result": {
                    "space": "daily",
                    "items": [{
                        "path": "/note.md",
                        "match_lines": ["42: SECRET_SEQUENCE_MATCH_LINE"]
                    }],
                    "page": {"limit": 20, "returned": 1, "has_more": false}
                }
            }]
        }))
        .into());

        let direct = redact_response("search", &grep_input, &grep_result);
        let sequence = redact_response("run_read_sequence", &sequence_input, &sequence_result);
        let direct_text = serialized(&direct);
        let sequence_text = serialized(&sequence);

        assert_eq!(
            direct["result"]["items"][0]["match_lines"]["category"],
            "document_content"
        );
        assert_eq!(
            sequence["result"]["results"][0]["result"]["items"][0]["match_lines"]["category"],
            "document_content"
        );
        assert_eq!(sequence["result"]["failed"], 0);
        assert_eq!(sequence["result"]["skipped"], 0);
        assert!(!direct_text.contains("SECRET_MATCH_LINE"));
        assert!(!sequence_text.contains("SECRET_SEQUENCE_MATCH_LINE"));
    }

    #[test]
    fn transfer_and_identity_responses_remove_credentials_and_pii() {
        let upload_input = json!({
            "purpose": "upload a report",
            "op": "begin_upload",
            "target": "daily:/report.pdf",
            "byte_len": 42,
            "media_type": "application/pdf",
            "original_filename": "SECRET_ORIGINAL_FILENAME.pdf",
            "encryption_mode": "server",
            "encryption_metadata": {"key": "SECRET_ENCRYPTION_METADATA"}
        });
        let transfer = Ok(CallToolResult::structured(json!({
            "target": "daily:/report.pdf",
            "transfer": {
                "method": "GET",
                "url": "SECRET_URL",
                "headers": {"authorization": "SECRET_HEADER"},
                "expires_in_seconds": 300
            },
            "node": {"path": "/report.pdf", "name": "report.pdf", "kind": "file"},
            "next_action": {"kind": "http_download", "instruction": "SECRET_INSTRUCTION", "transfer_field": "transfer"}
        })).into());
        let transfer_input = json!({"purpose": "download report", "target": "daily:/report.pdf"});
        let identity = Ok(CallToolResult::structured(json!({
            "account": {"id": "id", "kind": "user", "display_name": "SECRET_NAME"},
            "user": {"email": "SECRET_EMAIL"},
            "capabilities": {"can_create_space": true, "can_manage_agents": true},
            "server_version": "0.1.50"
        }))
        .into());

        let upload = redact_input("file_upload", &upload_input);
        let upload_text = serialized(&upload);
        let transfer_text = serialized(&redact_response(
            "file_download",
            &transfer_input,
            &transfer,
        ));
        let identity_text = serialized(&redact_response("me", &json!({}), &identity));

        assert_eq!(upload["encryption_mode"], "server");
        assert_eq!(upload["original_filename"]["category"], "original_filename");
        assert!(!upload_text.contains("SECRET_ORIGINAL_FILENAME"));
        assert!(!upload_text.contains("SECRET_ENCRYPTION_METADATA"));
        for secret in ["SECRET_URL", "SECRET_HEADER", "SECRET_INSTRUCTION"] {
            assert!(!transfer_text.contains(secret));
        }
        for secret in ["SECRET_NAME", "SECRET_EMAIL"] {
            assert!(!identity_text.contains(secret));
        }
    }

    #[test]
    fn protocol_errors_keep_codes_without_untrusted_messages() {
        let error = ErrorData::invalid_params(
            "failed near SECRET_PATTERN",
            Some(json!({
                "kind": "invalid_input",
                "code": "invalid_regex",
                "recoverable": true,
                "hint": "retry SECRET_PATTERN"
            })),
        );
        let result = Err(error);

        let redacted = redact_response("search", &json!({"op": "grep"}), &result);
        let text = serialized(&redacted);

        assert_eq!(redacted["error"]["code"], -32602);
        assert_eq!(redacted["error"]["data"]["code"], "invalid_regex");
        assert!(!text.contains("SECRET_PATTERN"));
    }

    #[test]
    fn sequence_errors_keep_safe_recovery_data_without_free_text() {
        let input = json!({
            "purpose": "retry a failed search",
            "commands": [{"tool": "search", "op": "grep", "target": "daily:/", "q": "cache"}]
        });
        let result = Ok(CallToolResult::structured(json!({
            "ok": false,
            "completed": 0,
            "failed": 1,
            "skipped": 0,
            "results": [{
                "index": 0,
                "tool": "search",
                "op": "grep",
                "ok": false,
                "error": {
                    "code": -32602,
                    "message": "SECRET_SEQUENCE_ERROR",
                    "data": {
                        "code": "invalid_input",
                        "recoverable": true,
                        "next_action": {
                            "kind": "retry",
                            "hint": "SECRET_SEQUENCE_HINT",
                            "input": {
                                "tool": "search",
                                "op": "grep",
                                "q": "SECRET_SEQUENCE_QUERY",
                                "target": "daily:/"
                            }
                        }
                    }
                }
            }]
        }))
        .into());

        let redacted = redact_response("run_read_sequence", &input, &result);
        let text = serialized(&redacted);

        assert_eq!(
            redacted["result"]["results"][0]["error"]["data"]["code"],
            "invalid_input"
        );
        assert_eq!(
            redacted["result"]["results"][0]["error"]["data"]["recoverable"],
            true
        );
        assert_eq!(
            redacted["result"]["results"][0]["error"]["data"]["next_action"]["input"]["q"]["category"],
            "search_query"
        );
        for secret in [
            "SECRET_SEQUENCE_ERROR",
            "SECRET_SEQUENCE_HINT",
            "SECRET_SEQUENCE_QUERY",
        ] {
            assert!(!text.contains(secret));
        }
    }

    #[test]
    fn sequence_preflight_logs_safe_nested_actions_without_error_text() {
        let input = json!({
            "purpose": "read notes",
            "commands": [{"tool": "read", "op": "read", "purpose": "wrong location"}]
        });
        let result = Err(ErrorData::invalid_params(
            "SECRET_PREFLIGHT_MESSAGE",
            Some(json!({
                "kind": "invalid_input",
                "code": "sequence_preflight_failed",
                "ok": false,
                "executed": false,
                "completed": 0,
                "failed": 0,
                "skipped": 0,
                "results": [],
                "next_action": {
                    "kind": "apply_error_actions",
                    "errors_field": "errors"
                },
                "errors": [{
                    "index": 0,
                    "path": "commands[0]",
                    "code": "sequence_command_purpose_not_allowed",
                    "message": "SECRET_NESTED_MESSAGE",
                    "hint": "SECRET_NESTED_HINT",
                    "next_action": {
                        "kind": "remove_fields",
                        "fields": ["commands[0].purpose"]
                    }
                }]
            })),
        ));

        let redacted = redact_response("run_read_sequence", &input, &result);
        let text = serialized(&redacted);

        assert_eq!(redacted["error"]["data"]["executed"], false);
        assert_eq!(redacted["error"]["data"]["ok"], false);
        assert_eq!(redacted["error"]["data"]["completed"], 0);
        assert_eq!(redacted["error"]["data"]["failed"], 0);
        assert_eq!(redacted["error"]["data"]["skipped"], 0);
        assert_eq!(
            redacted["error"]["data"]["next_action"]["kind"],
            "apply_error_actions"
        );
        assert_eq!(
            redacted["error"]["data"]["errors"][0]["code"],
            "sequence_command_purpose_not_allowed"
        );
        assert_eq!(
            redacted["error"]["data"]["errors"][0]["next_action"]["fields"][0],
            "commands[0].purpose"
        );
        for secret in [
            "SECRET_PREFLIGHT_MESSAGE",
            "SECRET_NESTED_MESSAGE",
            "SECRET_NESTED_HINT",
        ] {
            assert!(!text.contains(secret));
        }
    }

    #[test]
    fn sequence_responses_redact_read_content_and_write_diff() {
        let read_input = json!({
            "purpose": "read notes",
            "commands": [{"tool": "read", "op": "read", "target": "daily:/one.md"}]
        });
        let read_result = Ok(CallToolResult::structured(json!({
            "ok": true,
            "completed": 1,
            "failed": 0,
            "skipped": 0,
            "results": [{
                "index": 0,
                "tool": "read",
                "op": "read",
                "ok": true,
                "result": {
                    "space": "daily",
                    "path": "/one.md",
                    "content": "SECRET_SEQUENCE_CONTENT",
                    "content_sha256": "hash"
                }
            }]
        }))
        .into());
        let write_input = json!({
            "purpose": "patch notes",
            "commands": [{"tool": "write", "op": "patch", "target": "daily:/two.md", "edits": []}]
        });
        let write_result = Ok(CallToolResult::structured(json!({
            "ok": true,
            "completed": 1,
            "failed": 0,
            "skipped": 0,
            "results": [{
                "index": 0,
                "tool": "write",
                "op": "patch",
                "ok": true,
                "result": {
                    "space": "daily",
                    "path": "/two.md",
                    "diff": "SECRET_SEQUENCE_DIFF",
                    "content_sha256": "hash"
                }
            }]
        }))
        .into());

        let read = redact_response("run_read_sequence", &read_input, &read_result);
        let write = redact_response("run_write_sequence", &write_input, &write_result);
        let text = format!("{}{}", serialized(&read), serialized(&write));

        assert_eq!(
            read["result"]["results"][0]["result"]["content"]["category"],
            "document_content"
        );
        assert_eq!(
            write["result"]["results"][0]["result"]["diff"]["category"],
            "document_content"
        );
        assert!(!text.contains("SECRET_SEQUENCE_CONTENT"));
        assert!(!text.contains("SECRET_SEQUENCE_DIFF"));
    }

    #[test]
    fn unsupported_nested_sequences_and_me_arguments_are_fail_closed() {
        let sequence = json!({
            "purpose": "test nested commands",
            "commands": [{
                "tool": "run_read_sequence",
                "commands": [{"tool": "write", "content": "SECRET_NESTED_CONTENT"}]
            }]
        });
        let me = json!({
            "purpose": "SECRET_ME_PURPOSE",
            "unexpected": "SECRET_ME_ARGUMENT"
        });

        let sequence_text = serialized(&redact_input("run_read_sequence", &sequence));
        let me_text = serialized(&redact_input("me", &me));

        assert!(!sequence_text.contains("SECRET_NESTED_CONTENT"));
        assert!(!me_text.contains("SECRET_ME_PURPOSE"));
        assert!(!me_text.contains("SECRET_ME_ARGUMENT"));
    }

    #[test]
    fn oversized_redacted_responses_become_a_bounded_marker() {
        let items = (0..200)
            .map(|index| json!({"path": "x".repeat(2_048), "name": index.to_string()}))
            .collect::<Vec<_>>();
        let input = json!({"purpose": "list a large tree", "op": "tree", "target": "daily:/"});
        let result = Ok(CallToolResult::structured(json!({
            "space": "daily",
            "path": "/",
            "depth": 2,
            "items": items,
            "page": {"limit": 200, "returned": 200, "has_more": false}
        }))
        .into());

        let redacted = redact_response("read", &input, &result);

        assert_eq!(redacted["_truncated"], true);
        assert_eq!(redacted["category"], "snapshot_size_limit");
        assert_eq!(redacted["limit_bytes"], SNAPSHOT_MAX_BYTES);
        assert!(serialized(&redacted).len() < 1_024);
    }
}
