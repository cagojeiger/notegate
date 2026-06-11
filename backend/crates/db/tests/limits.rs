//! DB-level invariant tests: the schema's final defense line (CHECK
//! constraints + unique indexes). Service-layer guards are unit-tested
//! separately; these assert the database rejects violations even if a bug
//! bypassed the service. Skipped when `NOTEGATE_TEST_DATABASE_URL` is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{TestDb, insert_user_account};
use sqlx::PgPool;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Create an owner account + space; return (account_id, space_id, root_node_id).
async fn space_with_root(
    pool: &PgPool,
    sub: &str,
) -> Result<(Uuid, Uuid, Uuid), Box<dyn std::error::Error>> {
    let account = insert_user_account(pool, sub, &format!("{sub}@example.com")).await?;
    let space: Uuid = sqlx::query_scalar(
        "INSERT INTO spaces (owner_user_id, name) \
         VALUES ($1, $2) RETURNING id",
    )
    .bind(account)
    .bind(format!("ws-{sub}"))
    .fetch_one(pool)
    .await?;
    // The AFTER INSERT trigger creates the canonical root node.
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM nodes WHERE space_id = $1 AND parent_id IS NULL")
            .bind(space)
            .fetch_one(pool)
            .await?;
    Ok((account, space, root))
}

async fn insert_child(
    pool: &PgPool,
    ws: Uuid,
    parent: Uuid,
    account: Uuid,
    name: &str,
    kind: &str,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO nodes (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id \
         VALUES ($1, $2, $3, $4, $5, $5) RETURNING id",
    )
    .bind(ws)
    .bind(parent)
    .bind(name)
    .bind(kind)
    .bind(account)
    .fetch_one(pool)
    .await
}

#[tokio::test]
async fn root_trigger_attributes_root_to_space_creator() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, root) = space_with_root(&db.pool, "rootattr").await?;
    let (created_by_account_id, updated_by_account_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT created_by_account_id, updated_by_account_id FROM nodes WHERE id = $1",
    )
    .bind(root)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        created_by_account_id, account,
        "root created_by must equal space creator"
    );
    assert_eq!(
        updated_by_account_id, account,
        "root updated_by must equal space creator"
    );
    let _ = ws;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn second_root_node_is_rejected() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, _root) = space_with_root(&db.pool, "tworoots").await?;
    // A second parent_id IS NULL node violates nodes_one_root_per_space.
    let res = sqlx::query(
        "INSERT INTO nodes (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id \
         VALUES ($1, NULL, '/', 'folder', $2, $2)",
    )
    .bind(ws)
    .bind(account)
    .execute(&db.pool)
    .await;
    assert!(res.is_err(), "second root must be rejected");
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn duplicate_live_sibling_name_is_rejected() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, root) = space_with_root(&db.pool, "dupsib").await?;
    insert_child(&db.pool, ws, root, account, "note.md", "text").await?;
    let dup = insert_child(&db.pool, ws, root, account, "note.md", "text").await;
    assert!(dup.is_err(), "duplicate live sibling name must be rejected");
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn text_name_must_end_md_and_folder_must_not() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, root) = space_with_root(&db.pool, "mdcheck").await?;
    let bad_doc = insert_child(&db.pool, ws, root, account, "note", "text").await;
    assert!(bad_doc.is_err(), "text without .md must be rejected");
    let bad_folder = insert_child(&db.pool, ws, root, account, "dir.md", "folder").await;
    assert!(bad_folder.is_err(), "folder ending in .md must be rejected");
    // The valid forms succeed.
    insert_child(&db.pool, ws, root, account, "note.md", "text").await?;
    insert_child(&db.pool, ws, root, account, "dir", "folder").await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn invalid_node_names_are_rejected_by_check() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, root) = space_with_root(&db.pool, "namecheck").await?;
    for bad in ["has space.md", "..", ".", "a/b.md"] {
        let res = insert_child(&db.pool, ws, root, account, bad, "text").await;
        assert!(res.is_err(), "node name {bad:?} must be rejected by CHECK");
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn text_byte_and_line_bounds_are_enforced() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, root) = space_with_root(&db.pool, "docbounds").await?;
    let node = insert_child(&db.pool, ws, root, account, "big.md", "text").await?;

    // byte_len upper bound = 1048576.
    let over_bytes = sqlx::query(
        "INSERT INTO text_objects \
         (node_id, space_id, content_sha256, byte_len, line_count, media_type, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, 1048577, 0, 'text/plain', $4, $4)",
    )
    .bind(node)
    .bind(ws)
    .bind("0".repeat(64))
    .bind(account)
    .execute(&db.pool)
    .await;
    assert!(over_bytes.is_err(), "byte_len 1048577 must violate CHECK");

    // line_count upper bound = 2000; the boundary value 1048576/2000 is accepted.
    let at_boundary = sqlx::query(
        "INSERT INTO text_objects \
         (node_id, space_id, content_sha256, byte_len, line_count, media_type, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, 1048576, 2000, 'text/plain', $4, $4)",
    )
    .bind(node)
    .bind(ws)
    .bind("1".repeat(64))
    .bind(account)
    .execute(&db.pool)
    .await;
    assert!(
        at_boundary.is_ok(),
        "byte_len 1048576 / line_count 2000 must be accepted"
    );

    let over_lines = sqlx::query("UPDATE text_objects SET line_count = 2001 WHERE node_id = $1")
        .bind(node)
        .execute(&db.pool)
        .await;
    assert!(over_lines.is_err(), "line_count 2001 must violate CHECK");
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn space_name_check_and_owner_unique() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let account = insert_user_account(&db.pool, "wsname", "wsname@example.com").await?;
    // Invalid space name (space) violates the CHECK.
    let bad = sqlx::query("INSERT INTO spaces (owner_user_id, name) VALUES ($1, 'bad name')")
        .bind(account)
        .execute(&db.pool)
        .await;
    assert!(bad.is_err(), "space name with space must be rejected");

    // Duplicate (owner, name) violates the UNIQUE constraint.
    sqlx::query("INSERT INTO spaces (owner_user_id, name) VALUES ($1, 'personal')")
        .bind(account)
        .execute(&db.pool)
        .await?;
    let dup = sqlx::query("INSERT INTO spaces (owner_user_id, name) VALUES ($1, 'personal')")
        .bind(account)
        .execute(&db.pool)
        .await;
    assert!(dup.is_err(), "duplicate (owner, name) must be rejected");
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn agent_key_token_hash_is_unique() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let creator = insert_user_account(&db.pool, "keyowner", "keyowner@example.com").await?;
    let agent_account: Uuid =
        sqlx::query_scalar("INSERT INTO accounts (kind) VALUES ('agent') RETURNING id")
            .fetch_one(&db.pool)
            .await?;
    sqlx::query("INSERT INTO agents (id, name, owner_user_id) VALUES ($1, 'agent', $2)")
        .bind(agent_account)
        .bind(creator)
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "INSERT INTO api_keys (account_id, token_prefix, token_hash, hash_key_id, name, created_by_user_id, expires_at) \
         VALUES ($1, 'dup-hash-1', 'dup-hash', 'test-lookup', 'k1', $2, now() + interval '1 day')",
    )
    .bind(agent_account)
    .bind(creator)
    .execute(&db.pool)
    .await?;
    let dup = sqlx::query(
        "INSERT INTO api_keys (account_id, token_prefix, token_hash, hash_key_id, name, created_by_user_id, expires_at) \
         VALUES ($1, 'dup-hash-2', 'dup-hash', 'test-lookup', 'k2', $2, now() + interval '1 day')",
    )
    .bind(agent_account)
    .bind(creator)
    .execute(&db.pool)
    .await;
    assert!(dup.is_err(), "duplicate token_hash must be rejected");
    db.cleanup().await;
    Ok(())
}
