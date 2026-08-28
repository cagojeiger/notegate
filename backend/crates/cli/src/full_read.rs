use notegate_command::{FULL_TEXT_READ_MAX_BYTES, FULL_TEXT_READ_MAX_LINES};
use serde_json::{Value, json};

use crate::CliError;
use crate::checksum::sha256_hex;

pub(crate) fn prepare_input(input: &mut Value) -> Result<(), CliError> {
    if input.get("op").and_then(Value::as_str) != Some("read") {
        return Err(CliError::invalid_input(
            "full_read_requires_read_operation",
            "--all is only valid when read input uses op=read",
        ));
    }
    for field in ["start_line", "max_lines", "max_bytes"] {
        if input.get(field).is_some() {
            return Err(CliError::invalid_input(
                "full_read_range_conflict",
                format!("--all cannot be combined with the {field} input field"),
            ));
        }
    }
    if input.get("if_none_match_sha256").is_some() {
        return Err(CliError::invalid_input(
            "full_read_conditional_conflict",
            "--all cannot be combined with if_none_match_sha256 because an unchanged response does not contain the complete Text",
        ));
    }
    let Some(input) = input.as_object_mut() else {
        return Err(CliError::invalid_input(
            "invalid_read_input",
            "read input must be a JSON object",
        ));
    };
    input.insert("start_line".to_owned(), json!(1));
    input.insert("max_lines".to_owned(), json!(FULL_TEXT_READ_MAX_LINES));
    input.insert("max_bytes".to_owned(), json!(FULL_TEXT_READ_MAX_BYTES));
    Ok(())
}

pub(crate) fn verify_response(response: &Value) -> Result<(), CliError> {
    let content = response.get("content").and_then(Value::as_str);
    let byte_len = response.get("byte_len").and_then(Value::as_u64);
    let line_count = response.get("line_count").and_then(Value::as_u64);
    let returned_lines = response.get("returned_lines").and_then(Value::as_u64);
    let content_sha256 = response.get("content_sha256").and_then(Value::as_str);
    let truncated = response.get("truncated").and_then(Value::as_bool);
    let complete = match (
        content,
        byte_len,
        line_count,
        returned_lines,
        content_sha256,
        truncated,
    ) {
        (
            Some(content),
            Some(byte_len),
            Some(line_count),
            Some(returned_lines),
            Some(expected_sha256),
            Some(false),
        ) => {
            content.len() as u64 == byte_len
                && returned_lines == line_count
                && sha256_hex(content.as_bytes()) == expected_sha256
        }
        _ => false,
    };
    if complete {
        return Ok(());
    }
    Err(CliError::recoverable_protocol(
        "incomplete_full_read",
        "NoteGate did not return a verifiably complete Text for --all",
        "Do not use this content for a write. Retry a normal read and follow next_action pagination, or update the CLI and server before retrying --all",
    ))
}
