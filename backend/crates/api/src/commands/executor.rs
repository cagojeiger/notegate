//! Transport-neutral dispatch and input-only validation for NoteGate commands.

use notegate_command::{
    CommandError, LineEditInput, ManageInput, PatchEdit, ReadInput, RecoveryAction, SearchInput,
    WriteInput,
};
use notegate_core::validation::validate_space_name;
use notegate_search::{validate_find_input, validate_grep_input};
use notegate_service::ServiceError;
use notegate_service::files::{
    Target, content, parse_target, validate_structured_text,
    validation::{validate_basename, validate_text_content},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::error::validate_purpose;
use super::resolve::{
    actionable_input_error, invalid_input_error, required_input, search_error, service_error,
    split_parent_name,
};
use super::{CommandContext, events, files, search, spaces};
use crate::state::AppState;

pub async fn read(
    state: &AppState,
    context: &CommandContext,
    input: ReadInput,
) -> Result<Value, CommandError> {
    validate_read_operation(&input)?;
    match input.op.as_str() {
        "spaces" => spaces::list(state, context, input.name, input.limit, input.cursor).await,
        "ls" => {
            files::list(
                state,
                context,
                required(input.target, "target", "ls")?,
                Some(1),
                input.limit,
                input.cursor,
            )
            .await
        }
        "tree" => {
            files::list(
                state,
                context,
                required(input.target, "target", "tree")?,
                Some(input.depth.unwrap_or(5)),
                input.limit,
                input.cursor,
            )
            .await
        }
        "stat" => files::stat(state, context, required(input.target, "target", "stat")?).await,
        "read" => {
            files::read(
                state,
                context,
                required(input.target, "target", "read")?,
                input.start_line,
                input.max_lines,
                input.max_bytes,
                input.if_none_match_sha256,
            )
            .await
        }
        "changes" => {
            events::call(
                state,
                context,
                &input.purpose,
                required(input.target, "target", "changes")?,
                input.limit,
                input.direction,
                input.cursor,
            )
            .await
        }
        _ => Err(invalid_op(
            "read",
            &["spaces", "ls", "tree", "stat", "read", "changes"],
        )),
    }
}

pub(crate) fn validate_read_operation(input: &ReadInput) -> Result<(), CommandError> {
    validate_purpose(&input.purpose)?;
    validate_read_change_fields(input)?;
    match input.op.as_str() {
        "spaces" => {
            if let Some(name) = input.name.as_deref() {
                validate_space_name(name)
                    .map_err(|error| invalid_input_error(error.to_string()))?;
            }
            Ok(())
        }
        "ls" | "stat" => {
            parse_input_target(required_ref(
                input.target.as_ref(),
                "target",
                input.op.as_str(),
            )?)?;
            Ok(())
        }
        "tree" => {
            parse_input_target(required_ref(input.target.as_ref(), "target", "tree")?)?;
            if input.depth.is_some_and(|depth| depth < 1) {
                return Err(invalid_input_error("depth must be at least 1"));
            }
            Ok(())
        }
        "read" => {
            parse_input_target(required_ref(input.target.as_ref(), "target", "read")?)?;
            if input.max_bytes == Some(0) {
                return Err(invalid_input_error("max_bytes must be at least 1"));
            }
            Ok(())
        }
        "changes" => {
            events::validate_input(
                required_ref(input.target.as_ref(), "target", "changes")?,
                input.direction.as_deref(),
                input.cursor.as_deref(),
                &input.purpose,
            )?;
            Ok(())
        }
        _ => Err(invalid_op(
            "read",
            &["spaces", "ls", "tree", "stat", "read", "changes"],
        )),
    }
}

fn validate_read_change_fields(input: &ReadInput) -> Result<(), CommandError> {
    if input.op == "changes" {
        return Ok(());
    }
    if input.direction.is_some() {
        return Err(actionable_input_error(
            "changes_fields_not_allowed",
            "direction is only valid for read op=changes",
            "Remove direction or change op to changes.",
            RecoveryAction::RemoveFields {
                fields: vec!["direction".to_owned()],
            },
        ));
    }
    Ok(())
}

pub async fn search(
    state: &AppState,
    context: &CommandContext,
    input: SearchInput,
) -> Result<Value, CommandError> {
    validate_search_operation(&input)?;
    match input.op.as_str() {
        "find" => {
            search::find(
                state,
                context,
                input.target,
                input.q,
                input.kind,
                input.match_mode,
                input.include,
                input.exclude,
                input.limit,
                input.cursor,
            )
            .await
        }
        "grep" => {
            search::grep(
                state,
                context,
                input.target,
                input.q,
                input.match_mode,
                input.lines,
                input.include,
                input.exclude,
                input.limit,
                input.cursor,
            )
            .await
        }
        _ => Err(invalid_op("search", &["find", "grep"])),
    }
}

pub(crate) fn validate_search_operation(input: &SearchInput) -> Result<(), CommandError> {
    validate_purpose(&input.purpose)?;
    match input.op.as_str() {
        "find" => {
            parse_input_target(&input.target)?;
            if let Some(kind) = input.kind.as_deref() {
                search::parse_kind(kind)?;
            }
            let match_mode = search::parse_find_match_mode(input.match_mode.as_deref())?;
            validate_find_input(
                &input.q,
                match_mode,
                input.include.as_deref().unwrap_or_default(),
                input.exclude.as_deref().unwrap_or_default(),
            )
            .map_err(search_error)?;
            Ok(())
        }
        "grep" => {
            parse_input_target(&input.target)?;
            let match_mode = search::parse_grep_match_mode(input.match_mode.as_deref())?;
            search::parse_grep_line_mode(input.lines.as_deref())?;
            validate_grep_input(
                &input.q,
                match_mode,
                input.include.as_deref().unwrap_or_default(),
                input.exclude.as_deref().unwrap_or_default(),
            )
            .map_err(search_error)?;
            Ok(())
        }
        _ => Err(invalid_op("search", &["find", "grep"])),
    }
}

pub async fn write(
    state: &AppState,
    context: &CommandContext,
    input: WriteInput,
) -> Result<Value, CommandError> {
    validate_write_operation(&input)?;
    match input.op.as_str() {
        "write" => {
            files::write(
                state,
                context,
                input.target,
                required(input.content, "content", "write")?,
                input.create,
                input.expected_sha256,
            )
            .await
        }
        "append" => {
            files::append(
                state,
                context,
                input.target,
                required(input.content, "content", "append")?,
                input.create,
                input.ensure_newline,
                input.expected_sha256,
            )
            .await
        }
        "patch" => {
            files::patch(
                state,
                context,
                input.target,
                parse_edits(input.edits, "patch")?,
                input.expected_sha256,
            )
            .await
        }
        "edit" => {
            files::edit(
                state,
                context,
                input.target,
                parse_edits(input.edits, "edit")?,
                input.expected_sha256,
            )
            .await
        }
        _ => Err(invalid_op("write", &["write", "append", "patch", "edit"])),
    }
}

pub(crate) fn validate_write_operation(input: &WriteInput) -> Result<(), CommandError> {
    validate_purpose(&input.purpose)?;
    match input.op.as_str() {
        "write" | "append" => {
            required_ref(input.content.as_ref(), "content", input.op.as_str())?;
            validate_text_target(&input.target)?;
            Ok(())
        }
        "patch" => {
            let edits = parse_edits::<PatchEdit>(input.edits.clone(), "patch")?;
            files::prepare_patch_edits(&edits)?;
            validate_text_target(&input.target)?;
            Ok(())
        }
        "edit" => {
            let edits = parse_edits::<LineEditInput>(input.edits.clone(), "edit")?;
            files::prepare_line_edits(&edits)?;
            validate_text_target(&input.target)?;
            Ok(())
        }
        _ => Err(invalid_op("write", &["write", "append", "patch", "edit"])),
    }
}

pub(crate) fn validate_static_write_content(input: &WriteInput) -> Result<(), CommandError> {
    let content = match input.op.as_str() {
        "write" | "append" => required_ref(input.content.as_ref(), "content", &input.op)?,
        _ => return Ok(()),
    };
    let metrics = content::compute(content);
    validate_text_content(metrics.byte_len, metrics.line_count)
        .map_err(ServiceError::from)
        .map_err(service_error)?;

    if input.op == "write" {
        let target = parse_input_target(&input.target)?;
        let (_, name) = split_parent_name(&target.path)?;
        validate_structured_text(&name, content).map_err(service_error)?;
    }
    Ok(())
}

pub async fn manage(
    state: &AppState,
    context: &CommandContext,
    input: ManageInput,
) -> Result<Value, CommandError> {
    validate_manage_operation(&input)?;
    match input.op.as_str() {
        "mkdir" => {
            files::mkdir(
                state,
                context,
                required(input.target, "target", "mkdir")?,
                input.parents,
            )
            .await
        }
        "mv" => {
            files::mv(
                state,
                context,
                required(input.source, "source", "mv")?,
                required(input.destination, "destination", "mv")?,
            )
            .await
        }
        "cp" => {
            files::copy(
                state,
                context,
                required(input.source, "source", "cp")?,
                required(input.destination, "destination", "cp")?,
                input.recursive,
            )
            .await
        }
        "rm" => {
            files::rm(
                state,
                context,
                required(input.target, "target", "rm")?,
                input.recursive,
            )
            .await
        }
        _ => Err(invalid_op("manage", &["mkdir", "mv", "cp", "rm"])),
    }
}

pub(crate) fn validate_manage_operation(input: &ManageInput) -> Result<(), CommandError> {
    validate_purpose(&input.purpose)?;
    match input.op.as_str() {
        "mkdir" => {
            let target =
                parse_input_target(required_ref(input.target.as_ref(), "target", "mkdir")?)?;
            if !input.parents {
                validate_non_root_target(&target)?;
            }
            Ok(())
        }
        "rm" => {
            let target = parse_input_target(required_ref(input.target.as_ref(), "target", "rm")?)?;
            validate_non_root_target(&target)?;
            Ok(())
        }
        "mv" | "cp" => {
            let source = parse_input_target(required_ref(
                input.source.as_ref(),
                "source",
                input.op.as_str(),
            )?)?;
            let destination = parse_input_target(required_ref(
                input.destination.as_ref(),
                "destination",
                input.op.as_str(),
            )?)?;
            if source.space != destination.space {
                return Err(invalid_input_error(
                    "source and destination must be in the same space",
                ));
            }
            validate_non_root_target(&source)?;
            validate_non_root_target(&destination)?;
            Ok(())
        }
        _ => Err(invalid_op("manage", &["mkdir", "mv", "cp", "rm"])),
    }
}

fn validate_non_root_target(target: &Target) -> Result<(), CommandError> {
    split_parent_name(&target.path).map(|_| ())
}

fn parse_input_target(target: &str) -> Result<Target, CommandError> {
    let target = parse_target(target).map_err(|error| invalid_input_error(error.to_string()))?;
    for segment in target.path.split('/').filter(|segment| !segment.is_empty()) {
        validate_basename(segment)
            .map_err(ServiceError::from)
            .map_err(service_error)?;
    }
    Ok(target)
}

fn validate_text_target(target: &str) -> Result<(), CommandError> {
    let target = parse_input_target(target)?;
    split_parent_name(&target.path)?;
    Ok(())
}

fn required<T>(value: Option<T>, field: &'static str, op: &'static str) -> Result<T, CommandError> {
    required_input(value, field, &format!("op={op}"))
}

fn required_ref<'a, T>(
    value: Option<&'a T>,
    field: &'static str,
    op: &str,
) -> Result<&'a T, CommandError> {
    required_input(value, field, &format!("op={op}"))
}

fn parse_edits<T>(value: Option<Vec<Value>>, op: &'static str) -> Result<Vec<T>, CommandError>
where
    T: DeserializeOwned,
{
    let edits = value.ok_or_else(|| {
        invalid_input_error(format!(
            "op={op} requires edits; retry with a non-empty `edits` array"
        ))
    })?;
    edits
        .into_iter()
        .map(|edit| {
            serde_json::from_value(edit).map_err(|error| {
                invalid_input_error(format!("invalid edit entry for op={op}: {error}"))
            })
        })
        .collect()
}

fn invalid_op(tool: &'static str, allowed: &[&str]) -> CommandError {
    actionable_input_error(
        "invalid_op",
        format!(
            "invalid op for {tool}; allowed values are: {}",
            allowed.join(", ")
        ),
        "Choose one of the operation values listed by next_action.choices.",
        RecoveryAction::ChooseValue {
            field: "op".to_owned(),
            choices: allowed.iter().map(|value| json!(value)).collect(),
        },
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use notegate_command::{CommandErrorClass, ManageInput, ReadInput, SearchInput, WriteInput};
    use notegate_db::{SpaceRepo, test_support::TestDb};
    use notegate_service::files::{WriteTarget, WriteText, WriteTextBody};
    use serde_json::json;

    use super::*;

    #[test]
    fn read_change_fields_are_rejected_outside_changes() {
        let error = validate_read_operation(&ReadInput {
            purpose: "inspect tree".to_owned(),
            op: "tree".to_owned(),
            target: Some("daily:/".to_owned()),
            name: None,
            depth: None,
            limit: None,
            cursor: None,
            direction: Some("older".to_owned()),
            start_line: None,
            max_lines: None,
            max_bytes: None,
            if_none_match_sha256: None,
        })
        .expect_err("direction is changes-only");

        assert_eq!(error.class, CommandErrorClass::InvalidParams);
        assert_eq!(
            error.data.expect("action data")["code"],
            "changes_fields_not_allowed"
        );
    }

    #[test]
    fn cross_space_move_is_rejected_before_execution() {
        let error = validate_manage_operation(&ManageInput {
            purpose: "move note".to_owned(),
            op: "mv".to_owned(),
            target: None,
            source: Some("daily:/a.md".to_owned()),
            destination: Some("research:/a.md".to_owned()),
            parents: false,
            recursive: false,
        })
        .expect_err("cross-space move is invalid");

        assert_eq!(error.class, CommandErrorClass::InvalidParams);
        assert!(error.message.contains("same space"));
    }

    #[test]
    fn every_shared_executor_rejects_an_invalid_purpose() -> Result<(), Box<dyn std::error::Error>>
    {
        let read: ReadInput = serde_json::from_value(json!({
            "purpose": "",
            "op": "spaces"
        }))?;
        let search: SearchInput = serde_json::from_value(json!({
            "purpose": " padded ",
            "op": "find",
            "target": "daily:/",
            "q": "note"
        }))?;
        let write: WriteInput = serde_json::from_value(json!({
            "purpose": "가".repeat(notegate_command::PURPOSE_MAX_CHARS + 1),
            "op": "write",
            "target": "daily:/note.md",
            "content": "body"
        }))?;
        let manage: ManageInput = serde_json::from_value(json!({
            "purpose": "",
            "op": "mkdir",
            "target": "daily:/folder"
        }))?;

        let errors = [
            validate_read_operation(&read).expect_err("read purpose is validated"),
            validate_search_operation(&search).expect_err("search purpose is validated"),
            validate_write_operation(&write).expect_err("write purpose is validated"),
            validate_manage_operation(&manage).expect_err("manage purpose is validated"),
        ];
        for error in errors {
            assert_eq!(error.class, CommandErrorClass::InvalidParams);
            assert!(error.message.contains("purpose"));
        }
        Ok(())
    }

    #[tokio::test]
    async fn shared_command_file_flow_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, space_id, _root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;
        SpaceRepo::new(state.db.clone())
            .update_space(space_id, caller.account_id(), None, None, Some(true))
            .await?;
        let context = CommandContext::new(caller, None);

        let created = manage(
            &state,
            &context,
            serde_json::from_value(json!({
                "purpose": "create a nested notes folder",
                "op": "mkdir",
                "target": "rest-test:/docs/notes",
                "parents": true
            }))?,
        )
        .await
        .expect("recursive mkdir succeeds");
        assert_eq!(created["node"]["path"], "/docs/notes");
        assert_eq!(created["created_paths"], json!(["/docs", "/docs/notes"]));

        let missing_error = write(
            &state,
            &context,
            serde_json::from_value(json!({
                "purpose": "confirm explicit creation is required",
                "op": "write",
                "target": "rest-test:/docs/notes/log.md",
                "content": "line1"
            }))?,
        )
        .await
        .expect_err("a missing target without create=true must fail");
        assert_eq!(missing_error.class, CommandErrorClass::InvalidParams);
        assert_eq!(
            missing_error.data.expect("not-found metadata")["kind"],
            "not_found"
        );

        let written = write(
            &state,
            &context,
            serde_json::from_value(json!({
                "purpose": "create the first log line",
                "op": "write",
                "target": "rest-test:/docs/notes/log.md",
                "content": "line1",
                "create": true
            }))?,
        )
        .await
        .expect("write with create=true succeeds");
        assert_eq!(written["node"]["path"], "/docs/notes/log.md");
        assert_eq!(written["byte_len"], 5);
        assert!(written["content_sha256"].is_string());

        let appended = write(
            &state,
            &context,
            serde_json::from_value(json!({
                "purpose": "append the second log line",
                "op": "append",
                "target": "rest-test:/docs/notes/log.md",
                "content": "line2",
                "ensure_newline": true
            }))?,
        )
        .await
        .expect("append succeeds");
        assert_eq!(appended["appended"], true);
        assert_eq!(appended["byte_len"], 11);

        let read_back = read(
            &state,
            &context,
            serde_json::from_value(json!({
                "purpose": "verify the completed log",
                "op": "read",
                "target": "rest-test:/docs/notes/log.md"
            }))?,
        )
        .await
        .expect("read succeeds");
        assert_eq!(read_back["content"], "line1\nline2");
        assert_eq!(read_back["content_sha256"], appended["content_sha256"]);
        assert_eq!(read_back["byte_len"], 11);
        assert_eq!(read_back["truncated"], false);

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn shared_command_write_preserves_content_guards()
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
        let account_id = caller.account_id();
        let context = CommandContext::new(caller, None);

        write(
            &state,
            &context,
            serde_json::from_value(json!({
                "purpose": "create guarded text",
                "op": "write",
                "target": "rest-test:/guarded.md",
                "content": "current",
                "create": true
            }))?,
        )
        .await
        .expect("initial guarded write succeeds");

        let conflict = write(
            &state,
            &context,
            serde_json::from_value(json!({
                "purpose": "reject a stale overwrite",
                "op": "write",
                "target": "rest-test:/guarded.md",
                "content": "replacement",
                "expected_sha256": "stale-sha256"
            }))?,
        )
        .await
        .expect_err("a stale optimistic-write guard must fail");
        assert_eq!(conflict.class, CommandErrorClass::InvalidRequest);
        assert_eq!(
            conflict.data.expect("conflict metadata")["kind"],
            "conflict"
        );

        state
            .files
            .write_text(
                account_id,
                space_id,
                WriteText {
                    target: WriteTarget::Create {
                        parent_node_id: root_id,
                        name: "secret.bin".to_owned(),
                    },
                    body: WriteTextBody::Encrypted(json!({"ct": "opaque"})),
                    expected_sha256: None,
                },
            )
            .await?;

        let encrypted_error = write(
            &state,
            &context,
            serde_json::from_value(json!({
                "purpose": "reject plaintext over encrypted text",
                "op": "write",
                "target": "rest-test:/secret.bin",
                "content": "plaintext"
            }))?,
        )
        .await
        .expect_err("encrypted text must reject a plaintext overwrite");
        assert!(
            encrypted_error
                .message
                .contains("encrypted text cannot be modified"),
            "unexpected error: {}",
            encrypted_error.message
        );

        db.cleanup().await;
        Ok(())
    }
}
