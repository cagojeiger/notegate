mod common;

use common::TestDb;
use notegate_core::security::{PiiAad, PiiCrypto, PiiFieldKind};
use notegate_db::{AccountRepo, PurgeRepo};
use notegate_model::ResolveAttrs;
use sqlx::PgPool;
use uuid::Uuid;

const LEGACY_ISSUER: &str = "https://auth.example.test";
const OTHER_ISSUER: &str = "https://accounts.google.com";

fn attrs(issuer: &str, sub: &str) -> ResolveAttrs {
    ResolveAttrs {
        issuer: issuer.to_owned(),
        sub: sub.to_owned(),
        email: "identity@example.test".to_owned(),
        name: "Identity User".to_owned(),
    }
}

async fn legacy_user(pool: &PgPool, sub: &str) -> Result<Uuid, Box<dyn std::error::Error>> {
    let id = common::insert_legacy_user_account(pool).await?;
    let crypto = PiiCrypto::test();
    let display = crypto.encrypt_pii_string(
        &PiiAad::new(
            PiiFieldKind::AccountDisplayName,
            id.to_string(),
            crypto.enc_key_id(),
        ),
        "Identity User",
    )?;
    let email = crypto.encrypt_pii_string(
        &PiiAad::new(PiiFieldKind::UserEmail, id.to_string(), crypto.enc_key_id()),
        "identity@example.test",
    )?;
    sqlx::query(
        "UPDATE accounts SET display_name_ciphertext = $2, display_name_nonce = $3, \
         display_name_enc_key_id = $4, display_name_enc_version = $5 WHERE id = $1",
    )
    .bind(id)
    .bind(display.ciphertext)
    .bind(display.nonce)
    .bind(crypto.enc_key_id())
    .bind(crypto.version())
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE users SET provider_sub_hash = $2, provider_sub_hash_key_id = $3, \
         provider_sub_hash_version = $4, email_ciphertext = $5, email_nonce = $6, \
         email_enc_key_id = $7, email_enc_version = $4 WHERE id = $1",
    )
    .bind(id)
    .bind(crypto.provider_sub_hash("authgate", sub)?)
    .bind(crypto.lookup_key_id())
    .bind(crypto.version())
    .bind(email.ciphertext)
    .bind(email.nonce)
    .bind(crypto.enc_key_id())
    .execute(pool)
    .await?;
    Ok(id)
}

#[tokio::test]
async fn migration_preserves_legacy_owner_and_encrypted_pii()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup_before(55).await? else {
        return Ok(());
    };
    let owner = legacy_user(&db.pool, "legacy-sub").await?;
    let space: Uuid = sqlx::query_scalar(
        "INSERT INTO spaces (owner_user_id, name) VALUES ($1, 'Legacy Space') RETURNING id",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    let repo = AccountRepo::new(db.pool.clone());
    let before = repo
        .find_caller_by_account_id(owner)
        .await?
        .ok_or("owner missing")?;

    db.apply_migration(55).await?;
    // An unbound database must not interpret the legacy subject under any issuer.
    assert_eq!(
        repo.find_user_id_by_identity(LEGACY_ISSUER, "legacy-sub")
            .await?,
        None
    );
    repo.ensure_legacy_identity_issuer(LEGACY_ISSUER).await?;
    assert_eq!(
        repo.find_user_id_by_identity(LEGACY_ISSUER, "legacy-sub")
            .await?,
        Some(owner)
    );
    assert_eq!(
        repo.find_user_id_by_identity(OTHER_ISSUER, "legacy-sub")
            .await?,
        None
    );

    let (after, user) = repo
        .upsert_user_by_identity(&attrs(LEGACY_ISSUER, "legacy-sub"))
        .await?;
    assert_eq!(after.id, before.0.id);
    assert_eq!(after.created_at, before.0.created_at);
    assert_eq!(after.display_name, before.0.display_name);
    assert_eq!(user.email, before.1.email);
    let preserved: (Uuid, i64, i64) = sqlx::query_as(
        "SELECT owner_user_id, \
         (SELECT count(*) FROM external_identities WHERE user_id = $2), \
         (SELECT count(*) FROM audit_events WHERE owner_user_id = $2 AND op_type = 'account.create') \
         FROM spaces WHERE id = $1",
    )
    .bind(space)
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(preserved, (owner, 1, 0));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn same_subject_and_email_under_different_issuers_are_separate()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());
    repo.ensure_legacy_identity_issuer(LEGACY_ISSUER).await?;
    let (first, _) = repo
        .upsert_user_by_identity(&attrs(LEGACY_ISSUER, "same-sub"))
        .await?;
    let (second, _) = repo
        .upsert_user_by_identity(&attrs(OTHER_ISSUER, "same-sub"))
        .await?;
    assert_ne!(first.id, second.id);
    assert_eq!(
        repo.find_user_id_by_identity(LEGACY_ISSUER, "same-sub")
            .await?,
        Some(first.id)
    );
    assert_eq!(
        repo.find_user_id_by_identity(OTHER_ISSUER, "same-sub")
            .await?,
        Some(second.id)
    );
    let legacy_hash: Option<String> =
        sqlx::query_scalar("SELECT provider_sub_hash FROM users WHERE id = $1")
            .bind(second.id)
            .fetch_one(&db.pool)
            .await?;
    assert!(legacy_hash.is_none());
    let legacy_hash: String =
        sqlx::query_scalar("SELECT provider_sub_hash FROM users WHERE id = $1")
            .bind(first.id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(
        legacy_hash,
        PiiCrypto::test().provider_sub_hash("authgate", "same-sub")?
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn legacy_issuer_is_idempotent_and_cannot_be_rebound()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());
    repo.ensure_legacy_identity_issuer(LEGACY_ISSUER).await?;
    repo.ensure_legacy_identity_issuer(LEGACY_ISSUER).await?;
    assert!(
        repo.ensure_legacy_identity_issuer(OTHER_ISSUER)
            .await
            .is_err()
    );
    let registered: String = sqlx::query_scalar("SELECT issuer FROM legacy_identity_issuer")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(registered, LEGACY_ISSUER);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_legacy_logins_keep_one_account_and_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = legacy_user(&db.pool, "concurrent-sub").await?;
    let repo = AccountRepo::new(db.pool.clone());
    repo.ensure_legacy_identity_issuer(LEGACY_ISSUER).await?;
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let repo = repo.clone();
        tasks.push(tokio::spawn(async move {
            repo.upsert_user_by_identity(&attrs(LEGACY_ISSUER, "concurrent-sub"))
                .await
        }));
    }
    for task in tasks {
        assert_eq!(task.await??.0.id, owner);
    }
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users), (SELECT count(*) FROM external_identities)",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(counts, (1, 1));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_first_logins_create_one_scoped_account()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let repo = repo.clone();
        tasks.push(tokio::spawn(async move {
            repo.upsert_user_by_identity(&attrs(OTHER_ISSUER, "first-login"))
                .await
        }));
    }
    let mut ids = std::collections::HashSet::new();
    for task in tasks {
        ids.insert(task.await??.0.id);
    }
    assert_eq!(ids.len(), 1);
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users), (SELECT count(*) FROM external_identities), \
         (SELECT count(*) FROM audit_events WHERE op_type = 'account.create')",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(counts, (1, 1, 1));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn scoped_identity_tombstone_is_removed_only_during_account_purge()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());
    repo.ensure_legacy_identity_issuer(LEGACY_ISSUER).await?;
    let identity = attrs(LEGACY_ISSUER, "deleted-sub");
    let (account, _) = repo.upsert_user_by_identity(&identity).await?;
    repo.soft_delete_user(account.id, account.id).await?;
    assert!(repo.upsert_user_by_identity(&identity).await.is_err());
    assert_eq!(
        repo.find_user_id_by_identity(LEGACY_ISSUER, "deleted-sub")
            .await?,
        Some(account.id)
    );
    assert_eq!(
        PurgeRepo::new(db.pool.clone())
            .run_once()
            .await?
            .accounts_anonymized,
        0
    );
    sqlx::query(
        "UPDATE accounts SET deleted_at = now() - make_interval(days => $2::int) WHERE id = $1",
    )
    .bind(account.id)
    .bind(i32::try_from(
        notegate_core::limits::ACCOUNT_DELETION_RETENTION_DAYS + 1,
    )?)
    .execute(&db.pool)
    .await?;
    sqlx::raw_sql("CREATE FUNCTION reject_identity_purge() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'identity purge failure'; END $$; CREATE TRIGGER reject_identity_purge BEFORE DELETE ON external_identities FOR EACH ROW EXECUTE FUNCTION reject_identity_purge();")
        .execute(&db.pool).await?;
    assert!(PurgeRepo::new(db.pool.clone()).run_once().await.is_err());
    assert_eq!(
        repo.find_user_id_by_identity(LEGACY_ISSUER, "deleted-sub")
            .await?,
        Some(account.id)
    );
    assert_eq!(
        repo.find_account(account.id)
            .await?
            .ok_or("account shell missing")?
            .display_name,
        "Identity User"
    );
    sqlx::query("DROP TRIGGER reject_identity_purge ON external_identities")
        .execute(&db.pool)
        .await?;
    assert_eq!(
        PurgeRepo::new(db.pool.clone())
            .run_once()
            .await?
            .accounts_anonymized,
        1
    );
    assert_eq!(
        repo.find_user_id_by_identity(LEGACY_ISSUER, "deleted-sub")
            .await?,
        None
    );
    let (new_account, _) = repo.upsert_user_by_identity(&identity).await?;
    assert_ne!(new_account.id, account.id);
    assert!(
        !repo
            .find_account(account.id)
            .await?
            .ok_or("account shell missing")?
            .is_live()
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn inconsistent_legacy_and_scoped_owners_fail_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = legacy_user(&db.pool, "conflicting-sub").await?;
    let repo = AccountRepo::new(db.pool.clone());
    repo.ensure_legacy_identity_issuer(LEGACY_ISSUER).await?;
    let (other, _) = repo
        .upsert_user_by_identity(&attrs(OTHER_ISSUER, "other-sub"))
        .await?;
    let crypto = PiiCrypto::test();
    sqlx::query(
        "INSERT INTO external_identities \
         (user_id, issuer, subject_hash, subject_hash_key_id, subject_hash_version) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(other.id)
    .bind(LEGACY_ISSUER)
    .bind(crypto.provider_sub_hash(LEGACY_ISSUER, "conflicting-sub")?)
    .bind(crypto.lookup_key_id())
    .bind(crypto.version())
    .execute(&db.pool)
    .await?;
    assert!(
        repo.find_user_id_by_identity(LEGACY_ISSUER, "conflicting-sub")
            .await
            .is_err()
    );
    assert!(
        repo.find_user_by_identity(LEGACY_ISSUER, "conflicting-sub")
            .await
            .is_err()
    );
    assert!(
        repo.upsert_user_by_identity(&attrs(LEGACY_ISSUER, "conflicting-sub"))
            .await
            .is_err()
    );
    assert_eq!(
        repo.find_account(owner)
            .await?
            .ok_or("owner missing")?
            .display_name,
        "Identity User"
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn verified_identity_import_is_idempotent_and_preserves_ownership()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = legacy_user(&db.pool, "authgate-uuid").await?;
    let repo = AccountRepo::new(db.pool.clone());
    repo.ensure_legacy_identity_issuer(LEGACY_ISSUER).await?;
    let space: Uuid = sqlx::query_scalar(
        "INSERT INTO spaces (owner_user_id,name) VALUES ($1,'Preserved') RETURNING id",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    let mappings = vec![notegate_db::account_repo::IdentityLink {
        account_id: owner,
        issuer: OTHER_ISSUER.to_owned(),
        subject: "verified-google-subject".to_owned(),
    }];
    repo.import_identity_links(&mappings).await?;
    repo.import_identity_links(&mappings).await?;
    let (account, _) = repo
        .upsert_user_by_identity(&attrs(OTHER_ISSUER, "verified-google-subject"))
        .await?;
    assert_eq!(account.id, owner);
    let space_owner: Uuid = sqlx::query_scalar("SELECT owner_user_id FROM spaces WHERE id=$1")
        .bind(space)
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(space_owner, owner);
    assert_eq!(
        repo.find_user_id_by_identity(LEGACY_ISSUER, "authgate-uuid")
            .await?,
        Some(owner)
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn conflicting_identity_import_rolls_back_all_mappings()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let first = legacy_user(&db.pool, "first").await?;
    let second = legacy_user(&db.pool, "second").await?;
    let repo = AccountRepo::new(db.pool.clone());
    let mapping = |account_id, subject: &str| notegate_db::account_repo::IdentityLink {
        account_id,
        issuer: OTHER_ISSUER.to_owned(),
        subject: subject.to_owned(),
    };
    repo.import_identity_links(&[mapping(first, "taken")])
        .await?;
    assert!(
        repo.import_identity_links(&[mapping(second, "new"), mapping(second, "taken")])
            .await
            .is_err()
    );
    assert_eq!(
        repo.find_user_id_by_identity(OTHER_ISSUER, "new").await?,
        None
    );
    assert_eq!(
        repo.find_user_id_by_identity(OTHER_ISSUER, "taken").await?,
        Some(first)
    );
    db.cleanup().await;
    Ok(())
}
