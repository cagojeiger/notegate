//! Path-first MCP control plane for direct object upload and download transfers.

use std::collections::HashSet;

use axum::http::request::Parts;
use notegate_model::FileEncryptionMode;
use notegate_model::files::{BeginObjectUpload, ObjectUploadMode, ObjectUploadRegistration};
use rmcp::{ErrorData, Json};
use serde_json::{Value, json};
use uuid::Uuid;

use super::resolve::{
    caller, invalid_input_error, node_summary, resolve_target, service_error, split_parent_name,
};
use super::unified::{CompletedPartInput, FileTransferInput};
use crate::object_storage::{
    CompletedUploadPart, MCP_TRANSFER_URL_TTL, MULTIPART_PART_SIZE, ObjectStorageError,
    multipart_part_count, multipart_part_len, uses_multipart,
};
use crate::state::AppState;

const PART_URL_BATCH_MAX: usize = 16;
const PART_UPLOAD_CONCURRENCY_MAX: usize = 4;

pub async fn call(
    state: &AppState,
    parts: &Parts,
    input: FileTransferInput,
) -> Result<Json<Value>, ErrorData> {
    match input.op.as_str() {
        "begin_upload" => begin_upload(state, parts, input).await,
        "prepare_parts" => prepare_parts(state, parts, input).await,
        "complete_upload" => complete_upload(state, parts, input).await,
        "abort_upload" => abort_upload(state, parts, input).await,
        "prepare_download" => prepare_download(state, parts, input).await,
        _ => Err(invalid_input_error(
            "invalid op for file_transfer; allowed values are: begin_upload, prepare_parts, complete_upload, abort_upload, prepare_download",
        )),
    }
}

async fn begin_upload(
    state: &AppState,
    parts: &Parts,
    input: FileTransferInput,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let target = required(input.target, "target", "begin_upload")?;
    let byte_len = input
        .byte_len
        .ok_or_else(|| invalid_input_error("op=begin_upload requires byte_len"))?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let (parent_path, name) = split_parent_name(&path)?;
    let parent = state
        .files
        .resolve_path(caller.account_id(), resolved.space_id(), &parent_path)
        .await
        .map_err(service_error)?;
    let encryption_mode = input
        .encryption_mode
        .as_deref()
        .map(FileEncryptionMode::parse)
        .unwrap_or(Some(FileEncryptionMode::None))
        .ok_or_else(|| invalid_input_error("encryption_mode must be 'none' or 'client'"))?;
    let command = BeginObjectUpload {
        parent_node_id: parent.node.id,
        name,
        byte_len,
        media_type: input
            .media_type
            .unwrap_or_else(|| "application/octet-stream".to_owned()),
        original_filename: input.original_filename,
        encryption_mode,
        encryption_metadata: input.encryption_metadata,
    };
    state
        .files
        .prepare_object_upload(caller.account_id(), resolved.space_id(), &command)
        .await
        .map_err(service_error)?;

    let upload_id = Uuid::new_v4();
    let object_key = format!("objects/{upload_id}");
    if uses_multipart(byte_len) {
        let part_count = multipart_part_count(byte_len, MULTIPART_PART_SIZE)
            .ok_or_else(|| invalid_input_error("file is too large for multipart upload"))?;
        let storage_upload_id = state
            .object_storage
            .create_multipart_upload(&object_key, &command.media_type)
            .await
            .map_err(storage_error)?;
        let registration = ObjectUploadRegistration {
            id: upload_id,
            object_key: object_key.clone(),
            upload_mode: ObjectUploadMode::Multipart,
            multipart_upload_id: Some(storage_upload_id.clone()),
            multipart_part_size: Some(MULTIPART_PART_SIZE),
        };
        let recorded = state
            .files
            .record_registered_object_upload(
                &registration,
                caller.account_id(),
                resolved.space_id(),
                &command,
            )
            .await;
        if let Err(error) = recorded {
            if let Err(abort_error) = state
                .object_storage
                .abort_multipart_upload(&object_key, &storage_upload_id)
                .await
            {
                tracing::error!(
                    event = "object_storage.multipart_registration_cleanup_failed",
                    %upload_id,
                    %object_key,
                    ?abort_error,
                );
            }
            return Err(service_error(error));
        }
        tracing::info!(
            event = "object_storage.upload_created",
            %upload_id,
            space_id = %resolved.space_id(),
            account_id = %caller.account_id(),
            byte_len,
            upload_mode = "multipart",
        );
        let first_part_numbers =
            (1..=part_count.min(PART_URL_BATCH_MAX as i32)).collect::<Vec<_>>();
        return Ok(Json(json!({
            "upload_id": upload_id,
            "target": target,
            "transfer": {
                "mode": "multipart",
                "part_size": MULTIPART_PART_SIZE,
                "part_count": part_count,
            },
            "next_action": {
                "kind": "call_tool",
                "tool": "file_transfer",
                "input": {
                    "op": "prepare_parts",
                    "upload_id": upload_id,
                    "part_numbers": first_part_numbers,
                },
                "instruction": "Request upload URLs for the first part batch.",
            },
        })));
    }

    let registration = ObjectUploadRegistration {
        id: upload_id,
        object_key: object_key.clone(),
        upload_mode: ObjectUploadMode::Single,
        multipart_upload_id: None,
        multipart_part_size: None,
    };
    let transfer = state
        .object_storage
        .presign_put_with_ttl(
            &object_key,
            &command.media_type,
            command.byte_len,
            MCP_TRANSFER_URL_TTL,
        )
        .await
        .map_err(storage_error)?;
    state
        .files
        .record_registered_object_upload(
            &registration,
            caller.account_id(),
            resolved.space_id(),
            &command,
        )
        .await
        .map_err(service_error)?;
    tracing::info!(
        event = "object_storage.upload_created",
        %upload_id,
        space_id = %resolved.space_id(),
        account_id = %caller.account_id(),
        byte_len,
        upload_mode = "single",
    );
    Ok(Json(json!({
        "upload_id": upload_id,
        "target": target,
        "transfer": {
            "mode": "single",
            "method": "PUT",
            "url": transfer.url,
            "headers": transfer.headers,
            "content_length": byte_len,
            "expires_in_seconds": MCP_TRANSFER_URL_TTL.as_secs(),
        },
        "next_action": {
            "kind": "http_upload",
            "transfer_field": "transfer",
            "instruction": "PUT the local file using transfer.method, transfer.url, every transfer.headers entry, and the exact transfer.content_length.",
            "then": {
                "tool": "file_transfer",
                "input": {
                    "op": "complete_upload",
                    "upload_id": upload_id,
                },
            },
        },
    })))
}

async fn prepare_parts(
    state: &AppState,
    parts: &Parts,
    input: FileTransferInput,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let upload_id = upload_id(&input)?;
    let part_numbers = input
        .part_numbers
        .filter(|numbers| !numbers.is_empty())
        .ok_or_else(|| invalid_input_error("op=prepare_parts requires part_numbers"))?;
    if part_numbers.len() > PART_URL_BATCH_MAX {
        return Err(invalid_input_error(format!(
            "prepare_parts accepts at most {PART_URL_BATCH_MAX} part numbers"
        )));
    }
    let unique: HashSet<i32> = part_numbers.iter().copied().collect();
    if unique.len() != part_numbers.len() {
        return Err(invalid_input_error(
            "part_numbers must not contain duplicates",
        ));
    }
    let upload = state
        .files
        .object_upload_by_id(caller.account_id(), upload_id)
        .await
        .map_err(service_error)?;
    if upload.upload_mode != ObjectUploadMode::Multipart {
        return Err(invalid_input_error("upload is not multipart"));
    }
    let upload = state
        .files
        .touch_object_upload(caller.account_id(), upload.space_id, upload_id)
        .await
        .map_err(service_error)?;
    if upload.node_id.is_some() {
        return Err(invalid_input_error("upload is already complete"));
    }
    let storage_upload_id = upload
        .multipart_upload_id
        .as_deref()
        .ok_or_else(|| ErrorData::internal_error("multipart upload state is incomplete", None))?;
    let part_size = upload
        .multipart_part_size
        .ok_or_else(|| ErrorData::internal_error("multipart upload state is incomplete", None))?;

    let mut transfers = Vec::with_capacity(part_numbers.len());
    for part_number in part_numbers {
        let content_length = multipart_part_len(upload.byte_len, part_size, part_number)
            .ok_or_else(|| invalid_input_error("part number is outside the upload range"))?;
        let transfer = state
            .object_storage
            .presign_upload_part(
                &upload.object_key,
                storage_upload_id,
                part_number,
                content_length,
                MCP_TRANSFER_URL_TTL,
            )
            .await
            .map_err(storage_error)?;
        transfers.push(json!({
            "part_number": part_number,
            "method": "PUT",
            "url": transfer.url,
            "headers": transfer.headers,
            "content_length": content_length,
            "expires_in_seconds": MCP_TRANSFER_URL_TTL.as_secs(),
        }));
    }
    Ok(Json(json!({
        "upload_id": upload_id,
        "parts": transfers,
        "next_action": {
            "kind": "http_upload_parts",
            "transfers_field": "parts",
            "collect_response_header": "etag",
            "max_concurrency": PART_UPLOAD_CONCURRENCY_MAX,
            "instruction": "PUT at most 4 parts concurrently using each URL, headers, and exact content_length. Collect every response ETag, retry only failed parts with fresh URLs, request URLs for any remaining parts, then complete with all part_number/etag pairs.",
            "repeat": {
                "tool": "file_transfer",
                "input": {
                    "op": "prepare_parts",
                    "upload_id": upload_id,
                },
                "when": "parts remain unuploaded or a part URL expired",
                "requires": "add the needed part_numbers to input",
            },
            "then": {
                "tool": "file_transfer",
                "input": {
                    "op": "complete_upload",
                    "upload_id": upload_id,
                },
                "requires": "completed_parts for every part exactly once",
            },
        },
    })))
}

async fn complete_upload(
    state: &AppState,
    parts: &Parts,
    input: FileTransferInput,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let upload_id = upload_id(&input)?;
    let upload = state
        .files
        .object_upload_by_id(caller.account_id(), upload_id)
        .await
        .map_err(service_error)?;
    if upload.node_id.is_none() {
        // Refresh before any provider call so a stale-upload cleanup claim
        // cannot abort this upload while completion is in progress.
        state
            .files
            .touch_object_upload(caller.account_id(), upload.space_id, upload_id)
            .await
            .map_err(service_error)?;
        if upload.upload_mode == ObjectUploadMode::Multipart {
            let completed = validate_completed_parts(&upload, input.completed_parts)?;
            let storage_upload_id = upload.multipart_upload_id.as_deref().ok_or_else(|| {
                ErrorData::internal_error("multipart upload state is incomplete", None)
            })?;
            if let Err(completion_error) = state
                .object_storage
                .complete_multipart_upload(&upload.object_key, storage_upload_id, &completed)
                .await
            {
                // A concurrent completion may have already consumed the provider
                // upload id. A matching final object makes completion idempotent.
                match state
                    .object_storage
                    .verify_upload(&upload.object_key, upload.byte_len)
                    .await
                {
                    Ok(_) => {}
                    Err(ObjectStorageError::Missing) => {
                        return Err(storage_error(completion_error));
                    }
                    Err(error) => return Err(storage_error(error)),
                }
            }
        } else if input.completed_parts.is_some() {
            return Err(invalid_input_error(
                "single uploads do not accept completed_parts",
            ));
        }
        state
            .object_storage
            .verify_upload(&upload.object_key, upload.byte_len)
            .await
            .map_err(storage_error)?;
    }
    let view = state
        .files
        .complete_object_upload(caller.account_id(), upload.space_id, upload_id)
        .await
        .map_err(service_error)?;
    tracing::info!(
        event = "object_storage.file_attached",
        %upload_id,
        node_id = %view.node.node.id,
        space_id = %upload.space_id,
    );
    Ok(Json(json!({
        "upload_id": upload_id,
        "node": node_summary(&view.node),
        "next_action": {
            "kind": "done",
        },
    })))
}

async fn abort_upload(
    state: &AppState,
    parts: &Parts,
    input: FileTransferInput,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let upload_id = upload_id(&input)?;
    let upload = state
        .files
        .object_upload_by_id(caller.account_id(), upload_id)
        .await
        .map_err(service_error)?;
    state
        .files
        .cancel_object_upload(caller.account_id(), upload.space_id, upload_id)
        .await
        .map_err(service_error)?;
    tracing::info!(
        event = "object_storage.upload_aborted",
        %upload_id,
        space_id = %upload.space_id,
        account_id = %caller.account_id(),
    );
    Ok(Json(json!({
        "upload_id": upload_id,
        "status": "cleanup_queued",
        "next_action": {
            "kind": "done",
        },
    })))
}

async fn prepare_download(
    state: &AppState,
    parts: &Parts,
    input: FileTransferInput,
) -> Result<Json<Value>, ErrorData> {
    let caller = caller(parts)?;
    let target = required(input.target, "target", "prepare_download")?;
    let (resolved, path) = resolve_target(state, caller, &target).await?;
    let node = state
        .files
        .resolve_path(caller.account_id(), resolved.space_id(), &path)
        .await
        .map_err(service_error)?;
    let file = state
        .files
        .file_for_download(caller.account_id(), resolved.space_id(), node.node.id)
        .await
        .map_err(service_error)?;
    let url = state
        .object_storage
        .presign_get_with_ttl(
            &file.file.object_key,
            file.file.original_filename.as_deref(),
            MCP_TRANSFER_URL_TTL,
        )
        .await
        .map_err(storage_error)?;
    Ok(Json(json!({
        "target": target,
        "transfer": {
            "method": "GET",
            "url": url,
            "headers": {},
            "expires_in_seconds": MCP_TRANSFER_URL_TTL.as_secs(),
        },
        "node": node_summary(&file.node),
        "next_action": {
            "kind": "http_download",
            "transfer_field": "transfer",
            "instruction": "GET transfer.url with every transfer.headers entry and write the response bytes to the intended local file.",
        },
    })))
}

fn validate_completed_parts(
    upload: &notegate_model::files::PendingObjectUpload,
    parts: Option<Vec<CompletedPartInput>>,
) -> Result<Vec<CompletedUploadPart>, ErrorData> {
    let part_size = upload
        .multipart_part_size
        .ok_or_else(|| ErrorData::internal_error("multipart upload state is incomplete", None))?;
    let part_count = multipart_part_count(upload.byte_len, part_size)
        .ok_or_else(|| ErrorData::internal_error("invalid multipart upload geometry", None))?;
    let mut parts = parts
        .filter(|parts| !parts.is_empty())
        .ok_or_else(|| invalid_input_error("multipart completion requires completed_parts"))?;
    parts.sort_by_key(|part| part.part_number);
    if parts.len() != part_count as usize
        || parts.iter().enumerate().any(|(index, part)| {
            part.part_number != index as i32 + 1 || part.etag.trim().is_empty()
        })
    {
        return Err(invalid_input_error(
            "completed_parts must contain every part exactly once with a non-empty etag",
        ));
    }
    Ok(parts
        .into_iter()
        .map(|part| CompletedUploadPart {
            part_number: part.part_number,
            etag: part.etag,
        })
        .collect())
}

fn upload_id(input: &FileTransferInput) -> Result<Uuid, ErrorData> {
    let raw = input
        .upload_id
        .as_deref()
        .ok_or_else(|| invalid_input_error(format!("op={} requires upload_id", input.op)))?;
    Uuid::parse_str(raw).map_err(|_| invalid_input_error("upload_id must be a UUID"))
}

fn required(value: Option<String>, field: &str, op: &str) -> Result<String, ErrorData> {
    value.ok_or_else(|| invalid_input_error(format!("op={op} requires {field}")))
}

fn storage_error(error: ObjectStorageError) -> ErrorData {
    match error {
        ObjectStorageError::Missing => ErrorData::invalid_request(
            "uploaded object was not found",
            Some(json!({"kind": "conflict", "code": "object_missing"})),
        ),
        ObjectStorageError::SizeMismatch => ErrorData::invalid_request(
            "uploaded object size does not match the declared size",
            Some(json!({"kind": "invalid_input", "code": "size_mismatch"})),
        ),
        ObjectStorageError::InvalidMultipart => ErrorData::invalid_request(
            "multipart completion parts are invalid",
            Some(json!({"kind": "invalid_input", "code": "invalid_multipart"})),
        ),
        ObjectStorageError::Unavailable => ErrorData::new(
            rmcp::model::ErrorCode(-32001),
            "object storage is temporarily unavailable",
            Some(json!({
                "kind": "temporary_unavailable",
                "code": "object_storage_unavailable",
                "retryable": true,
            })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notegate_model::files::PendingObjectUpload;

    #[test]
    fn completed_parts_require_a_complete_ordered_set() -> Result<(), ErrorData> {
        let upload = PendingObjectUpload {
            id: Uuid::new_v4(),
            object_key: "objects/test".to_owned(),
            space_id: Uuid::new_v4(),
            parent_node_id: Uuid::new_v4(),
            requested_by_account_id: Uuid::new_v4(),
            name: "large.bin".to_owned(),
            byte_len: MULTIPART_PART_SIZE + 1,
            media_type: "application/octet-stream".to_owned(),
            original_filename: None,
            encryption_mode: FileEncryptionMode::None,
            encryption_metadata: None,
            upload_mode: ObjectUploadMode::Multipart,
            multipart_upload_id: Some("provider-id".to_owned()),
            multipart_part_size: Some(MULTIPART_PART_SIZE),
            node_id: None,
        };
        let valid = vec![
            CompletedPartInput {
                part_number: 2,
                etag: "two".to_owned(),
            },
            CompletedPartInput {
                part_number: 1,
                etag: "one".to_owned(),
            },
        ];
        assert_eq!(validate_completed_parts(&upload, Some(valid))?.len(), 2);
        assert!(
            validate_completed_parts(
                &upload,
                Some(vec![CompletedPartInput {
                    part_number: 1,
                    etag: "one".to_owned(),
                }])
            )
            .is_err()
        );
        Ok(())
    }
}
