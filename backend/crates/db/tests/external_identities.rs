mod common;

use common::TestDb;
use notegate_core::security::PiiCrypto;
use notegate_db::{AccountRepo, PurgeRepo};
use notegate_model::ResolveAttrs;
use sqlx::PgPool;
use uuid::Uuid;

const AUTH_ISSUER: &str = "https://auth.example.test";
const OTHER_ISSUER: &str = "https://accounts.google.com";

fn attrs(issuer: &str, sub: &str) -> ResolveAttrs {
    ResolveAttrs {
        issuer: issuer.to_owned(),
        sub: sub.to_owned(),
        email: "identity@example.test".to_owned(),
        name: "Identity User".to_owned(),
    }
}

async fn existing_user(pool: &PgPool, sub: &str) -> Result<Uuid, Box<dyn std::error::Error>> {
    Ok(AccountRepo::new(pool.clone())
        .upsert_user_by_identity(&attrs(AUTH_ISSUER, sub))
        .await?
        .0
        .id)
}

#[tokio::test]
async fn same_subject_and_email_under_different_issuers_are_separate()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());
    let (first, _) = repo
        .upsert_user_by_identity(&attrs(AUTH_ISSUER, "same-sub"))
        .await?;
    let (second, _) = repo
        .upsert_user_by_identity(&attrs(OTHER_ISSUER, "same-sub"))
        .await?;
    assert_ne!(first.id, second.id);
    assert_eq!(
        repo.find_user_id_by_identity(AUTH_ISSUER, "same-sub")
            .await?,
        Some(first.id)
    );
    assert_eq!(
        repo.find_user_id_by_identity(OTHER_ISSUER, "same-sub")
            .await?,
        Some(second.id)
    );
    for (id, issuer) in [(first.id, AUTH_ISSUER), (second.id, OTHER_ISSUER)] {
        let hash: String = sqlx::query_scalar(
            "SELECT subject_hash FROM external_identities WHERE user_id = $1 AND issuer = $2",
        )
        .bind(id)
        .bind(issuer)
        .fetch_one(&db.pool)
        .await?;
        assert_eq!(
            hash,
            PiiCrypto::test().provider_sub_hash(issuer, "same-sub")?
        );
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_existing_logins_keep_one_account_and_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = existing_user(&db.pool, "concurrent-sub").await?;
    let repo = AccountRepo::new(db.pool.clone());
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let repo = repo.clone();
        tasks.push(tokio::spawn(async move {
            repo.upsert_user_by_identity(&attrs(AUTH_ISSUER, "concurrent-sub"))
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
    let identity = attrs(AUTH_ISSUER, "deleted-sub");
    let (account, _) = repo.upsert_user_by_identity(&identity).await?;
    repo.soft_delete_user(account.id, account.id).await?;
    assert!(repo.upsert_user_by_identity(&identity).await.is_err());
    assert_eq!(
        repo.find_user_id_by_identity(AUTH_ISSUER, "deleted-sub")
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
        repo.find_user_id_by_identity(AUTH_ISSUER, "deleted-sub")
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
        repo.find_user_id_by_identity(AUTH_ISSUER, "deleted-sub")
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
async fn verified_identity_import_is_idempotent_and_preserves_ownership()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = existing_user(&db.pool, "authgate-uuid").await?;
    let repo = AccountRepo::new(db.pool.clone());
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
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE op_type='identity.link' AND owner_user_id=$1",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(audit_count, 1);
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
        repo.find_user_id_by_identity(AUTH_ISSUER, "authgate-uuid")
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
    let first = existing_user(&db.pool, "first").await?;
    let second = existing_user(&db.pool, "second").await?;
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
