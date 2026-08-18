#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_in_result
)]

mod common;

use chrono::{Duration, Utc};
use common::{TestDb, agent_api_key_prefix, attach_file, insert_user_account, space_with_root};
use notegate_core::security::PiiCrypto;
use notegate_db::api_key_repo::InsertApiKey;
use notegate_db::browser_session_repo::{InsertBrowserSession, token_prefix};
use notegate_db::{
    ApiKeyRepo, BrowserSessionRepo, FilesRepo, MediaTypeObservation, MetadataWriteRepo,
};
use notegate_model::CreateApiKey;
use uuid::Uuid;

#[tokio::test]
async fn bulk_metadata_flush_is_monotonic_and_idempotent() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user_id = insert_user_account(
        &db.pool,
        "metadata-write-owner",
        "metadata-write-owner@example.test",
    )
    .await?;
    let agent_id: Uuid =
        sqlx::query_scalar("INSERT INTO accounts (kind) VALUES ('agent') RETURNING id")
            .fetch_one(&db.pool)
            .await?;
    sqlx::query("INSERT INTO agents (id, name, owner_user_id) VALUES ($1, $2, $3)")
        .bind(agent_id)
        .bind("metadata-write-agent")
        .bind(user_id)
        .execute(&db.pool)
        .await?;

    let key_id = Uuid::new_v4();
    ApiKeyRepo::new(db.pool.clone())
        .insert_key_with_cap(
            InsertApiKey {
                key_id,
                account_id: agent_id,
                command: &CreateApiKey {
                    name: "metadata-write-key".to_owned(),
                    scopes: Vec::new(),
                    expires_at: Some(Utc::now() + Duration::days(1)),
                },
                token_prefix: &agent_api_key_prefix(key_id),
                token_hash: "metadata-write-key-hash",
                created_by: user_id,
                rotated_from_key_id: None,
            },
            10,
        )
        .await?;

    let session_id = Uuid::new_v4();
    let crypto = PiiCrypto::test();
    let session_secret = "metadata-write-session-secret";
    let token_hash = crypto.browser_session_hash(&session_id.to_string(), session_secret)?;
    let encrypted_refresh =
        crypto.encrypt_browser_refresh_token(&session_id.to_string(), "refresh-token")?;
    BrowserSessionRepo::new(db.pool.clone())
        .insert_session(InsertBrowserSession {
            session_id,
            user_id,
            token_prefix: &token_prefix(session_id),
            token_hash: &token_hash,
            refresh_token: &encrypted_refresh,
            refresh_token_enc_key_id: crypto.enc_key_id(),
            refresh_token_enc_version: crypto.version(),
            validated_until: Utc::now() + Duration::hours(1),
            expires_at: Utc::now() + Duration::days(1),
        })
        .await?;

    let (space_owner_id, space_id, root_id) =
        space_with_root(&db.pool, "metadata-write-space").await?;
    let (node, _) = attach_file(
        &FilesRepo::new(db.pool.clone()),
        space_id,
        root_id,
        "asset.bin",
        4,
        space_owner_id,
    )
    .await?;

    let database_now: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&db.pool)
        .await?;
    let later = database_now - Duration::hours(2);
    let earlier = later - Duration::minutes(1);
    let repo = MetadataWriteRepo::new(db.pool.clone());

    assert_eq!(repo.update_api_key_last_used(&[key_id, key_id]).await?, 1);
    assert_eq!(
        repo.update_browser_session_last_used(&[session_id, session_id])
            .await?,
        1
    );
    assert_eq!(
        repo.set_detected_media_types(&[
            MediaTypeObservation {
                space_id,
                node_id: node.id,
                media_type: "image/png".to_owned(),
                observed_at: later,
            },
            MediaTypeObservation {
                space_id,
                node_id: node.id,
                media_type: "image/jpeg".to_owned(),
                observed_at: earlier,
            },
        ])
        .await?,
        1
    );

    let api_key_last_used: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT last_used_at FROM api_keys WHERE id = $1")
            .bind(key_id)
            .fetch_one(&db.pool)
            .await?;
    let session_last_used: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT last_used_at FROM browser_sessions WHERE id = $1")
            .bind(session_id)
            .fetch_one(&db.pool)
            .await?;
    let media_type: Option<String> =
        sqlx::query_scalar("SELECT detected_media_type FROM file_objects WHERE node_id = $1")
            .bind(node.id)
            .fetch_one(&db.pool)
            .await?;
    assert!(api_key_last_used >= database_now);
    assert!(session_last_used >= database_now);
    assert_eq!(media_type.as_deref(), Some("image/jpeg"));

    assert_eq!(repo.update_api_key_last_used(&[key_id]).await?, 0);
    assert_eq!(
        repo.update_browser_session_last_used(&[session_id]).await?,
        0
    );

    let browser_updated_at: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT updated_at FROM browser_sessions WHERE id = $1")
            .bind(session_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(browser_updated_at, session_last_used);

    sqlx::query("UPDATE api_keys SET last_used_at = now() - interval '61 minutes' WHERE id = $1")
        .bind(key_id)
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "UPDATE browser_sessions \
         SET last_used_at = now() - interval '61 minutes', \
             updated_at = now() - interval '61 minutes' \
         WHERE id = $1",
    )
    .bind(session_id)
    .execute(&db.pool)
    .await?;
    assert_eq!(repo.update_api_key_last_used(&[key_id]).await?, 1);
    assert_eq!(
        repo.update_browser_session_last_used(&[session_id]).await?,
        1
    );
    assert_eq!(
        repo.set_detected_media_types(&[MediaTypeObservation {
            space_id,
            node_id: node.id,
            media_type: "application/pdf".to_owned(),
            observed_at: earlier - Duration::minutes(1),
        }])
        .await?,
        0
    );

    sqlx::query(
        "UPDATE file_objects SET detected_media_type = 'application/zip' WHERE node_id = $1",
    )
    .bind(node.id)
    .execute(&db.pool)
    .await?;
    let docx_media_type = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    assert_eq!(
        repo.set_detected_media_types(&[MediaTypeObservation {
            space_id,
            node_id: node.id,
            media_type: docx_media_type.to_owned(),
            observed_at: later + Duration::minutes(1),
        }])
        .await?,
        1
    );
    assert_eq!(
        repo.set_detected_media_types(&[MediaTypeObservation {
            space_id,
            node_id: node.id,
            media_type: "image/png".to_owned(),
            observed_at: later + Duration::minutes(2),
        }])
        .await?,
        0
    );
    let media_type: Option<String> =
        sqlx::query_scalar("SELECT detected_media_type FROM file_objects WHERE node_id = $1")
            .bind(node.id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(media_type.as_deref(), Some(docx_media_type));

    db.cleanup().await;
    Ok(())
}
