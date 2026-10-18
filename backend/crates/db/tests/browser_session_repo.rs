#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use chrono::{Duration, Utc};
use common::{TestDb, deactivate_account, insert_user_account};
use notegate_core::security::PiiCrypto;
use notegate_db::BrowserSessionRepo;
use notegate_db::browser_session_repo::{
    InsertBrowserSession, format_token, parse_token, token_prefix,
};
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
    let encrypted = crypto.encrypt_browser_refresh_token(&session_id.to_string(), "")?;
    let prefix = token_prefix(session_id);
    repo.insert_local_session(
        InsertBrowserSession {
            session_id,
            user_id,
            token_prefix: &prefix,
            token_hash: &token_hash,
            refresh_token: &encrypted,
            refresh_token_enc_key_id: crypto.enc_key_id(),
            refresh_token_enc_version: crypto.version(),
            validated_until: Utc::now() + Duration::hours(1),
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
    assert_eq!(
        crypto.decrypt_browser_refresh_token(
            &found.id.to_string(),
            &found.refresh_token_enc_key_id,
            found.refresh_token_enc_version,
            &found.refresh_token,
        )?,
        ""
    );
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

#[test]
fn browser_session_token_round_trips() {
    let session_id = Uuid::new_v4();
    let token = format_token(session_id, "secret");

    assert_eq!(parse_token(&token), Some((session_id, "secret")));
    assert!(parse_token("ngs_v1_not-a-uuid_secret").is_none());
    assert!(parse_token("ngk_v2_wrong").is_none());
}
