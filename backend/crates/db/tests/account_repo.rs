//! Integration tests for `AccountRepo` against a real Postgres schema.
//!
//! Run with:
//! `NOTEGATE_TEST_DATABASE_URL=postgres://notegate:notegate@localhost:5433/notegate \
//!  cargo test -p notegate-db --test account_repo`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::TestDb;
use notegate_core::{Error, limits};
use notegate_db::api_key_repo::InsertApiKey;
use notegate_db::{AccessRepo, AccountRepo, AgentRepo, ApiKeyRepo, PurgeRepo, WorkspaceRepo};
use notegate_model::{CreateAgent, CreateApiKey, CreateWorkspace, GrantAccess, ResolveAttrs, Role};
use sqlx::Row as _;

fn attrs(sub: &str, email: &str, name: &str) -> ResolveAttrs {
    ResolveAttrs {
        sub: sub.to_owned(),
        email: email.to_owned(),
        name: name.to_owned(),
    }
}

#[tokio::test]
async fn upsert_user_creates_account_and_user_rows() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (account, user) = repo
        .upsert_user_by_sub(&attrs("sub-1", "a@example.test", "Kang"))
        .await?;
    assert_eq!(account.id, user.id);
    assert_eq!(account.kind.as_str(), "user");
    assert_eq!(account.display_name, "Kang");
    assert!(account.is_active);
    assert_eq!(user.email.as_deref(), Some("a@example.test"));

    let plaintext_matches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM accounts \
         WHERE display_name_ciphertext::text LIKE '%Kang%'",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(plaintext_matches, 0);

    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&db.pool)
        .await?;
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(accounts, 1);
    assert_eq!(users, 1);
    let workspace_count: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(
        workspace_count, 0,
        "new user does not auto-create a workspace"
    );
    let access_count: i64 = sqlx::query_scalar("SELECT count(*) FROM workspace_access")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(access_count, 0, "new user does not auto-create access rows");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn upsert_user_rejects_identity_fields_over_system_limits()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let cases = [
        attrs(
            &"s".repeat(limits::OAUTH_PROVIDER_SUB_MAX_CHARS + 1),
            "a@example.test",
            "Kang",
        ),
        attrs(
            "sub-ok",
            "a@example.test",
            &"n".repeat(limits::USER_DISPLAY_NAME_MAX_CHARS + 1),
        ),
        attrs(
            "sub-ok",
            &format!("{}@example.test", "e".repeat(limits::USER_EMAIL_MAX_CHARS)),
            "Kang",
        ),
    ];

    for input in cases {
        let err = repo.upsert_user_by_sub(&input).await.unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    db.cleanup().await;
    Ok(())
}

/// P1-3: a stored PII enc version that diverges from the active crypto version
/// surfaces a clear error before decryption; NULL-PII rows stay non-erroring.
#[tokio::test]
async fn mismatched_pii_version_returns_clear_error() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());
    let (account, _) = repo
        .upsert_user_by_sub(&attrs("ver-sub", "ver@example.test", "Ver"))
        .await?;

    // Corrupt the stored account display-name version to a value the active crypto
    // does not match.
    sqlx::query("UPDATE accounts SET display_name_enc_version = 999 WHERE id = $1")
        .bind(account.id)
        .execute(&db.pool)
        .await?;
    let err = repo.find_account(account.id).await.unwrap_err();
    assert!(
        matches!(&err, Error::Internal(message) if message.contains("PII enc version mismatch")),
        "expected version-mismatch error, got {err:?}"
    );

    // Corrupt the stored email version too and assert the user read errors clearly.
    sqlx::query("UPDATE accounts SET display_name_enc_version = 1 WHERE id = $1")
        .bind(account.id)
        .execute(&db.pool)
        .await?;
    sqlx::query("UPDATE users SET email_enc_version = 999 WHERE id = $1")
        .bind(account.id)
        .execute(&db.pool)
        .await?;
    let err = repo
        .find_caller_by_account_id(account.id)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::Internal(message) if message.contains("PII enc version mismatch")),
        "expected version-mismatch error, got {err:?}"
    );

    // A NULL-PII row (the anonymized shell left by the purge run) must read back
    // without erroring.
    sqlx::query(
        "UPDATE accounts SET display_name_ciphertext = NULL, display_name_nonce = NULL, \
         display_name_enc_key_id = NULL, display_name_enc_version = NULL WHERE id = $1",
    )
    .bind(account.id)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "UPDATE users SET email_ciphertext = NULL, email_nonce = NULL, \
         email_enc_key_id = NULL, email_enc_version = NULL, email_hash = NULL, \
         email_hash_key_id = NULL, email_hash_version = NULL WHERE id = $1",
    )
    .bind(account.id)
    .execute(&db.pool)
    .await?;
    let caller = repo.find_caller_by_account_id(account.id).await?;
    let (anon_account, anon_user) = caller.expect("anonymized account still resolves");
    assert_eq!(anon_account.display_name, "");
    assert_eq!(anon_user.email, None);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn duplicate_sub_updates_and_does_not_duplicate() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (first, _) = repo
        .upsert_user_by_sub(&attrs("sub-dup", "old@example.test", "Old Name"))
        .await?;
    let (second, user) = repo
        .upsert_user_by_sub(&attrs("sub-dup", "new@example.test", "New Name"))
        .await?;

    assert_eq!(first.id, second.id, "same sub must reuse the same account");
    assert_eq!(second.display_name, "New Name");
    assert_eq!(user.email.as_deref(), Some("new@example.test"));

    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(
        accounts, 1,
        "duplicate sub must not create a second account"
    );
    let workspaces: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(workspaces, 0, "duplicate sub must not create workspaces");

    db.cleanup().await;
    Ok(())
}

/// ADR 0004: re-registering a sub whose account is in a soft-deleted (pending-deletion)
/// state is rejected — the account is neither reactivated nor duplicated.
#[tokio::test]
async fn duplicate_sub_on_pending_deletion_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (first, _) = repo
        .upsert_user_by_sub(&attrs("sub-inactive", "old@example.test", "Old Name"))
        .await?;
    sqlx::query(
        "UPDATE accounts \
         SET is_active = false, deleted_at = now(), deleted_by = $1 \
         WHERE id = $1",
    )
    .bind(first.id)
    .execute(&db.pool)
    .await?;

    let err = repo
        .upsert_user_by_sub(&attrs("sub-inactive", "new@example.test", "New Name"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Conflict(_)),
        "pending-deletion sub must be rejected, got {err:?}"
    );

    // No duplicate, PII not refreshed, still inactive.
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(accounts, 1, "no duplicate account is created");
    let still = repo
        .find_account(first.id)
        .await?
        .ok_or("account remains")?;
    assert!(!still.is_active);
    assert_eq!(still.display_name, "Old Name", "PII not refreshed");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn find_user_by_sub_and_account_resolve_the_pair() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (created, _) = repo
        .upsert_user_by_sub(&attrs("sub-find", "f@example.test", "Finder"))
        .await?;

    let (account, user) = repo
        .find_user_by_sub("sub-find")
        .await?
        .ok_or("user resolves by sub")?;
    assert_eq!(account.id, created.id);
    assert_eq!(user.email.as_deref(), Some("f@example.test"));

    let by_id = repo.find_caller_by_account_id(created.id).await?;
    assert_eq!(by_id.map(|(a, _)| a.id), Some(created.id));

    let by_account = repo.find_account(created.id).await?;
    assert_eq!(by_account.map(|a| a.id), Some(created.id));

    assert!(repo.find_user_by_sub("nope").await?.is_none());

    db.cleanup().await;
    Ok(())
}

/// ADR 0004: soft-delete marks the account deleted and tears down lifecycle, but KEEPS
/// PII and the `provider_sub_hash` tombstone — anonymization happens later at purge.
#[tokio::test]
async fn soft_delete_user_marks_deleted_and_keeps_tombstone()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (account, _) = repo
        .upsert_user_by_sub(&attrs("sub-gone", "g@example.test", "Gone"))
        .await?;

    repo.soft_delete_user(account.id, account.id).await?;

    let after = repo
        .find_account(account.id)
        .await?
        .ok_or("account still present after soft delete")?;
    assert!(!after.is_active);
    assert!(after.deleted_at.is_some());
    assert_eq!(after.deleted_by, Some(account.id));
    assert_eq!(after.display_name, "Gone", "PII is retained until purge");

    let row = sqlx::query(
        "SELECT provider_sub_hash, email_hash, email_ciphertext, anonymized_at \
         FROM users WHERE id = $1",
    )
    .bind(account.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(
        row.get::<Option<String>, _>("provider_sub_hash").is_some(),
        "sub-hash tombstone is retained until purge"
    );
    assert!(row.get::<Option<String>, _>("email_hash").is_some());
    assert!(row.get::<Option<Vec<u8>>, _>("email_ciphertext").is_some());
    assert!(
        row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("anonymized_at")
            .is_none(),
        "not anonymized until the purge run"
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn soft_delete_user_tears_down_owned_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let accounts = AccountRepo::new(db.pool.clone());
    let agents = AgentRepo::new(db.pool.clone());
    let api_keys = ApiKeyRepo::new(db.pool.clone());
    let access = AccessRepo::new(db.pool.clone());
    let workspaces = WorkspaceRepo::new(db.pool.clone());

    let (owner, _) = accounts
        .upsert_user_by_sub(&attrs("owner-sub", "owner@example.test", "Owner"))
        .await?;
    let (member, _) = accounts
        .upsert_user_by_sub(&attrs("member-sub", "member@example.test", "Member"))
        .await?;

    let workspace_id = workspaces
        .create_workspace(
            owner.id,
            &CreateWorkspace {
                name: "owned".to_owned(),
            },
        )
        .await?
        .id;

    let agent = agents
        .insert_agent(
            &CreateAgent {
                name: "owned-agent".to_owned(),
            },
            owner.id,
        )
        .await?;
    let key_id = uuid::Uuid::new_v4();
    api_keys
        .insert_key_unchecked_for_test(InsertApiKey {
            key_id,
            account_id: agent.id,
            command: &CreateApiKey {
                name: "key".to_owned(),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
            },
            token_prefix: "ngk_v1_agent",
            token_hash: "hashed-token",
            created_by: owner.id,
            rotated_from_key_id: None,
        })
        .await?;
    let user_key_id = uuid::Uuid::new_v4();
    api_keys
        .insert_key_unchecked_for_test(InsertApiKey {
            key_id: user_key_id,
            account_id: owner.id,
            command: &CreateApiKey {
                name: "owner-key".to_owned(),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
            },
            token_prefix: "ngk_v1_owner",
            token_hash: "hashed-owner-token",
            created_by: owner.id,
            rotated_from_key_id: None,
        })
        .await?;

    access
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: member.id,
                role: Role::Viewer,
            },
            owner.id,
        )
        .await?;
    access
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: agent.id,
                role: Role::Editor,
            },
            owner.id,
        )
        .await?;

    accounts.soft_delete_user(owner.id, owner.id).await?;

    let account = accounts
        .find_account(owner.id)
        .await?
        .ok_or("owner account remains as attribution target")?;
    assert!(!account.is_active);
    assert_eq!(account.display_name, "Owner", "PII retained until purge");
    assert!(
        accounts.find_user_by_sub("owner-sub").await?.is_some(),
        "sub-hash tombstone retained until purge"
    );

    let workspace_deleted: (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<uuid::Uuid>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as("SELECT deleted_at, deleted_by, purge_after FROM workspaces WHERE id = $1")
        .bind(workspace_id)
        .fetch_one(&db.pool)
        .await?;
    assert!(workspace_deleted.0.is_some());
    assert_eq!(workspace_deleted.1, Some(owner.id));
    assert!(workspace_deleted.2.is_some());

    let agent_active: bool = sqlx::query_scalar("SELECT is_active FROM accounts WHERE id = $1")
        .bind(agent.id)
        .fetch_one(&db.pool)
        .await?;
    assert!(!agent_active);

    let key_revoked_by: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT revoked_by FROM api_keys WHERE id = $1")
            .bind(key_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(key_revoked_by, Some(owner.id));

    let user_key_revoked_by: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT revoked_by FROM api_keys WHERE id = $1")
            .bind(user_key_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(user_key_revoked_by, Some(owner.id));

    let live_access: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_access WHERE workspace_id = $1 AND revoked_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(live_access, 0);

    db.cleanup().await;
    Ok(())
}

/// ADR 0004: after soft-delete, re-login with the same OAuth sub is rejected (cooldown)
/// until the purge run anonymizes the account; only then does the same sub register as a
/// fresh, unrelated account. Pins the fix for the duplicate-account bug.
#[tokio::test]
async fn reregister_blocked_until_purge_then_creates_fresh_account()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (first, _) = repo
        .upsert_user_by_sub(&attrs("sub-rejoin", "rejoin@example.test", "Rejoin"))
        .await?;
    repo.soft_delete_user(first.id, first.id).await?;

    // During the cooldown window the same sub cannot re-register.
    let err = repo
        .upsert_user_by_sub(&attrs("sub-rejoin", "rejoin@example.test", "Rejoin"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Conflict(_)),
        "re-login during cooldown must be rejected, got {err:?}"
    );

    // Advance past the retention window and run the purge to free the tombstone.
    sqlx::query("UPDATE accounts SET deleted_at = now() - interval '30 days' WHERE id = $1")
        .bind(first.id)
        .execute(&db.pool)
        .await?;
    let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(run.accounts_anonymized, 1);

    // Now the same sub registers as a brand-new, unrelated account.
    let (second, _) = repo
        .upsert_user_by_sub(&attrs("sub-rejoin", "rejoin@example.test", "Rejoin"))
        .await?;
    assert_ne!(
        first.id, second.id,
        "post-purge re-registration is a fresh account"
    );
    assert!(second.is_active);

    // The old identity survives only as an anonymized attribution shell.
    let shell = repo
        .find_account(first.id)
        .await?
        .ok_or("anonymized attribution shell is kept")?;
    assert!(!shell.is_active);
    assert_eq!(shell.display_name, "", "shell PII is anonymized at purge");

    db.cleanup().await;
    Ok(())
}
