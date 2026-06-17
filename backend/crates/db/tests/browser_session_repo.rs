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
    InsertBrowserSession, RotatedRefreshToken, format_token, parse_token, token_prefix,
};
use uuid::Uuid;

async fn insert_session(
    repo: &BrowserSessionRepo,
    crypto: &PiiCrypto,
    user_id: Uuid,
    refresh_token: &str,
) -> Result<(Uuid, String), Box<dyn std::error::Error>> {
    let session_id = Uuid::new_v4();
    let secret = "session-secret";
    let token_hash = crypto.browser_session_hash(&session_id.to_string(), secret)?;
    let encrypted = crypto.encrypt_browser_refresh_token(&session_id.to_string(), refresh_token)?;
    let prefix = token_prefix(session_id);
    repo.insert_session(InsertBrowserSession {
        session_id,
        user_id,
        token_prefix: &prefix,
        token_hash: &token_hash,
        refresh_token: &encrypted,
        refresh_token_enc_key_id: crypto.enc_key_id(),
        refresh_token_enc_version: crypto.version(),
        validated_until: Utc::now() + Duration::hours(1),
        expires_at: Utc::now() + Duration::days(15),
    })
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
    let (session_id, token_hash) = insert_session(&repo, &crypto, user_id, "refresh-1").await?;

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
        "refresh-1"
    );
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
    let (revoked_id, revoked_hash) = insert_session(&repo, &crypto, user_id, "refresh-1").await?;
    repo.revoke_session(revoked_id, "test").await?;
    assert!(
        repo.find_live_by_token(revoked_id, &revoked_hash)
            .await?
            .is_none()
    );

    let (inactive_id, inactive_hash) = insert_session(&repo, &crypto, user_id, "refresh-2").await?;
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
async fn refresh_rotation_updates_encrypted_token() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user_id = insert_user_account(&db.pool, "rotate-user", "rotate@example.test").await?;
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let crypto = PiiCrypto::test();
    let (session_id, token_hash) = insert_session(&repo, &crypto, user_id, "refresh-old").await?;
    sqlx::query(
        "UPDATE browser_sessions SET validated_until = now() - interval '1 second' WHERE id = $1",
    )
    .bind(session_id)
    .execute(&db.pool)
    .await?;
    let refresh_claim_id = Uuid::new_v4();
    assert!(
        repo.claim_refresh(session_id, &token_hash, refresh_claim_id)
            .await?
            .is_some()
    );
    let rotated = crypto.encrypt_browser_refresh_token(&session_id.to_string(), "refresh-new")?;
    assert!(
        repo.mark_refreshed(
            session_id,
            refresh_claim_id,
            Utc::now() + Duration::hours(1),
            Some(RotatedRefreshToken {
                refresh_token: &rotated,
                refresh_token_enc_key_id: crypto.enc_key_id(),
                refresh_token_enc_version: crypto.version(),
            }),
        )
        .await?
    );

    let found = repo
        .find_live_by_token(session_id, &token_hash)
        .await?
        .expect("session should still be live");
    assert_eq!(
        crypto.decrypt_browser_refresh_token(
            &found.id.to_string(),
            &found.refresh_token_enc_key_id,
            found.refresh_token_enc_version,
            &found.refresh_token,
        )?,
        "refresh-new"
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn transient_refresh_stores_rotated_token_without_extending_validation()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user_id = insert_user_account(&db.pool, "transient-user", "transient@example.test").await?;
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let crypto = PiiCrypto::test();
    let (session_id, token_hash) = insert_session(&repo, &crypto, user_id, "refresh-old").await?;
    sqlx::query(
        "UPDATE browser_sessions SET validated_until = now() - interval '1 second' WHERE id = $1",
    )
    .bind(session_id)
    .execute(&db.pool)
    .await?;

    let refresh_claim_id = Uuid::new_v4();
    let claimed = repo
        .claim_refresh(session_id, &token_hash, refresh_claim_id)
        .await?
        .expect("expired session should be claimed");
    let validated_until = claimed.validated_until;
    let rotated = crypto.encrypt_browser_refresh_token(&session_id.to_string(), "refresh-new")?;
    assert!(
        repo.store_rotated_refresh_token_and_clear_claim(
            session_id,
            refresh_claim_id,
            RotatedRefreshToken {
                refresh_token: &rotated,
                refresh_token_enc_key_id: crypto.enc_key_id(),
                refresh_token_enc_version: crypto.version(),
            },
        )
        .await?
    );

    let found = repo
        .find_live_by_token(session_id, &token_hash)
        .await?
        .expect("session should still be live");
    assert_eq!(found.validated_until, validated_until);
    assert_eq!(
        crypto.decrypt_browser_refresh_token(
            &found.id.to_string(),
            &found.refresh_token_enc_key_id,
            found.refresh_token_enc_version,
            &found.refresh_token,
        )?,
        "refresh-new"
    );
    let claim_cleared: bool = sqlx::query_scalar(
        "SELECT refresh_claim_id IS NULL AND refresh_started_at IS NULL FROM browser_sessions WHERE id = $1",
    )
    .bind(session_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(claim_cleared);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn refresh_claim_serializes_refresh_attempts() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user_id = insert_user_account(&db.pool, "claim-user", "claim@example.test").await?;
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let crypto = PiiCrypto::test();
    let (session_id, token_hash) = insert_session(&repo, &crypto, user_id, "refresh-1").await?;
    sqlx::query(
        "UPDATE browser_sessions SET validated_until = now() - interval '1 second' WHERE id = $1",
    )
    .bind(session_id)
    .execute(&db.pool)
    .await?;

    let first_claim_id = Uuid::new_v4();
    let second_claim_id = Uuid::new_v4();
    assert!(
        repo.claim_refresh(session_id, &token_hash, first_claim_id)
            .await?
            .is_some()
    );
    assert!(
        repo.claim_refresh(session_id, &token_hash, second_claim_id)
            .await?
            .is_none(),
        "a fresh refresh claim must block duplicate refresh attempts"
    );
    assert!(
        repo.clear_refresh_claim(session_id, first_claim_id).await?,
        "transient refresh failures release the claim"
    );
    assert!(
        repo.claim_refresh(session_id, &token_hash, second_claim_id)
            .await?
            .is_some(),
        "clearing a claim allows the next request to retry refresh"
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
    assert!(parse_token("ngk_v1_wrong").is_none());
}
