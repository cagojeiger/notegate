mod common;

use chrono::{Duration, Utc};
use common::TestDb;
use notegate_core::security::{PiiAad, PiiCrypto, PiiFieldKind};
use notegate_db::account_repo::IdentityLink;
use notegate_db::browser_session_repo::{InsertBrowserSession, token_prefix};
use notegate_db::oauth_repo::OAuthRepo;
use notegate_db::{AccountRepo, BrowserSessionRepo, FilesRepo};
use notegate_model::ResolveAttrs;
use notegate_model::files::{StoredContent, WriteTextBody};
use sqlx::PgPool;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const ISSUER: &str = "https://auth.example.test";

async fn legacy_user(pool: &PgPool, sub: &str) -> TestResult<Uuid> {
    let id = common::insert_legacy_user_account(pool).await?;
    let crypto = PiiCrypto::test();
    let display = crypto.encrypt_pii_string(
        &PiiAad::new(
            PiiFieldKind::AccountDisplayName,
            id.to_string(),
            crypto.enc_key_id(),
        ),
        "Preserved User",
    )?;
    let email = crypto.encrypt_pii_string(
        &PiiAad::new(PiiFieldKind::UserEmail, id.to_string(), crypto.enc_key_id()),
        "preserved@example.test",
    )?;
    sqlx::query(
        "UPDATE accounts SET display_name_ciphertext=$2,display_name_nonce=$3, \
         display_name_enc_key_id=$4,display_name_enc_version=$5 WHERE id=$1",
    )
    .bind(id)
    .bind(display.ciphertext)
    .bind(display.nonce)
    .bind(crypto.enc_key_id())
    .bind(crypto.version())
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE users SET provider_sub_hash=$2,provider_sub_hash_key_id=$3,provider_sub_hash_version=$4, \
         email_ciphertext=$5,email_nonce=$6,email_enc_key_id=$7,email_enc_version=$4 WHERE id=$1",
    ).bind(id).bind(crypto.provider_sub_hash("authgate", sub)?).bind(crypto.lookup_key_id())
        .bind(crypto.version()).bind(email.ciphertext).bind(email.nonce)
        .bind(crypto.enc_key_id()).execute(pool).await?;
    Ok(id)
}

async fn bind_original_issuer(pool: &PgPool) -> TestResult {
    sqlx::query("INSERT INTO legacy_identity_issuer(issuer) VALUES ($1)")
        .bind(ISSUER)
        .execute(pool)
        .await?;
    Ok(())
}

fn link(account_id: Uuid, issuer: &str, subject: &str) -> IdentityLink {
    IdentityLink {
        account_id,
        issuer: issuer.to_owned(),
        subject: subject.to_owned(),
    }
}

#[tokio::test]
async fn unlinked_active_and_pending_deleted_accounts_block_identity_migration() -> TestResult {
    for deleted in [false, true] {
        let Some(db) = TestDb::setup_before(58).await? else {
            return Ok(());
        };
        let owner = legacy_user(&db.pool, "unlinked").await?;
        bind_original_issuer(&db.pool).await?;
        let repo = AccountRepo::new(db.pool.clone());
        if deleted {
            repo.soft_delete_user(owner, owner).await?;
        }
        assert!(db.apply_migration(58).await.is_err());
        let columns: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM information_schema.columns WHERE table_schema=current_schema() \
             AND table_name='users' AND column_name IN ('provider_sub_hash','provider_sub_hash_key_id','provider_sub_hash_version')",
        ).fetch_one(&db.pool).await?;
        assert_eq!(columns, 3);
        let identities: i64 = sqlx::query_scalar("SELECT count(*) FROM external_identities")
            .fetch_one(&db.pool)
            .await?;
        assert_eq!(identities, 0);
        assert!(repo.find_account(owner).await?.is_some());
        db.cleanup().await;
    }
    Ok(())
}

#[tokio::test]
async fn a_different_issuer_link_cannot_replace_the_original_identity() -> TestResult {
    let Some(db) = TestDb::setup_before(58).await? else {
        return Ok(());
    };
    let owner = legacy_user(&db.pool, "original-subject").await?;
    bind_original_issuer(&db.pool).await?;
    let repo = AccountRepo::new(db.pool.clone());
    repo.import_identity_links(&[link(
        owner,
        "https://other.example.test",
        "different-subject",
    )])
    .await?;
    assert!(db.apply_migration(58).await.is_err());
    repo.import_identity_links(&[link(owner, ISSUER, "original-subject")])
        .await?;
    db.apply_migration(58).await?;
    assert_eq!(
        repo.find_user_id_by_identity(ISSUER, "original-subject")
            .await?,
        Some(owner)
    );
    assert_eq!(
        repo.find_user_id_by_identity("https://other.example.test", "different-subject")
            .await?,
        Some(owner)
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn only_anonymized_account_shells_may_have_no_external_identity() -> TestResult {
    let Some(db) = TestDb::setup_before(58).await? else {
        return Ok(());
    };
    let owner = common::insert_legacy_user_account(&db.pool).await?;
    assert!(db.apply_migration(58).await.is_err());
    common::deactivate_account(&db.pool, owner, owner).await?;
    sqlx::query("UPDATE users SET anonymized_at=now() WHERE id=$1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    db.apply_migration(58).await?;
    assert!(
        AccountRepo::new(db.pool.clone())
            .find_account(owner)
            .await?
            .is_some()
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn identity_migration_preserves_ownership_pii_documents_sessions_and_oauth() -> TestResult {
    let Some(db) = TestDb::setup_before(58).await? else {
        return Ok(());
    };
    let owner = legacy_user(&db.pool, "preserved-subject").await?;
    let deleted = legacy_user(&db.pool, "deleted-subject").await?;
    bind_original_issuer(&db.pool).await?;
    let repo = AccountRepo::new(db.pool.clone());
    repo.import_identity_links(&[
        link(owner, ISSUER, "preserved-subject"),
        link(deleted, ISSUER, "deleted-subject"),
    ])
    .await?;
    repo.soft_delete_user(deleted, deleted).await?;
    let before_caller = repo
        .find_caller_by_account_id(owner)
        .await?
        .ok_or("owner missing")?;
    let space: Uuid = sqlx::query_scalar(
        "INSERT INTO spaces(owner_user_id,name) VALUES ($1,'Preserved') RETURNING id",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM nodes WHERE space_id=$1 AND parent_id IS NULL")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    let files = FilesRepo::new(db.pool.clone());
    let (document, _) = files
        .insert_text(
            space,
            root,
            "preserved.md",
            &StoredContent {
                body: WriteTextBody::Plain("preserved".to_owned()),
                content_sha256: "0d8da7dc169a0c12f0a6f121b819ca38b055b6e2440830f3453bb6c17d3c3106"
                    .to_owned(),
                byte_len: 9,
                line_count: 1,
            },
            owner,
        )
        .await?;
    let crypto = PiiCrypto::test();
    let now = Utc::now();
    let session_id = Uuid::new_v4();
    let session_hash = crypto.browser_session_hash(&session_id.to_string(), "preserved-session")?;
    let prefix = token_prefix(session_id);
    let sessions = BrowserSessionRepo::new(db.pool.clone());
    sessions
        .insert_local_session(
            InsertBrowserSession {
                session_id,
                user_id: owner,
                token_prefix: &prefix,
                token_hash: &session_hash,
                validated_until: now + Duration::hours(1),
                expires_at: now + Duration::days(7),
            },
            ISSUER,
        )
        .await?;
    let grant = Uuid::new_v4();
    let access_hash = crypto.oauth_hash("token", "preserved-access")?;
    let refresh_hash = crypto.oauth_hash("token", "preserved-refresh")?;
    let resource = "https://notegate.example.test/cli";
    sqlx::query(
        "INSERT INTO oauth_grants(id,user_id,client_id,client_name,resource,scope,created_at,expires_at,idle_expires_at) \
         VALUES ($1,$2,'notegate-cli','NoteGate CLI',$3,'cli offline_access',$4,$5,$5)",
    ).bind(grant).bind(owner).bind(resource).bind(now).bind(now+Duration::days(7))
        .execute(&db.pool).await?;
    sqlx::query(
        "INSERT INTO oauth_credentials(token_hash,hash_key_id,hash_version,grant_id,kind,expires_at) \
         VALUES ($1,'test-lookup',1,$3,'access',$4),($2,'test-lookup',1,$3,'refresh',$5)",
    ).bind(&access_hash).bind(&refresh_hash).bind(grant).bind(now+Duration::minutes(10))
        .bind(now+Duration::days(7)).execute(&db.pool).await?;

    let snapshot = "SELECT jsonb_build_object( \
        'accounts',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM accounts t), \
        'users',(SELECT jsonb_agg(to_jsonb(t)-ARRAY['provider_sub_hash','provider_sub_hash_key_id','provider_sub_hash_version'] ORDER BY id) FROM users t), \
        'identities',(SELECT jsonb_agg(to_jsonb(t) ORDER BY issuer,subject_hash) FROM external_identities t), \
        'spaces',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM spaces t), \
        'nodes',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM nodes t), \
        'text',(SELECT jsonb_agg(to_jsonb(t) ORDER BY node_id) FROM text_objects t), \
        'revisions',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM text_revisions t), \
        'audit',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM audit_events t), \
        'sessions',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM browser_sessions t), \
        'grants',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM oauth_grants t), \
        'credentials',(SELECT jsonb_agg(to_jsonb(t) ORDER BY token_hash) FROM oauth_credentials t))";
    let before: serde_json::Value = sqlx::query_scalar(snapshot).fetch_one(&db.pool).await?;
    notegate_db::run_migrations(&db.pool).await?;
    notegate_db::check_readiness(&db.pool).await?;
    let after: serde_json::Value = sqlx::query_scalar(snapshot).fetch_one(&db.pool).await?;
    assert_eq!(after, before);
    let removed: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM information_schema.columns WHERE table_schema=current_schema() \
         AND table_name='users' AND column_name IN ('provider_sub_hash','provider_sub_hash_key_id','provider_sub_hash_version')), \
         (SELECT count(*) FROM information_schema.tables WHERE table_schema=current_schema() AND table_name='legacy_identity_issuer')",
    ).fetch_one(&db.pool).await?;
    assert_eq!(removed, (0, 0));
    let caller = sessions
        .find_live_caller_by_token(session_id, &session_hash, &crypto)
        .await?
        .ok_or("session missing")?;
    assert_eq!(caller.account.id, owner);
    assert!(files.find_text(space, document.id).await?.is_some());
    assert_eq!(
        OAuthRepo::new(db.pool.clone())
            .access_user(&access_hash, resource, "cli", now)
            .await?,
        Some(owner)
    );
    let (account, user) = repo
        .upsert_user_by_identity(&ResolveAttrs {
            issuer: ISSUER.to_owned(),
            sub: "preserved-subject".to_owned(),
            email: "preserved@example.test".to_owned(),
            name: "Preserved User".to_owned(),
        })
        .await?;
    assert_eq!(account.id, before_caller.0.id);
    assert_eq!(account.created_at, before_caller.0.created_at);
    assert_eq!(account.display_name, before_caller.0.display_name);
    assert_eq!(user.email, before_caller.1.email);
    assert!(
        repo.upsert_user_by_identity(&ResolveAttrs {
            issuer: ISSUER.to_owned(),
            sub: "deleted-subject".to_owned(),
            email: "preserved@example.test".to_owned(),
            name: "Preserved User".to_owned(),
        })
        .await
        .is_err()
    );
    assert_eq!(
        repo.find_user_id_by_identity(ISSUER, "deleted-subject")
            .await?,
        Some(deleted)
    );
    assert!(sessions.revoke_session_for_logout(session_id).await?);
    let session_events: Vec<String> = sqlx::query_scalar(
        "SELECT op_type FROM audit_events WHERE resource_id=$1 AND resource_type='browser_session' ORDER BY id",
    ).bind(session_id).fetch_all(&db.pool).await?;
    assert_eq!(session_events, ["session.login", "session.logout"]);
    db.cleanup().await;
    Ok(())
}
