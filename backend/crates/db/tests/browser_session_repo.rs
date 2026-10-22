#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use chrono::{Duration, Utc};
use common::{TestDb, deactivate_account, insert_user_account, space_with_root};
use notegate_core::security::PiiCrypto;
use notegate_db::BrowserSessionRepo;
use notegate_db::browser_session_repo::{
    InsertBrowserSession, format_token, parse_token, token_prefix,
};
use notegate_db::oauth_repo::{CredentialIssue, ExchangeOutcome, OAuthRepo};
use uuid::Uuid;

async fn audit_op_types(
    db: &TestDb,
    user_id: Uuid,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(sqlx::query_scalar(
        "SELECT op_type FROM audit_events \
             WHERE owner_user_id = $1 AND op_type LIKE 'session.%' ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(&db.pool)
    .await?)
}

async fn insert_session(
    repo: &BrowserSessionRepo,
    crypto: &PiiCrypto,
    user_id: Uuid,
) -> Result<(Uuid, String), Box<dyn std::error::Error>> {
    let session_id = Uuid::new_v4();
    let secret = "session-secret";
    let token_hash = crypto.browser_session_hash(&session_id.to_string(), secret)?;
    let prefix = token_prefix(session_id);
    repo.insert_session(
        InsertBrowserSession {
            client_environment: None,
            session_id,
            user_id,
            token_prefix: &prefix,
            token_hash: &token_hash,
            idle_expires_at: Utc::now() + Duration::hours(1),
            expires_at: Utc::now() + Duration::days(15),
        },
        "https://auth.example.test",
    )
    .await?;
    Ok((session_id, token_hash))
}

#[tokio::test]
async fn inserted_session_can_be_found_by_hash() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user_id = insert_user_account(&db.pool, "browser-user", "browser@example.test").await?;
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let crypto = PiiCrypto::test();
    let (session_id, token_hash) = insert_session(&repo, &crypto, user_id).await?;

    let found = repo
        .find_live_by_token(session_id, &token_hash)
        .await?
        .expect("session should be live");

    assert_eq!(found.user_id, user_id);
    let issuer: String = sqlx::query_scalar("SELECT metadata->>'issuer' FROM audit_events WHERE resource_id=$1 AND op_type='session.login'")
        .bind(session_id).fetch_one(&db.pool).await?;
    assert_eq!(issuer, "https://auth.example.test");
    let resolved = repo
        .find_live_caller_by_token(session_id, &token_hash, &crypto)
        .await?
        .expect("session caller should be live");
    assert_eq!(resolved.session_id, session_id);
    assert_eq!(resolved.account.id, user_id);
    assert_eq!(resolved.account.display_name, "user-browser-user");
    assert_eq!(resolved.user.email.as_deref(), Some("browser@example.test"));
    assert_eq!(audit_op_types(&db, user_id).await?, ["session.login"]);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn bulk_browser_revocation_rolls_back_when_audit_cannot_be_recorded()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "revoke-owner", "owner@example.test").await?;
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let crypto = PiiCrypto::test();
    let (first, first_hash) = insert_session(&repo, &crypto, owner).await?;
    let (second, second_hash) = insert_session(&repo, &crypto, owner).await?;
    sqlx::query("ALTER TABLE audit_events ADD CONSTRAINT test_revoke_audit CHECK(op_type<>'session.revoke') NOT VALID").execute(&db.pool).await?;
    assert!(repo.revoke_owned_sessions(owner, None, None).await.is_err());
    assert!(repo.find_live_by_token(first, &first_hash).await?.is_some());
    assert!(
        repo.find_live_by_token(second, &second_hash)
            .await?
            .is_some()
    );
    assert_eq!(
        audit_op_types(&db, owner).await?,
        ["session.login", "session.login"]
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn capped_session_inventory_always_includes_the_current_session()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "inventory-owner", "owner@example.test").await?;
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let crypto = PiiCrypto::test();
    let (current, hash) = insert_session(&repo, &crypto, owner).await?;
    sqlx::query("UPDATE browser_sessions SET created_at=now()-interval '3 days' WHERE id=$1")
        .bind(current)
        .execute(&db.pool)
        .await?;
    sqlx::query("INSERT INTO browser_sessions(id,user_id,token_prefix,token_hash,hash_key_id,hash_version,idle_expires_at,expires_at) \
        SELECT id,$1,'ngs_v1_'||id::text,md5(id::text),'test-lookup',1,now()+interval '1 hour',now()+interval '15 days' \
        FROM (SELECT gen_random_uuid() AS id FROM generate_series(1,100)) s")
        .bind(owner).execute(&db.pool).await?;
    assert!(repo.renew_idle_expiry(current, &hash).await?);
    let sessions = repo.list_owned_live(owner, current).await?;
    assert_eq!(sessions.len(), 100);
    assert_eq!(sessions.first().map(|s| s.id), Some(current));
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM browser_sessions WHERE user_id=$1")
        .bind(owner)
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(total, 101);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn revoked_or_inactive_sessions_are_not_live() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user_id = insert_user_account(&db.pool, "inactive-user", "inactive@example.test").await?;
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let crypto = PiiCrypto::test();
    let (revoked_id, revoked_hash) = insert_session(&repo, &crypto, user_id).await?;
    assert!(repo.revoke_session_for_logout(revoked_id).await?);
    assert!(!repo.revoke_session_for_logout(revoked_id).await?);
    assert_eq!(
        audit_op_types(&db, user_id).await?,
        ["session.login", "session.logout"]
    );
    assert!(
        repo.find_live_by_token(revoked_id, &revoked_hash)
            .await?
            .is_none()
    );

    let (inactive_id, inactive_hash) = insert_session(&repo, &crypto, user_id).await?;
    deactivate_account(&db.pool, user_id, user_id).await?;
    assert!(
        repo.find_live_by_token(inactive_id, &inactive_hash)
            .await?
            .is_none()
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn browser_activity_extends_idle_expiry_without_moving_absolute_expiry()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "active-browser", "browser@example.test").await?;
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let crypto = PiiCrypto::test();
    let (id, hash) = insert_session(&repo, &crypto, user).await?;
    let before = repo
        .find_by_token(id, &hash)
        .await?
        .ok_or("missing session")?;
    assert!(
        repo.find_live_caller_by_token(id, &hash, &crypto)
            .await?
            .is_some()
    );
    assert_eq!(
        repo.find_by_token(id, &hash)
            .await?
            .unwrap()
            .idle_expires_at,
        before.idle_expires_at
    );
    let before_renewal: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&db.pool)
        .await?;
    assert!(repo.renew_idle_expiry(id, &hash).await?);
    let after = repo
        .find_by_token(id, &hash)
        .await?
        .ok_or("missing renewed session")?;
    let after_renewal: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&db.pool)
        .await?;
    assert!(after.idle_expires_at >= before_renewal + Duration::days(14));
    assert!(after.idle_expires_at <= after_renewal + Duration::days(14));
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.expires_at, before.expires_at);
    assert_eq!(after.user_id, before.user_id);
    assert_eq!(audit_op_types(&db, user).await?, ["session.login"]);
    sqlx::query("UPDATE browser_sessions SET expires_at=now()+interval '1 day', idle_expires_at=now()+interval '1 hour' WHERE id=$1")
        .bind(id).execute(&db.pool).await?;
    assert!(repo.renew_idle_expiry(id, &hash).await?);
    let capped = repo.find_by_token(id, &hash).await?.unwrap();
    assert_eq!(capped.idle_expires_at, capped.expires_at);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn browser_activity_cannot_revive_expired_revoked_inactive_or_foreign_sessions()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let crypto = PiiCrypto::test();
    for case in ["idle", "absolute", "revoked", "inactive", "deleted", "hash"] {
        let user = insert_user_account(&db.pool, case, "browser@example.test").await?;
        let (id, hash) = insert_session(&repo, &crypto, user).await?;
        match case {
            "idle" => {
                sqlx::query("UPDATE browser_sessions SET idle_expires_at=now() WHERE id=$1")
                    .bind(id)
                    .execute(&db.pool)
                    .await?;
            }
            "absolute" => {
                sqlx::query("UPDATE browser_sessions SET idle_expires_at=now(), expires_at=now() WHERE id=$1").bind(id).execute(&db.pool).await?;
            }
            "revoked" => {
                repo.revoke_session_for_logout(id).await?;
            }
            "inactive" => {
                sqlx::query("UPDATE accounts SET is_active=false WHERE id=$1")
                    .bind(user)
                    .execute(&db.pool)
                    .await?;
            }
            "deleted" => {
                deactivate_account(&db.pool, user, user).await?;
            }
            _ => {}
        }
        let before: serde_json::Value =
            sqlx::query_scalar("SELECT to_jsonb(t) FROM browser_sessions t WHERE id=$1")
                .bind(id)
                .fetch_one(&db.pool)
                .await?;
        let supplied_hash = if case == "hash" { "wrong-hash" } else { &hash };
        assert!(!repo.renew_idle_expiry(id, supplied_hash).await?, "{case}");
        let after: serde_json::Value =
            sqlx::query_scalar("SELECT to_jsonb(t) FROM browser_sessions t WHERE id=$1")
                .bind(id)
                .fetch_one(&db.pool)
                .await?;
        assert_eq!(after, before, "{case}");
        if case != "hash" {
            assert!(repo.find_live_by_token(id, &hash).await?.is_none());
            assert!(
                repo.find_live_caller_by_token(id, &hash, &crypto)
                    .await?
                    .is_none()
            );
        }
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_browser_activity_never_undoes_logout() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "logout-race", "browser@example.test").await?;
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let crypto = PiiCrypto::test();
    let (id, hash) = insert_session(&repo, &crypto, user).await?;
    let (renewed, revoked) = tokio::join!(
        repo.renew_idle_expiry(id, &hash),
        repo.revoke_session_for_logout(id),
    );
    renewed?;
    assert!(revoked?);
    assert!(!repo.renew_idle_expiry(id, &hash).await?);
    assert!(repo.find_live_by_token(id, &hash).await?.is_none());
    assert_eq!(
        audit_op_types(&db, user).await?,
        ["session.login", "session.logout"]
    );
    let (remote_id, remote_hash) = insert_session(&repo, &crypto, user).await?;
    let (renewed, revoked) = tokio::join!(
        repo.renew_idle_expiry(remote_id, &remote_hash),
        repo.revoke_owned_sessions(user, Some(remote_id), None),
    );
    renewed?;
    assert_eq!(revoked?, 1);
    assert!(!repo.renew_idle_expiry(remote_id, &remote_hash).await?);
    assert!(
        repo.find_live_by_token(remote_id, &remote_hash)
            .await?
            .is_none()
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn dropping_browser_refresh_storage_preserves_sessions_and_owned_data()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup_before(57).await? else {
        return Ok(());
    };
    let (user, _, _) = space_with_root(&db.pool, "browser-migration").await?;
    let crypto = PiiCrypto::test();
    let now: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&db.pool)
        .await?;
    let local_id = Uuid::new_v4();
    let legacy_id = Uuid::new_v4();
    for (id, local) in [(local_id, true), (legacy_id, false)] {
        let hash = crypto.browser_session_hash(&id.to_string(), "existing-session")?;
        // The retired ciphertext is deliberately opaque: migration must not decrypt it.
        sqlx::query(
            "INSERT INTO browser_sessions \
             (id,user_id,token_prefix,token_hash,hash_key_id,hash_version, \
              refresh_token_ciphertext,refresh_token_nonce,refresh_token_enc_key_id,refresh_token_enc_version, \
              validated_until,expires_at,last_refreshed_at,refresh_started_at,refresh_claim_id,local_session,oidc_issuer) \
             VALUES ($1,$2,$3,$4,'test-lookup',1,'opaque'::bytea,decode('000000000000000000000000','hex'),'test-enc',1, \
                     $5,$6,$7,$7,$1,$8,$9)",
        )
        .bind(id)
        .bind(user)
        .bind(token_prefix(id))
        .bind(hash)
        .bind(now + Duration::hours(1))
        .bind(now + Duration::days(15))
        .bind(now)
        .bind(local)
        .bind(local.then_some("https://auth.example.test"))
        .execute(&db.pool)
        .await?;
    }
    let grant = Uuid::new_v4();
    let resource = "https://notegate.example.test/cli";
    let access = crypto.oauth_hash("token", "existing-access")?;
    let refresh = crypto.oauth_hash("token", "existing-refresh")?;
    sqlx::query(
        "INSERT INTO oauth_grants \
         (id,user_id,client_id,client_name,resource,scope,created_at,expires_at,idle_expires_at) \
         VALUES ($1,$2,'notegate-cli','NoteGate CLI',$3,'cli offline_access',$4,$5,$5)",
    )
    .bind(grant)
    .bind(user)
    .bind(resource)
    .bind(now)
    .bind(now + Duration::days(7))
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO oauth_credentials (token_hash,hash_key_id,hash_version,grant_id,kind,expires_at) \
         VALUES ($1,'test-lookup',1,$3,'access',$4), ($2,'test-lookup',1,$3,'refresh',$5)",
    )
    .bind(&access).bind(&refresh).bind(grant)
    .bind(now + Duration::minutes(10)).bind(now + Duration::days(7))
    .execute(&db.pool).await?;

    let removed = [
        "refresh_token_ciphertext",
        "refresh_token_nonce",
        "refresh_token_enc_key_id",
        "refresh_token_enc_version",
        "last_refreshed_at",
        "refresh_started_at",
        "refresh_claim_id",
    ];
    let columns_query = "SELECT count(*) FROM information_schema.columns \
        WHERE table_schema=current_schema() AND table_name='browser_sessions' AND column_name=ANY($1)";
    assert_eq!(
        sqlx::query_scalar::<_, i64>(columns_query)
            .bind(&removed[..])
            .fetch_one(&db.pool)
            .await?,
        7
    );
    // Compare every retained field, including IDs, hashes, timestamps, ownership and encrypted PII.
    let snapshot = "SELECT jsonb_build_object( \
        'accounts',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM accounts t), \
        'users',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM users t), \
        'identities',(SELECT jsonb_agg(to_jsonb(t) ORDER BY issuer,subject_hash) FROM external_identities t), \
        'spaces',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM spaces t), \
        'nodes',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM nodes t), \
        'audit',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM audit_events t), \
        'sessions',(SELECT jsonb_agg(to_jsonb(t)-$1::text[] ORDER BY id) FROM browser_sessions t), \
        'grants',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM oauth_grants t), \
        'credentials',(SELECT jsonb_agg(to_jsonb(t) ORDER BY token_hash) FROM oauth_credentials t))";
    let before: serde_json::Value = sqlx::query_scalar(snapshot)
        .bind(&removed[..])
        .fetch_one(&db.pool)
        .await?;
    assert!(!before.get("audit").unwrap().as_array().unwrap().is_empty());
    db.apply_migration(57).await?;
    let after: serde_json::Value = sqlx::query_scalar(snapshot)
        .bind(&removed[..])
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(after, before);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(columns_query)
            .bind(&removed[..])
            .fetch_one(&db.pool)
            .await?,
        0
    );

    for version in 58..=61 {
        db.apply_migration(version).await?;
    }
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let hash = crypto.browser_session_hash(&local_id.to_string(), "existing-session")?;
    let caller = repo
        .find_live_caller_by_token(local_id, &hash, &crypto)
        .await?
        .unwrap();
    assert_eq!(caller.account.id, user);
    let issuer: Option<String> =
        sqlx::query_scalar("SELECT oidc_issuer FROM browser_sessions WHERE id=$1")
            .bind(local_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(issuer.as_deref(), Some("https://auth.example.test"));
    assert_eq!(caller.idle_expires_at, now + Duration::hours(1));
    let legacy_hash = crypto.browser_session_hash(&legacy_id.to_string(), "existing-session")?;
    let legacy = repo.find_by_token(legacy_id, &legacy_hash).await?.unwrap();
    assert_eq!(legacy.user_id, user);
    let issuer: Option<String> =
        sqlx::query_scalar("SELECT oidc_issuer FROM browser_sessions WHERE id=$1")
            .bind(legacy_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(issuer, None);

    db.apply_migration(63).await?;
    let oauth = OAuthRepo::new(db.pool.clone());
    assert_eq!(
        oauth.access_user(&access, resource, "cli", now).await?,
        Some(user)
    );
    let issued = oauth
        .rotate_refresh(
            &refresh,
            "notegate-cli",
            resource,
            None,
            &CredentialIssue {
                client_environment: None,
                access_hash: "new-access-hash",
                refresh_hash: Some("new-refresh-hash"),
                key_id: crypto.lookup_key_id(),
                version: crypto.version(),
                access_expires_at: now + Duration::minutes(10),
                grant_expires_at: now + Duration::days(7),
                idle_expires_at: now + Duration::days(7),
                private_refresh_response: None,
            },
            now,
        )
        .await?;
    assert!(matches!(issued, ExchangeOutcome::Issued(g) if g.id == grant && g.user_id == user));
    db.cleanup().await;
    Ok(())
}

#[test]
fn browser_session_token_round_trips() {
    let session_id = Uuid::new_v4();
    let token = format_token(session_id, "secret");

    assert_eq!(parse_token(&token), Some((session_id, "secret")));
    assert!(parse_token("ngs_v1_not-a-uuid_secret").is_none());
    assert!(parse_token("ngk_v2_wrong").is_none());
}
