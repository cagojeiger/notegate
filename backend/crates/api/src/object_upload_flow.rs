//! Shared object-upload coordination for REST and MCP entry points.

use std::collections::HashSet;
use std::time::Duration;

use notegate_model::files::{
    BeginObjectUpload, ObjectUploadMode, ObjectUploadRegistration, PendingObjectUpload,
};
use notegate_service::ServiceError;
use uuid::Uuid;

use crate::error::ApiError;
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

impl From<UploadFlowError> for ApiError {
    fn from(error: UploadFlowError) -> Self {
        match error {
            UploadFlowError::InvalidInput(message) => Self::invalid_field(message),
            UploadFlowError::Service(error) => error.into(),
            UploadFlowError::Storage(error) => error.into(),
            UploadFlowError::Internal(message) => {
                tracing::error!(event = "error.internal", detail = message);
                Self::internal("internal server error")
            }
        }
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

fn plan_upload_parts(
    upload: &PendingObjectUpload,
    part_numbers: &[i32],
) -> Result<Vec<(i32, i64)>, UploadFlowError> {
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
    part_numbers
        .iter()
        .copied()
        .map(|part_number| {
            multipart_part_len(upload.byte_len, part_size, part_number)
                .map(|content_length| (part_number, content_length))
                .ok_or_else(|| invalid("part number is outside the upload range"))
        })
        .collect()
}

pub async fn prepare_parts(
    state: &AppState,
    account_id: Uuid,
    upload: PendingObjectUpload,
    part_numbers: Vec<i32>,
    transfer_ttl: Duration,
) -> Result<Vec<UploadPartTransfer>, UploadFlowError> {
    let prepared_parts = plan_upload_parts(&upload, &part_numbers)?;

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

    fn multipart_upload(byte_len: i64) -> PendingObjectUpload {
        PendingObjectUpload {
            id: Uuid::new_v4(),
            object_key: "objects/test".to_owned(),
            space_id: Uuid::new_v4(),
            parent_node_id: Uuid::new_v4(),
            requested_by_account_id: Uuid::new_v4(),
            name: "large.bin".to_owned(),
            byte_len,
            media_type: "application/octet-stream".to_owned(),
            original_filename: None,
            encryption_mode: FileEncryptionMode::None,
            encryption_metadata: None,
            upload_mode: ObjectUploadMode::Multipart,
            multipart_upload_id: Some("provider-id".to_owned()),
            multipart_part_size: Some(MULTIPART_PART_SIZE),
            node_id: None,
        }
    }

    #[test]
    fn part_plan_rejects_invalid_batches_before_upload_state() {
        let upload = multipart_upload(MULTIPART_PART_SIZE + 1);
        assert!(matches!(
            plan_upload_parts(&upload, &[]),
            Err(UploadFlowError::InvalidInput(message))
                if message == "part_numbers must not be empty"
        ));

        let too_many_duplicates = vec![1; PART_URL_BATCH_MAX + 1];
        assert!(matches!(
            plan_upload_parts(&upload, &too_many_duplicates),
            Err(UploadFlowError::InvalidInput(message))
                if message == "part_numbers accepts at most 16 values"
        ));

        let mut single_upload = upload;
        single_upload.upload_mode = ObjectUploadMode::Single;
        assert!(matches!(
            plan_upload_parts(&single_upload, &[1, 1]),
            Err(UploadFlowError::InvalidInput(message))
                if message == "part_numbers must not contain duplicates"
        ));
    }

    #[test]
    fn part_plan_rejects_invalid_upload_state_in_order() {
        let mut upload = multipart_upload(MULTIPART_PART_SIZE + 1);
        upload.upload_mode = ObjectUploadMode::Single;
        assert!(matches!(
            plan_upload_parts(&upload, &[1]),
            Err(UploadFlowError::InvalidInput(message))
                if message == "upload is not multipart"
        ));

        upload.upload_mode = ObjectUploadMode::Multipart;
        upload.node_id = Some(Uuid::new_v4());
        upload.multipart_part_size = None;
        assert!(matches!(
            plan_upload_parts(&upload, &[1]),
            Err(UploadFlowError::InvalidInput(message))
                if message == "upload is already complete"
        ));

        upload.node_id = None;
        assert!(matches!(
            plan_upload_parts(&upload, &[1]),
            Err(UploadFlowError::Internal(
                "multipart upload state is incomplete"
            ))
        ));
    }

    #[test]
    fn part_plan_rejects_numbers_outside_the_upload_range() {
        let upload = multipart_upload(MULTIPART_PART_SIZE + 1);

        for part_number in [0, 3] {
            assert!(matches!(
                plan_upload_parts(&upload, &[part_number]),
                Err(UploadFlowError::InvalidInput(message))
                    if message == "part number is outside the upload range"
            ));
        }
    }

    #[test]
    fn part_plan_preserves_order_and_content_lengths() -> Result<(), UploadFlowError> {
        let upload = multipart_upload(MULTIPART_PART_SIZE + 7);

        assert_eq!(
            plan_upload_parts(&upload, &[2, 1])?,
            vec![(2, 7), (1, MULTIPART_PART_SIZE)]
        );
        Ok(())
    }

    #[test]
    fn part_plan_accepts_the_maximum_batch_size() -> Result<(), UploadFlowError> {
        let upload = multipart_upload(MULTIPART_PART_SIZE * PART_URL_BATCH_MAX as i64);
        let part_numbers = (1..=PART_URL_BATCH_MAX as i32).rev().collect::<Vec<_>>();

        let planned = plan_upload_parts(&upload, &part_numbers)?;

        assert_eq!(planned.len(), PART_URL_BATCH_MAX);
        assert_eq!(
            planned.first(),
            Some(&(PART_URL_BATCH_MAX as i32, MULTIPART_PART_SIZE))
        );
        assert_eq!(planned.last(), Some(&(1, MULTIPART_PART_SIZE)));
        Ok(())
    }

    #[test]
    fn completed_parts_require_every_part_exactly_once() -> Result<(), UploadFlowError> {
        let upload = multipart_upload(MULTIPART_PART_SIZE + 1);
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
