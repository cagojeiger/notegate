//! Shared object-upload coordination for REST and MCP entry points.

use std::collections::HashSet;
use std::time::Duration;

use notegate_model::files::{
    BeginObjectUpload, ObjectUploadMode, ObjectUploadRegistration, PendingObjectUpload,
};
use notegate_service::ServiceError;
use uuid::Uuid;

use crate::object_storage::{
    CompletedUploadPart, MULTIPART_PART_SIZE, ObjectStorageError, PresignedPut,
    multipart_part_count, multipart_part_len, uses_multipart,
};
use crate::state::AppState;

pub const PART_URL_BATCH_MAX: usize = 16;
pub const PART_UPLOAD_CONCURRENCY_MAX: usize = 4;

pub struct BegunUpload {
    pub upload_id: Uuid,
    pub transfer: BegunTransfer,
}

pub enum BegunTransfer {
    Single(PresignedPut),
    Multipart { part_size: i64, part_count: i32 },
}

pub struct UploadPartTransfer {
    pub part_number: i32,
    pub content_length: i64,
    pub transfer: PresignedPut,
}

#[derive(Debug)]
pub enum UploadFlowError {
    InvalidInput(String),
    Service(ServiceError),
    Storage(ObjectStorageError),
    Internal(&'static str),
}

impl From<ServiceError> for UploadFlowError {
    fn from(error: ServiceError) -> Self {
        Self::Service(error)
    }
}

impl From<ObjectStorageError> for UploadFlowError {
    fn from(error: ObjectStorageError) -> Self {
        Self::Storage(error)
    }
}

pub async fn begin_upload(
    state: &AppState,
    account_id: Uuid,
    space_id: Uuid,
    command: &BeginObjectUpload,
    transfer_ttl: Duration,
) -> Result<BegunUpload, UploadFlowError> {
    state
        .files
        .prepare_object_upload(account_id, space_id, command)
        .await?;

    let upload_id = Uuid::new_v4();
    let object_key = format!("objects/{upload_id}");
    let transfer = if uses_multipart(command.byte_len) {
        let part_count = multipart_part_count(command.byte_len, MULTIPART_PART_SIZE)
            .ok_or_else(|| invalid("file is too large for multipart upload"))?;
        let storage_upload_id = state
            .object_storage
            .create_multipart_upload(&object_key, &command.media_type)
            .await?;
        let registration = ObjectUploadRegistration {
            id: upload_id,
            object_key: object_key.clone(),
            upload_mode: ObjectUploadMode::Multipart,
            multipart_upload_id: Some(storage_upload_id.clone()),
            multipart_part_size: Some(MULTIPART_PART_SIZE),
        };
        if let Err(error) = state
            .files
            .record_registered_object_upload(&registration, account_id, space_id, command)
            .await
        {
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
            return Err(error.into());
        }
        BegunTransfer::Multipart {
            part_size: MULTIPART_PART_SIZE,
            part_count,
        }
    } else {
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
                transfer_ttl,
            )
            .await?;
        state
            .files
            .record_registered_object_upload(&registration, account_id, space_id, command)
            .await?;
        BegunTransfer::Single(transfer)
    };

    tracing::info!(
        event = "object_storage.upload_created",
        %upload_id,
        %space_id,
        %account_id,
        byte_len = command.byte_len,
        upload_mode = if matches!(transfer, BegunTransfer::Multipart { .. }) {
            "multipart"
        } else {
            "single"
        },
    );
    Ok(BegunUpload {
        upload_id,
        transfer,
    })
}

pub async fn prepare_parts(
    state: &AppState,
    account_id: Uuid,
    upload: PendingObjectUpload,
    part_numbers: Vec<i32>,
    transfer_ttl: Duration,
) -> Result<Vec<UploadPartTransfer>, UploadFlowError> {
    if part_numbers.is_empty() {
        return Err(invalid("part_numbers must not be empty"));
    }
    if part_numbers.len() > PART_URL_BATCH_MAX {
        return Err(invalid(format!(
            "part_numbers accepts at most {PART_URL_BATCH_MAX} values"
        )));
    }
    let unique: HashSet<i32> = part_numbers.iter().copied().collect();
    if unique.len() != part_numbers.len() {
        return Err(invalid("part_numbers must not contain duplicates"));
    }
    if upload.upload_mode != ObjectUploadMode::Multipart {
        return Err(invalid("upload is not multipart"));
    }
    if upload.node_id.is_some() {
        return Err(invalid("upload is already complete"));
    }
    let part_size = upload.multipart_part_size.ok_or(UploadFlowError::Internal(
        "multipart upload state is incomplete",
    ))?;
    let prepared_parts = part_numbers
        .into_iter()
        .map(|part_number| {
            multipart_part_len(upload.byte_len, part_size, part_number)
                .map(|content_length| (part_number, content_length))
                .ok_or_else(|| invalid("part number is outside the upload range"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let upload = state
        .files
        .touch_object_upload(account_id, upload.space_id, upload.id)
        .await?;
    if upload.node_id.is_some() {
        return Err(invalid("upload is already complete"));
    }
    let storage_upload_id =
        upload
            .multipart_upload_id
            .as_deref()
            .ok_or(UploadFlowError::Internal(
                "multipart upload state is incomplete",
            ))?;

    let mut transfers = Vec::with_capacity(prepared_parts.len());
    for (part_number, content_length) in prepared_parts {
        let transfer = state
            .object_storage
            .presign_upload_part(
                &upload.object_key,
                storage_upload_id,
                part_number,
                content_length,
                transfer_ttl,
            )
            .await?;
        transfers.push(UploadPartTransfer {
            part_number,
            content_length,
            transfer,
        });
    }
    Ok(transfers)
}

pub async fn complete_upload(
    state: &AppState,
    account_id: Uuid,
    upload: PendingObjectUpload,
    completed_parts: Option<Vec<CompletedUploadPart>>,
) -> Result<notegate_model::files::FileView, UploadFlowError> {
    if upload.node_id.is_none() {
        if upload.upload_mode == ObjectUploadMode::Multipart {
            let completed = validate_completed_parts(&upload, completed_parts)?;
            let storage_upload_id =
                upload
                    .multipart_upload_id
                    .as_deref()
                    .ok_or(UploadFlowError::Internal(
                        "multipart upload state is incomplete",
                    ))?;
            // Refresh before the provider call so stale cleanup cannot claim
            // this upload while multipart completion is in progress.
            state
                .files
                .touch_object_upload(account_id, upload.space_id, upload.id)
                .await?;
            if let Err(completion_error) = state
                .object_storage
                .complete_multipart_upload(&upload.object_key, storage_upload_id, &completed)
                .await
            {
                // Another completion may already have consumed the provider
                // upload id. A matching final object makes this idempotent.
                match state
                    .object_storage
                    .verify_upload(&upload.object_key, upload.byte_len)
                    .await
                {
                    Ok(_) => {}
                    Err(ObjectStorageError::Missing) => {
                        return Err(completion_error.into());
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            verify_upload(state, &upload).await?;
        } else {
            if completed_parts.is_some() {
                return Err(invalid("single uploads do not accept completed_parts"));
            }
            // Verify first so missing single-PUT objects remain eligible for inactivity cleanup.
            verify_upload(state, &upload).await?;
            state
                .files
                .touch_object_upload(account_id, upload.space_id, upload.id)
                .await?;
        }
    }

    let detected_media_type = if upload.node_id.is_none() {
        match crate::file_preview::detect_object_media_type(
            &state.object_storage,
            &upload.object_key,
            upload.byte_len,
            upload.encryption_mode,
        )
        .await
        {
            Ok(media_type) => media_type,
            Err(error) => {
                tracing::warn!(
                    event = "object_storage.media_type_detection_failed",
                    upload_id = %upload.id,
                    space_id = %upload.space_id,
                    ?error,
                );
                None
            }
        }
    } else {
        None
    };

    let view = state
        .files
        .complete_object_upload(
            account_id,
            upload.space_id,
            upload.id,
            detected_media_type.as_deref(),
        )
        .await?;
    tracing::info!(
        event = "object_storage.file_attached",
        upload_id = %upload.id,
        node_id = %view.node.node.id,
        space_id = %upload.space_id,
    );
    Ok(view)
}

pub async fn abort_upload(
    state: &AppState,
    account_id: Uuid,
    upload: &PendingObjectUpload,
) -> Result<(), UploadFlowError> {
    state
        .files
        .cancel_object_upload(account_id, upload.space_id, upload.id)
        .await?;
    tracing::info!(
        event = "object_storage.upload_aborted",
        upload_id = %upload.id,
        space_id = %upload.space_id,
        %account_id,
    );
    Ok(())
}

fn validate_completed_parts(
    upload: &PendingObjectUpload,
    parts: Option<Vec<CompletedUploadPart>>,
) -> Result<Vec<CompletedUploadPart>, UploadFlowError> {
    let part_size = upload.multipart_part_size.ok_or(UploadFlowError::Internal(
        "multipart upload state is incomplete",
    ))?;
    let part_count = multipart_part_count(upload.byte_len, part_size).ok_or(
        UploadFlowError::Internal("invalid multipart upload geometry"),
    )?;
    let mut parts = parts
        .filter(|parts| !parts.is_empty())
        .ok_or_else(|| invalid("multipart completion requires completed_parts"))?;
    parts.sort_by_key(|part| part.part_number);
    if parts.len() != part_count as usize
        || parts.iter().enumerate().any(|(index, part)| {
            part.part_number != index as i32 + 1 || part.etag.trim().is_empty()
        })
    {
        return Err(invalid(
            "completed_parts must contain every part exactly once with a non-empty etag",
        ));
    }
    Ok(parts)
}

async fn verify_upload(
    state: &AppState,
    upload: &PendingObjectUpload,
) -> Result<(), UploadFlowError> {
    let etag = state
        .object_storage
        .verify_upload(&upload.object_key, upload.byte_len)
        .await?;
    tracing::info!(
        event = "object_storage.upload_verified",
        upload_id = %upload.id,
        space_id = %upload.space_id,
        %etag,
    );
    Ok(())
}

fn invalid(message: impl Into<String>) -> UploadFlowError {
    UploadFlowError::InvalidInput(message.into())
}

#[cfg(test)]
mod tests {
    use notegate_model::FileEncryptionMode;

    use super::*;

    #[test]
    fn completed_parts_require_every_part_exactly_once() -> Result<(), UploadFlowError> {
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
            CompletedUploadPart {
                part_number: 2,
                etag: "second".to_owned(),
            },
            CompletedUploadPart {
                part_number: 1,
                etag: "first".to_owned(),
            },
        ];

        let normalized = validate_completed_parts(&upload, Some(valid))?;
        assert_eq!(
            normalized
                .iter()
                .map(|part| part.part_number)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(matches!(
            validate_completed_parts(
                &upload,
                Some(vec![CompletedUploadPart {
                    part_number: 1,
                    etag: "first".to_owned(),
                }])
            ),
            Err(UploadFlowError::InvalidInput(_))
        ));
        Ok(())
    }
}
