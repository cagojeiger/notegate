use notegate_model::files::{ObjectUploadMode, ObjectUploadRegistration};
use notegate_model::{Channel, FileEncryptionMode};
use notegate_service::ServiceError;
use notegate_service::files::{BeginObjectUpload, DeleteNode};
use uuid::Uuid;

use crate::write_lock_support::{Fixture, TestResult, assert_write_locked};

#[tokio::test]
async fn upload_reservation_and_file_deletion_follow_lock_policy() -> TestResult {
    let Some(fixture) = Fixture::setup("files").await? else {
        return Ok(());
    };
    let folder_id = fixture.folder(fixture.root_id, "uploads").await?;
    let upload_id = Uuid::new_v4();
    let command = BeginObjectUpload {
        parent_node_id: folder_id,
        name: "report.bin".to_owned(),
        byte_len: 4,
        media_type: "application/octet-stream".to_owned(),
        original_filename: Some("report.bin".to_owned()),
        encryption_mode: FileEncryptionMode::None,
        encryption_metadata: None,
    };
    fixture
        .files
        .prepare_object_upload(fixture.owner, fixture.space_id, &command)
        .await?;
    fixture
        .files
        .record_registered_object_upload(
            &ObjectUploadRegistration {
                id: upload_id,
                object_key: format!("objects/{upload_id}"),
                upload_mode: ObjectUploadMode::Single,
                multipart_upload_id: None,
                multipart_part_size: None,
            },
            fixture.owner,
            fixture.space_id,
            &command,
        )
        .await?;
    let canceled_upload_id = Uuid::new_v4();
    fixture
        .files
        .record_registered_object_upload(
            &ObjectUploadRegistration {
                id: canceled_upload_id,
                object_key: format!("objects/{canceled_upload_id}"),
                upload_mode: ObjectUploadMode::Single,
                multipart_upload_id: None,
                multipart_part_size: None,
            },
            fixture.owner,
            fixture.space_id,
            &BeginObjectUpload {
                name: "cancel.bin".to_owned(),
                ..command.clone()
            },
        )
        .await?;

    fixture.set_lock(folder_id, true).await?;
    let blocked_upload_id = Uuid::new_v4();
    assert_write_locked(
        fixture
            .files
            .record_registered_object_upload(
                &ObjectUploadRegistration {
                    id: blocked_upload_id,
                    object_key: format!("objects/{blocked_upload_id}"),
                    upload_mode: ObjectUploadMode::Single,
                    multipart_upload_id: None,
                    multipart_part_size: None,
                },
                fixture.owner,
                fixture.space_id,
                &BeginObjectUpload {
                    name: "blocked.bin".to_owned(),
                    ..command.clone()
                },
            )
            .await,
    );
    fixture
        .files
        .cancel_object_upload(fixture.owner, fixture.space_id, canceled_upload_id)
        .await?;
    let canceled_state: String =
        sqlx::query_scalar("SELECT state FROM object_storage_objects WHERE id = $1")
            .bind(canceled_upload_id)
            .fetch_one(&fixture.db.pool)
            .await?;
    assert_eq!(canceled_state, "expire_pending");

    let file = fixture
        .files
        .complete_object_upload(fixture.owner, fixture.space_id, upload_id, None, None)
        .await?;
    let file_id = file.node.node.id;
    assert_eq!(
        file.node
            .write_lock_sources
            .first()
            .map(|source| source.node_id),
        Some(folder_id)
    );
    let upload_state: String =
        sqlx::query_scalar("SELECT state FROM object_storage_objects WHERE id = $1")
            .bind(upload_id)
            .fetch_one(&fixture.db.pool)
            .await?;
    assert_eq!(upload_state, "attached");

    let repeated = fixture
        .files
        .complete_object_upload(fixture.owner, fixture.space_id, upload_id, None, None)
        .await?;
    assert_eq!(repeated.node.node.id, file_id);
    assert_eq!(
        repeated
            .node
            .write_lock_sources
            .first()
            .map(|source| source.node_id),
        Some(folder_id)
    );
    assert_eq!(
        fixture
            .files
            .file_for_download(fixture.owner, fixture.space_id, file_id)
            .await?
            .node
            .node
            .id,
        file_id
    );
    assert_write_locked(
        fixture
            .files
            .delete_node(
                fixture.owner,
                fixture.space_id,
                DeleteNode {
                    node_id: file_id,
                    recursive: false,
                },
            )
            .await,
    );

    // Transfer reads need the same authorization and file data as the full
    // download view, but write locks must not prevent downloading.
    for channel in [Channel::Browser, Channel::Api, Channel::Mcp] {
        let files = fixture.files.for_channel(channel);
        let full = files
            .file_for_download(fixture.owner, fixture.space_id, file_id)
            .await?;
        let (node, source) = files
            .file_transfer_source(fixture.owner, fixture.space_id, file_id)
            .await?;
        assert_eq!(node, full.node.node);
        assert_eq!(source, full.file);
        for (owner, space, id) in [
            (Uuid::new_v4(), fixture.space_id, file_id),
            (fixture.owner, Uuid::new_v4(), file_id),
            (fixture.owner, fixture.space_id, Uuid::new_v4()),
            (fixture.owner, fixture.space_id, folder_id),
        ] {
            assert!(matches!(
                files.file_transfer_source(owner, space, id).await,
                Err(ServiceError::NotFound(_))
            ));
        }
    }

    // Both the file's own policy and inherited folder policy must be enforced.
    for hidden_id in [file_id, folder_id] {
        sqlx::query("UPDATE nodes SET external_access_enabled = false WHERE id = $1")
            .bind(hidden_id)
            .execute(&fixture.db.pool)
            .await?;
        fixture
            .files
            .file_transfer_source(fixture.owner, fixture.space_id, file_id)
            .await?;
        for channel in [Channel::Api, Channel::Mcp] {
            assert!(matches!(
                fixture
                    .files
                    .for_channel(channel)
                    .file_transfer_source(fixture.owner, fixture.space_id, file_id)
                    .await,
                Err(ServiceError::NotFound(_))
            ));
        }
        sqlx::query("UPDATE nodes SET external_access_enabled = true WHERE id = $1")
            .bind(hidden_id)
            .execute(&fixture.db.pool)
            .await?;
    }
    sqlx::query("UPDATE nodes SET deleted_at = now(), deleted_by_account_id = updated_by_account_id, purge_after = now() WHERE id = $1")
        .bind(file_id)
        .execute(&fixture.db.pool)
        .await?;
    assert!(matches!(
        fixture
            .files
            .file_transfer_source(fixture.owner, fixture.space_id, file_id)
            .await,
        Err(ServiceError::NotFound(_))
    ));

    fixture.cleanup().await;
    Ok(())
}
