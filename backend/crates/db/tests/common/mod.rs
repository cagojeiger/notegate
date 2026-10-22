//! Shared helpers for DB integration tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
use notegate_db::FilesRepo;
#[allow(unused_imports)]
pub use notegate_db::test_support::{TestDb, deactivate_account, insert_user_account};
use notegate_model::files::BeginObjectUpload;
use notegate_model::{FileEncryptionMode, FileObject, Node};
use sqlx::PgPool;
use uuid::Uuid;

#[allow(dead_code)]
pub fn agent_api_key_prefix(key_id: Uuid) -> String {
    format!("ngk_v2_{key_id}")
}

/// Create an owner account + space, returning `(account_id, space_id, root_node_id)`.
#[allow(dead_code)]
pub async fn space_with_root(
    pool: &PgPool,
    sub: &str,
) -> Result<(Uuid, Uuid, Uuid), Box<dyn std::error::Error>> {
    let account = insert_user_account(pool, sub, &format!("{sub}@example.com")).await?;
    let space: Uuid =
        sqlx::query_scalar("INSERT INTO spaces (owner_user_id, name) VALUES ($1, $2) RETURNING id")
            .bind(account)
            .bind(format!("ws-{sub}"))
            .fetch_one(pool)
            .await?;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM nodes WHERE space_id = $1 AND parent_id IS NULL")
            .bind(space)
            .fetch_one(pool)
            .await?;
    Ok((account, space, root))
}

/// Seed old-schema migration tests without invoking the latest event writer.
#[allow(dead_code)]
pub async fn insert_legacy_user_account(pool: &PgPool) -> Result<Uuid, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let account: Uuid =
        sqlx::query_scalar("INSERT INTO accounts (kind) VALUES ('user') RETURNING id")
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("INSERT INTO users (id) VALUES ($1)")
        .bind(account)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(account)
}

/// Seed the schema 57–59 session writer shape for migration tests.
#[allow(dead_code)]
pub async fn insert_pre_marker_removal_session(
    pool: &PgPool,
    args: notegate_db::browser_session_repo::InsertBrowserSession<'_>,
    issuer: Option<&str>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO browser_sessions \
         (id,user_id,token_prefix,token_hash,hash_key_id,hash_version,validated_until,expires_at,local_session,oidc_issuer) \
         VALUES ($1,$2,$3,$4,'test-lookup',1,$5,$6,$7,$8)",
    )
    .bind(args.session_id)
    .bind(args.user_id)
    .bind(args.token_prefix)
    .bind(args.token_hash)
    .bind(args.idle_expires_at)
    .bind(args.expires_at)
    .bind(issuer.is_some())
    .bind(issuer)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO audit_events \
         (owner_user_id,actor_account_id,source,op_type,resource_type,resource_id,metadata) \
         VALUES ($1,$1,'rest','session.login','browser_session',$2,'{}'::jsonb)",
    )
    .bind(args.user_id)
    .bind(args.session_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

#[allow(dead_code)]
pub async fn legacy_space_with_root(
    pool: &PgPool,
    sub: &str,
) -> Result<(Uuid, Uuid, Uuid), sqlx::Error> {
    let account = insert_legacy_user_account(pool).await?;
    let space: Uuid =
        sqlx::query_scalar("INSERT INTO spaces (owner_user_id, name) VALUES ($1, $2) RETURNING id")
            .bind(account)
            .bind(format!("ws-{sub}"))
            .fetch_one(pool)
            .await?;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM nodes WHERE space_id = $1 AND parent_id IS NULL")
            .bind(space)
            .fetch_one(pool)
            .await?;
    Ok((account, space, root))
}

/// Record and attach an object-backed file without contacting object storage.
#[allow(dead_code)]
pub async fn attach_file(
    repo: &FilesRepo,
    space_id: Uuid,
    parent_node_id: Uuid,
    name: &str,
    byte_len: i64,
    account_id: Uuid,
) -> Result<(Node, FileObject), notegate_core::Error> {
    let upload_id = Uuid::new_v4();
    repo.insert_object_upload(
        upload_id,
        &format!("objects/{upload_id}"),
        space_id,
        account_id,
        &BeginObjectUpload {
            parent_node_id,
            name: name.to_owned(),
            byte_len,
            media_type: "application/octet-stream".to_owned(),
            original_filename: Some(name.to_owned()),
            encryption_mode: FileEncryptionMode::None,
            encryption_metadata: None,
        },
    )
    .await?;
    repo.attach_object_upload(upload_id, space_id, account_id, None, None)
        .await
}

/// Assign a test user to a product tier.
#[allow(dead_code)]
pub async fn set_user_tier(pool: &PgPool, user_id: Uuid, tier: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET tier = $2 WHERE id = $1")
        .bind(user_id)
        .bind(tier)
        .execute(pool)
        .await?;
    Ok(())
}

#[allow(dead_code)]
pub fn empty_change_metadata(space: Uuid) -> serde_json::Value {
    let encrypted = notegate_core::security::PiiCrypto::test()
        .encrypt_history(&format!("changes/{space}/{}", Uuid::nil()), "{}")
        .expect("fixed test key is valid");
    serde_json::to_value(encrypted).expect("history envelope serializes")
}

#[allow(dead_code)]
pub fn empty_invocation_payload(owner: Uuid) -> serde_json::Value {
    let encrypted = notegate_core::security::PiiCrypto::test()
        .encrypt_history(
            &format!("invocations/{owner}/{}", Uuid::nil()),
            r#"{"purpose":null,"space_name":null,"input":{},"response":null}"#,
        )
        .expect("fixed test key is valid");
    serde_json::to_value(encrypted).expect("history envelope serializes")
}
