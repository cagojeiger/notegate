mod common;

use common::{TestDb, space_with_root};
use notegate_db::FilesRepo;
use notegate_model::files::{StoredContent, WriteTextBody};

#[tokio::test]
async fn migration_removes_legacy_policy_and_preserves_stored_state()
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

    sqlx::query(
        "ALTER TABLE text_objects ADD COLUMN encryption_enabled BOOLEAN NOT NULL DEFAULT false",
    )
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

    sqlx::raw_sql(include_str!("../migrations/0023_text_encryption_state.sql"))
        .execute(&db.pool)
        .await?;

    let encryption_column_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (\
            SELECT 1 FROM information_schema.columns \
            WHERE table_schema = current_schema() \
              AND table_name = 'text_objects' \
              AND column_name = 'encryption_enabled'\
        )",
    )
    .fetch_one(&db.pool)
    .await?;
    assert!(!encryption_column_exists);

    let stored_state: (String, Option<String>) = sqlx::query_as(
        "SELECT at_rest_encryption, content_text FROM text_objects \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(node.id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(stored_state.0, "none");
    assert_eq!(stored_state.1.as_deref(), Some("legacy plaintext"));

    db.cleanup().await;
    Ok(())
}
