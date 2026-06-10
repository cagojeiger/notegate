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

/// Create an owner account + workspace; return (account_id, workspace_id, root_node_id).
async fn workspace_with_root(
    pool: &PgPool,
    sub: &str,
) -> Result<(Uuid, Uuid, Uuid), Box<dyn std::error::Error>> {
    let account = insert_user_account(pool, sub, &format!("{sub}@example.com")).await?;
    let workspace: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (created_by, name) \
         VALUES ($1, $2) RETURNING id",
    )
    .bind(account)
    .bind(format!("ws-{sub}"))
    .fetch_one(pool)
    .await?;
    // The AFTER INSERT trigger creates the canonical root node.
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM nodes WHERE workspace_id = $1 AND parent_id IS NULL")
            .bind(workspace)
            .fetch_one(pool)
            .await?;
    Ok((account, workspace, root))
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
        "INSERT INTO nodes (workspace_id, parent_id, name, kind, created_by, updated_by) \
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
async fn root_trigger_attributes_root_to_workspace_creator() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, root) = workspace_with_root(&db.pool, "rootattr").await?;
    let (created_by, updated_by): (Uuid, Uuid) =
        sqlx::query_as("SELECT created_by, updated_by FROM nodes WHERE id = $1")
            .bind(root)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(created_by, account, "root created_by must equal ws creator");
    assert_eq!(updated_by, account, "root updated_by must equal ws creator");
    let _ = ws;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn second_root_node_is_rejected() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, _root) = workspace_with_root(&db.pool, "tworoots").await?;
    // A second parent_id IS NULL node violates nodes_one_root_per_workspace.
    let res = sqlx::query(
        "INSERT INTO nodes (workspace_id, parent_id, name, kind, created_by, updated_by) \
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
    let (account, ws, root) = workspace_with_root(&db.pool, "dupsib").await?;
    insert_child(&db.pool, ws, root, account, "note.md", "document").await?;
    let dup = insert_child(&db.pool, ws, root, account, "note.md", "document").await;
    assert!(dup.is_err(), "duplicate live sibling name must be rejected");
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn document_name_must_end_md_and_folder_must_not() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, root) = workspace_with_root(&db.pool, "mdcheck").await?;
    let bad_doc = insert_child(&db.pool, ws, root, account, "note", "document").await;
    assert!(bad_doc.is_err(), "document without .md must be rejected");
    let bad_folder = insert_child(&db.pool, ws, root, account, "dir.md", "folder").await;
    assert!(bad_folder.is_err(), "folder ending in .md must be rejected");
    // The valid forms succeed.
    insert_child(&db.pool, ws, root, account, "note.md", "document").await?;
    insert_child(&db.pool, ws, root, account, "dir", "folder").await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn invalid_node_names_are_rejected_by_check() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, root) = workspace_with_root(&db.pool, "namecheck").await?;
    for bad in ["has space.md", "..", ".", "a/b.md"] {
        let res = insert_child(&db.pool, ws, root, account, bad, "document").await;
        assert!(res.is_err(), "node name {bad:?} must be rejected by CHECK");
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn document_byte_and_line_bounds_are_enforced() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, root) = workspace_with_root(&db.pool, "docbounds").await?;
    let node = insert_child(&db.pool, ws, root, account, "big.md", "document").await?;

    // byte_len upper bound = 524288.
    let over_bytes = sqlx::query(
        "INSERT INTO documents (node_id, workspace_id, byte_len, line_count, created_by, updated_by) \
         VALUES ($1, $2, 524289, 0, $3, $3)",
    )
    .bind(node)
    .bind(ws)
    .bind(account)
    .execute(&db.pool)
    .await;
    assert!(over_bytes.is_err(), "byte_len 524289 must violate CHECK");

    // line_count upper bound = 2000; the boundary value 524288/2000 is accepted.
    let at_boundary = sqlx::query(
        "INSERT INTO documents (node_id, workspace_id, byte_len, line_count, created_by, updated_by) \
         VALUES ($1, $2, 524288, 2000, $3, $3)",
    )
    .bind(node)
    .bind(ws)
    .bind(account)
    .execute(&db.pool)
    .await;
    assert!(
        at_boundary.is_ok(),
        "byte_len 524288 / line_count 2000 must be accepted"
    );

    let over_lines = sqlx::query("UPDATE documents SET line_count = 2001 WHERE node_id = $1")
        .bind(node)
        .execute(&db.pool)
        .await;
    assert!(over_lines.is_err(), "line_count 2001 must violate CHECK");
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn workspace_name_check_and_owner_unique() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let account = insert_user_account(&db.pool, "wsname", "wsname@example.com").await?;
    // Invalid workspace name (space) violates the CHECK.
    let bad = sqlx::query("INSERT INTO workspaces (created_by, name) VALUES ($1, 'bad name')")
        .bind(account)
        .execute(&db.pool)
        .await;
    assert!(bad.is_err(), "workspace name with space must be rejected");

    // Duplicate (owner, name) violates the UNIQUE constraint.
    sqlx::query("INSERT INTO workspaces (created_by, name) VALUES ($1, 'personal')")
        .bind(account)
        .execute(&db.pool)
        .await?;
    let dup = sqlx::query("INSERT INTO workspaces (created_by, name) VALUES ($1, 'personal')")
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
    sqlx::query("INSERT INTO agents (id, name, created_by) VALUES ($1, 'agent', $2)")
        .bind(agent_account)
        .bind(creator)
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "INSERT INTO api_keys (account_id, token_prefix, token_hash, hash_key_id, name, expires_at) \
         VALUES ($1, 'dup-hash-1', 'dup-hash', 'test-lookup', 'k1', now() + interval '1 day')",
    )
    .bind(agent_account)
    .execute(&db.pool)
    .await?;
    let dup = sqlx::query(
        "INSERT INTO api_keys (account_id, token_prefix, token_hash, hash_key_id, name, expires_at) \
         VALUES ($1, 'dup-hash-2', 'dup-hash', 'test-lookup', 'k2', now() + interval '1 day')",
    )
    .bind(agent_account)
    .execute(&db.pool)
    .await;
    assert!(dup.is_err(), "duplicate token_hash must be rejected");
    db.cleanup().await;
    Ok(())
}
