//! Path-first MCP control plane for direct object upload and download transfers.

use axum::http::request::Parts;
use notegate_model::FileEncryptionMode;
use notegate_model::files::{BeginObjectUpload, PendingObjectUpload};
use rmcp::{ErrorData, Json};
use serde_json::{Value, json};
use uuid::Uuid;

use super::resolve::{
    caller, invalid_input_error, node_summary, resolve_target, service_error, split_parent_name,
};
use super::unified::FileTransferInput;
use crate::object_storage::{AGENT_TRANSFER_URL_TTL, CompletedUploadPart, ObjectStorageError};
use crate::object_upload_flow::{
    BegunTransfer, BegunUpload, PART_UPLOAD_CONCURRENCY_MAX, PART_URL_BATCH_MAX, UploadFlowError,
    UploadPartTransfer, abort_upload as abort_object_upload, begin_upload as begin_object_upload,
    complete_upload as complete_object_upload, prepare_parts as prepare_upload_parts,
};
use crate::state::AppState;

pub async fn call(
    state: &AppState,
    parts: &Parts,
    input: FileTransferInput,
) -> Result<Json<Value>, ErrorData> {
    let purpose = input.purpose.clone();
    let mut response = match input.op.as_str() {
        "begin_upload" => begin_upload(state, parts, input).await,
        "prepare_parts" => prepare_parts(state, parts, input).await,
        "complete_upload" => complete_upload(state, parts, input).await,
        "abort_upload" => abort_upload(state, parts, input).await,
        "prepare_download" => prepare_download(state, parts, input).await,
        _ => Err(invalid_input_error(
            "invalid op for file_transfer; allowed values are: begin_upload, prepare_parts, complete_upload, abort_upload, prepare_download",
        )),
    }?;
    add_purpose_to_next_action_inputs(&mut response.0, &purpose);
    Ok(response)
}

fn add_purpose_to_next_action_inputs(response: &mut Value, purpose: &str) {
    let Some(next_action) = response
        .get_mut("next_action")
        .and_then(Value::as_object_mut)
    else {
        return;
    };

    add_purpose_to_call_tool(next_action, purpose);
    for key in ["repeat", "then"] {
        if let Some(action) = next_action.get_mut(key).and_then(Value::as_object_mut) {
            add_purpose_to_call_tool(action, purpose);
        }
    }
}

fn add_purpose_to_call_tool(action: &mut serde_json::Map<String, Value>, purpose: &str) {
    if action.contains_key("tool")
        && let Some(Value::Object(input)) = action.get_mut("input")
    {
        input.insert("purpose".to_owned(), Value::String(purpose.to_owned()));
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
        AGENT_TRANSFER_URL_TTL,
    )
    .await
    .map_err(flow_error)?;
    Ok(build_begin_upload_response(target, byte_len, begun))
}

fn build_begin_upload_response(target: String, byte_len: i64, begun: BegunUpload) -> Json<Value> {
    let BegunUpload {
        upload_id,
        transfer,
    } = begun;
    match transfer {
        BegunTransfer::Multipart {
            part_size,
            part_count,
        } => {
            let first_part_numbers =
                (1..=part_count.min(PART_URL_BATCH_MAX as i32)).collect::<Vec<_>>();
            Json(json!({
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
            }))
        }
        BegunTransfer::Single(transfer) => Json(json!({
            "upload_id": upload_id,
            "target": target,
            "transfer": {
                "mode": "single",
                "method": "PUT",
                "url": transfer.url,
                "headers": transfer.headers,
                "content_length": byte_len,
                "expires_in_seconds": AGENT_TRANSFER_URL_TTL.as_secs(),
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
        })),
    }
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
    require_upload_space_visible(state, caller.account_id(), &upload).await?;
    let transfers = prepare_upload_parts(
        state,
        caller.account_id(),
        upload,
        part_numbers,
        AGENT_TRANSFER_URL_TTL,
    )
    .await
    .map_err(flow_error)?;
    Ok(build_prepare_parts_response(upload_id, transfers))
}

fn build_prepare_parts_response(
    upload_id: Uuid,
    transfers: Vec<UploadPartTransfer>,
) -> Json<Value> {
    let parts = transfers
        .into_iter()
        .map(|part| {
            json!({
                "part_number": part.part_number,
                "method": "PUT",
                "url": part.transfer.url,
                "headers": part.transfer.headers,
                "content_length": part.content_length,
                "expires_in_seconds": AGENT_TRANSFER_URL_TTL.as_secs(),
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "upload_id": upload_id,
        "parts": parts,
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
    }))
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
    require_upload_space_visible(state, caller.account_id(), &upload).await?;
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
    require_upload_space_visible(state, caller.account_id(), &upload).await?;
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

async fn require_upload_space_visible(
    state: &AppState,
    caller_account_id: Uuid,
    upload: &PendingObjectUpload,
) -> Result<(), ErrorData> {
    match state
        .spaces
        .find_mcp_visible_by_id(caller_account_id, upload.space_id)
        .await
        .map_err(service_error)?
    {
        Some(_) => Ok(()),
        None => Err(service_error(notegate_service::ServiceError::NotFound(
            "space not found".to_owned(),
        ))),
    }
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
            AGENT_TRANSFER_URL_TTL,
        )
        .await
        .map_err(storage_error)?;
    Ok(Json(json!({
        "target": target,
        "transfer": {
            "method": "GET",
            "url": url,
            "headers": {},
            "expires_in_seconds": AGENT_TRANSFER_URL_TTL.as_secs(),
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

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]

    use std::collections::BTreeMap;

    use crate::object_storage::PresignedPut;

    use super::*;

    #[test]
    fn purpose_is_added_to_every_follow_up_tool_input() {
        let mut response = json!({
            "next_action": {
                "repeat": {
                    "tool": "file_transfer",
                    "input": {"op": "prepare_parts"}
                },
                "then": {
                    "tool": "file_transfer",
                    "input": {"op": "complete_upload"}
                }
            }
        });

        add_purpose_to_next_action_inputs(&mut response, "finish uploading the report");

        assert_eq!(
            response["next_action"]["repeat"]["input"]["purpose"],
            "finish uploading the report"
        );
        assert_eq!(
            response["next_action"]["then"]["input"]["purpose"],
            "finish uploading the report"
        );
    }

    #[test]
    fn purpose_propagation_does_not_mutate_unrelated_response_objects() {
        let mut response = json!({
            "metadata": {
                "tool": "file_transfer",
                "input": {"op": "prepare_parts"}
            },
            "next_action": {
                "tool": "file_transfer",
                "input": {"op": "complete_upload"}
            }
        });

        add_purpose_to_next_action_inputs(&mut response, "finish uploading the report");

        assert_eq!(response["metadata"]["input"].get("purpose"), None);
        assert_eq!(
            response["next_action"]["input"]["purpose"],
            "finish uploading the report"
        );
    }

    fn presigned_put(url: &str, content_type: &str) -> PresignedPut {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_owned(), content_type.to_owned());
        PresignedPut {
            url: url.to_owned(),
            headers,
        }
    }

    #[test]
    fn begin_response_guides_single_upload_and_completion() {
        let upload_id = Uuid::from_u128(1);
        let response = build_begin_upload_response(
            "daily:/report.pdf".to_owned(),
            42,
            BegunUpload {
                upload_id,
                transfer: BegunTransfer::Single(presigned_put(
                    "https://storage.example/upload",
                    "application/pdf",
                )),
            },
        )
        .0;

        assert_eq!(
            response.pointer("/upload_id").and_then(Value::as_str),
            Some(upload_id.to_string().as_str())
        );
        assert_eq!(
            response.pointer("/target").and_then(Value::as_str),
            Some("daily:/report.pdf")
        );
        assert_eq!(
            response.pointer("/transfer/mode").and_then(Value::as_str),
            Some("single")
        );
        assert_eq!(
            response.pointer("/transfer/method").and_then(Value::as_str),
            Some("PUT")
        );
        assert_eq!(
            response.pointer("/transfer/url").and_then(Value::as_str),
            Some("https://storage.example/upload")
        );
        assert_eq!(
            response
                .pointer("/transfer/headers/content-type")
                .and_then(Value::as_str),
            Some("application/pdf")
        );
        assert_eq!(
            response
                .pointer("/transfer/content_length")
                .and_then(Value::as_i64),
            Some(42)
        );
        assert_eq!(
            response
                .pointer("/transfer/expires_in_seconds")
                .and_then(Value::as_u64),
            Some(300)
        );
        assert_eq!(
            response
                .pointer("/next_action/kind")
                .and_then(Value::as_str),
            Some("http_upload")
        );
        assert_eq!(
            response
                .pointer("/next_action/transfer_field")
                .and_then(Value::as_str),
            Some("transfer")
        );
        assert_eq!(
            response
                .pointer("/next_action/then/input/op")
                .and_then(Value::as_str),
            Some("complete_upload")
        );
        assert_eq!(
            response
                .pointer("/next_action/then/input/upload_id")
                .and_then(Value::as_str),
            Some(upload_id.to_string().as_str())
        );
    }

    #[test]
    fn begin_response_caps_multipart_first_batch() {
        let upload_id = Uuid::from_u128(2);
        let response = build_begin_upload_response(
            "daily:/archive.bin".to_owned(),
            1_000,
            BegunUpload {
                upload_id,
                transfer: BegunTransfer::Multipart {
                    part_size: 200,
                    part_count: 20,
                },
            },
        )
        .0;
        let expected_part_numbers = json!([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);

        assert_eq!(
            response.pointer("/transfer/mode").and_then(Value::as_str),
            Some("multipart")
        );
        assert_eq!(
            response
                .pointer("/transfer/part_size")
                .and_then(Value::as_i64),
            Some(200)
        );
        assert_eq!(
            response
                .pointer("/transfer/part_count")
                .and_then(Value::as_i64),
            Some(20)
        );
        assert_eq!(
            response
                .pointer("/next_action/kind")
                .and_then(Value::as_str),
            Some("call_tool")
        );
        assert_eq!(
            response
                .pointer("/next_action/tool")
                .and_then(Value::as_str),
            Some("file_transfer")
        );
        assert_eq!(
            response
                .pointer("/next_action/input/op")
                .and_then(Value::as_str),
            Some("prepare_parts")
        );
        assert_eq!(
            response.pointer("/next_action/input/part_numbers"),
            Some(&expected_part_numbers)
        );
        assert_eq!(
            response
                .pointer("/next_action/input/upload_id")
                .and_then(Value::as_str),
            Some(upload_id.to_string().as_str())
        );
    }

    #[test]
    fn prepare_response_preserves_part_order_and_continuations() {
        let upload_id = Uuid::from_u128(3);
        let response = build_prepare_parts_response(
            upload_id,
            vec![
                UploadPartTransfer {
                    part_number: 2,
                    content_length: 20,
                    transfer: presigned_put("https://storage.example/part-2", "application/bin"),
                },
                UploadPartTransfer {
                    part_number: 1,
                    content_length: 10,
                    transfer: presigned_put("https://storage.example/part-1", "application/bin"),
                },
            ],
        )
        .0;

        assert_eq!(
            response
                .pointer("/parts/0/part_number")
                .and_then(Value::as_i64),
            Some(2)
        );
        assert_eq!(
            response.pointer("/parts/0/url").and_then(Value::as_str),
            Some("https://storage.example/part-2")
        );
        assert_eq!(
            response.pointer("/parts/0/method").and_then(Value::as_str),
            Some("PUT")
        );
        assert_eq!(
            response
                .pointer("/parts/0/headers/content-type")
                .and_then(Value::as_str),
            Some("application/bin")
        );
        assert_eq!(
            response
                .pointer("/parts/0/content_length")
                .and_then(Value::as_i64),
            Some(20)
        );
        assert_eq!(
            response
                .pointer("/parts/0/expires_in_seconds")
                .and_then(Value::as_u64),
            Some(300)
        );
        assert_eq!(
            response
                .pointer("/parts/1/part_number")
                .and_then(Value::as_i64),
            Some(1)
        );
        assert_eq!(
            response
                .pointer("/next_action/transfers_field")
                .and_then(Value::as_str),
            Some("parts")
        );
        assert_eq!(
            response
                .pointer("/next_action/collect_response_header")
                .and_then(Value::as_str),
            Some("etag")
        );
        assert_eq!(
            response
                .pointer("/next_action/max_concurrency")
                .and_then(Value::as_u64),
            Some(4)
        );
        assert_eq!(
            response
                .pointer("/next_action/repeat/input/upload_id")
                .and_then(Value::as_str),
            Some(upload_id.to_string().as_str())
        );
        assert_eq!(
            response
                .pointer("/next_action/then/input/upload_id")
                .and_then(Value::as_str),
            Some(upload_id.to_string().as_str())
        );
    }
}
