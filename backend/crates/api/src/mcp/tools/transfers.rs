//! Path-first MCP control plane for direct object upload and download transfers.

use axum::http::request::Parts;
use notegate_model::FileEncryptionMode;
use notegate_model::files::BeginObjectUpload;
use rmcp::{ErrorData, Json};
use serde_json::{Value, json};
use uuid::Uuid;

use super::resolve::{
    caller, invalid_input_error, node_summary, resolve_target, service_error, split_parent_name,
};
use super::unified::FileTransferInput;
use crate::object_storage::{CompletedUploadPart, MCP_TRANSFER_URL_TTL, ObjectStorageError};
use crate::object_upload_flow::{
    BegunTransfer, PART_UPLOAD_CONCURRENCY_MAX, PART_URL_BATCH_MAX, UploadFlowError,
    abort_upload as abort_object_upload, begin_upload as begin_object_upload,
    complete_upload as complete_object_upload, prepare_parts as prepare_upload_parts,
};
use crate::state::AppState;

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
    let begun = begin_object_upload(
        state,
        caller.account_id(),
        resolved.space_id(),
        &command,
        MCP_TRANSFER_URL_TTL,
    )
    .await
    .map_err(flow_error)?;
    let upload_id = begun.upload_id;
    let transfer = match begun.transfer {
        BegunTransfer::Multipart {
            part_size,
            part_count,
        } => {
            let first_part_numbers =
                (1..=part_count.min(PART_URL_BATCH_MAX as i32)).collect::<Vec<_>>();
            return Ok(Json(json!({
                "upload_id": upload_id,
                "target": target,
                "transfer": {
                    "mode": "multipart",
                    "part_size": part_size,
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
        BegunTransfer::Single(transfer) => transfer,
    };
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
    let upload = state
        .files
        .object_upload_by_id(caller.account_id(), upload_id)
        .await
        .map_err(service_error)?;
    let transfers = prepare_upload_parts(
        state,
        caller.account_id(),
        upload,
        part_numbers,
        MCP_TRANSFER_URL_TTL,
    )
    .await
    .map_err(flow_error)?
    .into_iter()
    .map(|part| {
        json!({
            "part_number": part.part_number,
            "method": "PUT",
            "url": part.transfer.url,
            "headers": part.transfer.headers,
            "content_length": part.content_length,
            "expires_in_seconds": MCP_TRANSFER_URL_TTL.as_secs(),
        })
    })
    .collect::<Vec<_>>();
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
    let completed_parts = input.completed_parts.map(|parts| {
        parts
            .into_iter()
            .map(|part| CompletedUploadPart {
                part_number: part.part_number,
                etag: part.etag,
            })
            .collect()
    });
    let view = complete_object_upload(state, caller.account_id(), upload, completed_parts)
        .await
        .map_err(flow_error)?;
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
    abort_object_upload(state, caller.account_id(), &upload)
        .await
        .map_err(flow_error)?;
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

fn flow_error(error: UploadFlowError) -> ErrorData {
    match error {
        UploadFlowError::InvalidInput(message) => invalid_input_error(message),
        UploadFlowError::Service(error) => service_error(error),
        UploadFlowError::Storage(error) => storage_error(error),
        UploadFlowError::Internal(message) => ErrorData::internal_error(message, None),
    }
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
