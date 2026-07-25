mod common;

use common::{TestDb, space_with_root};
use notegate_db::FilesRepo;
use notegate_model::files::{StoredContent, WriteTextBody};

#[tokio::test]
async fn migration_rejects_policy_and_storage_drift_without_clearing_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space_id, root_id) = space_with_root(&db.pool, "text-encryption-migration").await?;
    let content = StoredContent {
        body: WriteTextBody::Plain("legacy plaintext".to_owned()),
        content_sha256: "0".repeat(64),
        byte_len: 16,
        line_count: 1,
    };
    let (node, _) = FilesRepo::new(db.pool.clone())
        .insert_text(space_id, root_id, "legacy.md", &content, owner)
        .await?;

    sqlx::query("ALTER TABLE text_objects DROP CONSTRAINT text_objects_encryption_state_check")
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "UPDATE text_objects SET encryption_enabled = true \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(node.id)
    .execute(&db.pool)
    .await?;

    let Err(error) = sqlx::raw_sql(include_str!("../migrations/0023_text_encryption_state.sql"))
        .execute(&db.pool)
        .await
    else {
        return Err("migration accepted inconsistent encryption state".into());
    };
    assert!(
        error
            .to_string()
            .contains("text encryption policy differs from stored state")
    );

    let encryption_enabled: bool = sqlx::query_scalar(
        "SELECT encryption_enabled FROM text_objects \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(node.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(encryption_enabled);

    db.cleanup().await;
    Ok(())
}
