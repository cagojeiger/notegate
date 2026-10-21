//! Real-Postgres OAuth state-machine tests. Every boundary uses an injected clock.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod common;

use chrono::{DateTime, Duration, Utc};
use common::{TestDb, insert_user_account};
use notegate_core::security::PiiCrypto;
use notegate_db::oauth_repo::{Authorization, OAuthRepo};
use notegate_service::oauth::{OAuthService, TokenOutcome, TokenResponse, pkce_challenge};
use uuid::Uuid;

const CLIENT: &str = "https://client.example.test/metadata";
const RESOURCE: &str = "https://notes.example.test/mcp";
const REDIRECT: &str = "https://client.example.test/callback";
const VERIFIER: &str = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGH";

fn service(db: &TestDb) -> OAuthService {
    OAuthService::new(OAuthRepo::new(db.pool.clone()), PiiCrypto::test())
}
fn now() -> DateTime<Utc> {
    DateTime::from_timestamp(1_800_000_000, 0).unwrap()
}
fn issued(outcome: TokenOutcome) -> TokenResponse {
    match outcome {
        TokenOutcome::Issued(response) => response,
        TokenOutcome::Rejected(e) => panic!("unexpected OAuth rejection: {e}"),
    }
}
fn rejected(outcome: TokenOutcome, expected: &str) {
    match outcome {
        TokenOutcome::Rejected(e) => assert_eq!(e, expected),
        TokenOutcome::Issued(_) => panic!("unexpected credential issuance"),
    }
}

async fn approved(
    db: &TestDb,
    user: Uuid,
    code: &str,
    scope: &str,
    time: DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let crypto = PiiCrypto::test();
    let repo = OAuthRepo::new(db.pool.clone());
    let auth = Authorization {
        id: Uuid::new_v4(),
        client_id: CLIENT.to_owned(),
        client_name: "Test client".to_owned(),
        resource: RESOURCE.to_owned(),
        redirect_uri: REDIRECT.to_owned(),
        scope: scope.to_owned(),
        code_challenge: pkce_challenge(VERIFIER),
        cookie_hash: "bound-browser".to_owned(),
        private_payload: serde_json::json!({}),
        user_id: None,
        code_hash: None,
        expires_at: time + Duration::minutes(5),
        consumed_at: None,
    };
    repo.insert_authorization(&auth).await?;
    assert!(
        repo.find_authorization(auth.id, "other-browser", time)
            .await?
            .is_none()
    );
    assert!(
        !repo
            .approve_authorization(
                auth.id,
                "other-browser",
                user,
                "wrong",
                time,
                time + Duration::seconds(60)
            )
            .await?
    );
    assert!(
        repo.approve_authorization(
            auth.id,
            "bound-browser",
            user,
            &crypto.oauth_hash("code", code)?,
            time,
            time + Duration::seconds(60)
        )
        .await?
    );
    assert!(
        !repo
            .approve_authorization(
                auth.id,
                "bound-browser",
                user,
                "second",
                time,
                time + Duration::seconds(60)
            )
            .await?
    );
    Ok(())
}
async fn exchange(
    svc: &OAuthService,
    code: &str,
    time: DateTime<Utc>,
) -> notegate_core::Result<TokenOutcome> {
    svc.exchange_code(code, CLIENT, RESOURCE, REDIRECT, VERIFIER, time, None)
        .await
}

#[tokio::test]
async fn connection_management_preserves_environment_and_bulk_revoke_wins_over_refresh()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "connection-management", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "managed", "mcp offline_access", t).await?;
    let tokens = issued(
        svc.exchange_code(
            "managed",
            CLIENT,
            RESOURCE,
            REDIRECT,
            VERIFIER,
            t,
            Some("Firefox on Linux"),
        )
        .await?,
    );
    let grant = svc.store.list_grants(user, t).await?.remove(0);
    assert_eq!(
        grant.client_environment.as_deref(),
        Some("Firefox on Linux")
    );
    assert_eq!(grant.last_token_at, Some(t));
    let metadata: serde_json::Value = sqlx::query_scalar(
        "SELECT metadata FROM audit_events WHERE resource_id=$1 AND op_type='oauth.approve'",
    )
    .bind(grant.id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(metadata["client_environment"], "Firefox on Linux");
    let refreshed = issued(
        svc.refresh(
            tokens.refresh_token.as_deref().unwrap(),
            CLIENT,
            RESOURCE,
            t + Duration::seconds(1),
        )
        .await?,
    );
    let grant = svc.store.list_grants(user, t).await?.remove(0);
    assert_eq!(
        grant.client_environment.as_deref(),
        Some("Firefox on Linux")
    );
    assert_eq!(grant.last_token_at, Some(t + Duration::seconds(1)));
    let refresh = svc.refresh(
        refreshed.refresh_token.as_deref().unwrap(),
        CLIENT,
        RESOURCE,
        t + Duration::seconds(2),
    );
    let revoke = svc
        .store
        .revoke_owned_grants(user, "mcp", t + Duration::seconds(2));
    let (refresh, revoked) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(refresh, revoke)
    })
    .await?;
    revoked?;
    assert_eq!(
        svc.access_user(
            &tokens.access_token,
            RESOURCE,
            "mcp",
            t + Duration::seconds(3)
        )
        .await?,
        None
    );
    if let TokenOutcome::Issued(tokens) = refresh? {
        assert_eq!(
            svc.access_user(
                &tokens.access_token,
                RESOURCE,
                "mcp",
                t + Duration::seconds(3)
            )
            .await?,
            None
        );
        rejected(
            svc.refresh(
                tokens.refresh_token.as_deref().unwrap(),
                CLIENT,
                RESOURCE,
                t + Duration::seconds(3),
            )
            .await?,
            "invalid_grant",
        );
    }
    svc.store
        .revoke_owned_grants(user, "mcp", t + Duration::seconds(3))
        .await?;
    let revokes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE resource_id=$1 AND op_type='oauth.revoke'",
    )
    .bind(grant.id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(revokes, 1);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn bulk_oauth_revocation_preserves_all_connections_when_audit_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "bulk-rollback", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "first", "mcp offline_access", t).await?;
    let first = issued(exchange(&svc, "first", t).await?);
    approved(&db, user, "second", "mcp offline_access", t).await?;
    let second = issued(exchange(&svc, "second", t).await?);
    sqlx::query("ALTER TABLE audit_events ADD CONSTRAINT test_oauth_revoke_audit CHECK(op_type<>'oauth.revoke') NOT VALID").execute(&db.pool).await?;
    assert!(svc.store.revoke_owned_grants(user, "mcp", t).await.is_err());
    for tokens in [&first, &second] {
        assert_eq!(
            svc.access_user(&tokens.access_token, RESOURCE, "mcp", t)
                .await?,
            Some(user)
        );
    }
    assert_eq!(svc.store.list_grants(user, t).await?.len(), 2);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn authorization_code_is_browser_pkce_client_redirect_and_resource_bound()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "oauth", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "code", "mcp offline_access", t).await?;
    for (client, resource, redirect, verifier) in [
        ("other", RESOURCE, REDIRECT, VERIFIER),
        (CLIENT, "https://other.test/mcp", REDIRECT, VERIFIER),
        (CLIENT, RESOURCE, "https://evil.test/callback", VERIFIER),
        (CLIENT, RESOURCE, REDIRECT, "wrong"),
        (
            CLIENT,
            RESOURCE,
            REDIRECT,
            "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH",
        ),
    ] {
        rejected(
            svc.exchange_code("code", client, resource, redirect, verifier, t, None)
                .await?,
            "invalid_grant",
        );
    }
    let tokens = issued(exchange(&svc, "code", t).await?);
    assert_eq!(tokens.scope, "mcp offline_access");
    assert!(tokens.refresh_token.is_some());
    assert_eq!(
        svc.access_user(&tokens.access_token, RESOURCE, "mcp", t)
            .await?,
        Some(user)
    );
    rejected(exchange(&svc, "code", t).await?, "invalid_grant");
    assert_eq!(
        svc.access_user(&tokens.access_token, RESOURCE, "mcp", t)
            .await?,
        None
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM oauth_grants")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(count, 1);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn code_and_access_expire_at_exact_boundaries_without_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "expiry", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "expired", "mcp", t).await?;
    rejected(
        exchange(&svc, "expired", t + Duration::seconds(60)).await?,
        "invalid_grant",
    );
    approved(&db, user, "live", "mcp", t).await?;
    let tokens = issued(exchange(&svc, "live", t + Duration::seconds(59)).await?);
    assert!(tokens.refresh_token.is_none());
    let start = t + Duration::seconds(59);
    assert_eq!(
        svc.access_user(
            &tokens.access_token,
            RESOURCE,
            "mcp",
            start + Duration::seconds(599)
        )
        .await?,
        Some(user)
    );
    assert_eq!(
        svc.access_user(
            &tokens.access_token,
            RESOURCE,
            "mcp",
            start + Duration::seconds(600)
        )
        .await?,
        None
    );
    assert_eq!(
        svc.access_user(
            &tokens.access_token,
            "https://notes.example.test/cli",
            "cli",
            start
        )
        .await?,
        None
    );
    assert_eq!(
        svc.access_user(&tokens.access_token, RESOURCE, "cli", start)
            .await?,
        None
    );
    sqlx::query("UPDATE accounts SET is_active=false WHERE id=$1")
        .bind(user)
        .execute(&db.pool)
        .await?;
    assert_eq!(
        svc.access_user(&tokens.access_token, RESOURCE, "mcp", start)
            .await?,
        None
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn refresh_rotation_commits_family_revocation_on_reuse()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "refresh", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "code", "mcp offline_access", t).await?;
    let first = issued(exchange(&svc, "code", t).await?);
    let refresh = first.refresh_token.unwrap();
    rejected(
        svc.refresh(&refresh, "other", RESOURCE, t).await?,
        "invalid_grant",
    );
    rejected(
        svc.refresh(&refresh, CLIENT, "https://other.test/mcp", t)
            .await?,
        "invalid_grant",
    );
    let second = issued(
        svc.refresh(&refresh, CLIENT, RESOURCE, t + Duration::seconds(1))
            .await?,
    );
    assert_eq!(
        svc.access_user(
            &second.access_token,
            RESOURCE,
            "mcp",
            t + Duration::seconds(1)
        )
        .await?,
        Some(user)
    );
    rejected(
        svc.refresh(&refresh, CLIENT, RESOURCE, t + Duration::seconds(11))
            .await?,
        "invalid_grant",
    );
    assert_eq!(
        svc.access_user(
            &second.access_token,
            RESOURCE,
            "mcp",
            t + Duration::seconds(11)
        )
        .await?,
        None
    );
    assert_eq!(
        svc.access_user(
            &first.access_token,
            RESOURCE,
            "mcp",
            t + Duration::seconds(11)
        )
        .await?,
        None
    );
    rejected(
        svc.refresh(
            &second.refresh_token.unwrap(),
            CLIENT,
            RESOURCE,
            t + Duration::seconds(11),
        )
        .await?,
        "invalid_grant",
    );
    let events:i64=sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE op_type='oauth.revoke' AND metadata->>'reason'='refresh_reuse'").fetch_one(&db.pool).await?;
    assert_eq!(events, 1);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn refresh_retries_and_successor_requests_share_one_bounded_encrypted_response()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "refresh-retry", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "retry", "mcp offline_access", t).await?;
    let initial = issued(exchange(&svc, "retry", t).await?);
    let previous = initial.refresh_token.ok_or("missing refresh token")?;
    let rotated = issued(
        svc.refresh(&previous, CLIENT, RESOURCE, t + Duration::seconds(1))
            .await?,
    );
    let current = rotated
        .refresh_token
        .as_deref()
        .ok_or("missing successor")?;
    let grant = svc.store.list_grants(user, t).await?.remove(0);
    let snapshot = "SELECT jsonb_agg(to_jsonb(c) ORDER BY token_hash) FROM oauth_credentials c";
    let before: serde_json::Value = sqlx::query_scalar(snapshot).fetch_one(&db.pool).await?;
    let blob: serde_json::Value = sqlx::query_scalar(
        "SELECT private_refresh_response FROM oauth_credentials WHERE token_hash=$1",
    )
    .bind(PiiCrypto::test().oauth_hash("token", &previous)?)
    .fetch_one(&db.pool)
    .await?;
    let serialized = blob.to_string();
    for secret in [previous.as_str(), rotated.access_token.as_str(), current] {
        assert!(!serialized.contains(secret));
    }
    for (offset, token) in [
        (Duration::milliseconds(36), previous.as_str()),
        (Duration::milliseconds(150), previous.as_str()),
        (Duration::seconds(3), current),
        (Duration::seconds(9), previous.as_str()),
        (Duration::seconds(9), current),
    ] {
        let time = t + Duration::seconds(1) + offset;
        let retry = issued(svc.refresh(token, CLIENT, RESOURCE, time).await?);
        assert!(retry.access_token == rotated.access_token);
        assert!(retry.refresh_token == rotated.refresh_token);
        assert_eq!(
            retry.expires_in,
            (t + Duration::seconds(601) - time).num_seconds()
        );
        assert_eq!(
            svc.access_user(&retry.access_token, RESOURCE, "mcp", time)
                .await?,
            Some(user)
        );
    }
    let after: serde_json::Value = sqlx::query_scalar(snapshot).fetch_one(&db.pool).await?;
    assert!(before == after);
    let retained = svc.store.list_grants(user, t).await?.remove(0);
    assert_eq!(retained.id, grant.id);
    assert_eq!(retained.last_token_at, grant.last_token_at);
    assert_eq!(retained.idle_expires_at, grant.idle_expires_at);
    assert_eq!(retained.expires_at, grant.expires_at);
    let next = issued(
        svc.refresh(current, CLIENT, RESOURCE, t + Duration::seconds(11))
            .await?,
    );
    assert!(next.access_token != rotated.access_token);
    assert!(next.refresh_token != rotated.refresh_token);
    assert_eq!(
        svc.store.list_grants(user, t).await?.remove(0).expires_at,
        grant.expires_at
    );
    let revoked: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE op_type='oauth.revoke'")
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(revoked, 0);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn refresh_retry_still_enforces_client_resource_scope_revocation_and_expiry()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "retry-bindings", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "bindings", "mcp offline_access", t).await?;
    let initial = issued(exchange(&svc, "bindings", t).await?);
    let previous = initial.refresh_token.ok_or("missing refresh token")?;
    let rotated = issued(
        svc.refresh(&previous, CLIENT, RESOURCE, t + Duration::seconds(1))
            .await?,
    );
    for token in [
        previous.as_str(),
        rotated
            .refresh_token
            .as_deref()
            .ok_or("missing successor")?,
    ] {
        rejected(
            svc.refresh(token, "other-client", RESOURCE, t + Duration::seconds(2))
                .await?,
            "invalid_grant",
        );
        rejected(
            svc.refresh(
                token,
                CLIENT,
                "https://other.test/mcp",
                t + Duration::seconds(2),
            )
            .await?,
            "invalid_grant",
        );
        rejected(
            svc.refresh_with_scope(
                token,
                CLIENT,
                RESOURCE,
                Some("cli offline_access"),
                t + Duration::seconds(2),
            )
            .await?,
            "invalid_scope",
        );
    }
    let valid = issued(
        svc.refresh(&previous, CLIENT, RESOURCE, t + Duration::seconds(2))
            .await?,
    );
    assert!(valid.access_token == rotated.access_token);
    sqlx::query("UPDATE accounts SET is_active=false WHERE id=$1")
        .bind(user)
        .execute(&db.pool)
        .await?;
    rejected(
        svc.refresh(&previous, CLIENT, RESOURCE, t + Duration::seconds(3))
            .await?,
        "invalid_grant",
    );
    sqlx::query("UPDATE accounts SET is_active=true WHERE id=$1")
        .bind(user)
        .execute(&db.pool)
        .await?;
    let grant = svc.store.list_grants(user, t).await?.remove(0);
    sqlx::query("UPDATE oauth_grants SET idle_expires_at=$2 WHERE id=$1")
        .bind(grant.id)
        .bind(t + Duration::seconds(4))
        .execute(&db.pool)
        .await?;
    rejected(
        svc.refresh(&previous, CLIENT, RESOURCE, t + Duration::seconds(4))
            .await?,
        "invalid_grant",
    );
    sqlx::query("UPDATE oauth_grants SET idle_expires_at=$2,expires_at=$3 WHERE id=$1")
        .bind(grant.id)
        .bind(t + Duration::days(7))
        .bind(t + Duration::seconds(5))
        .execute(&db.pool)
        .await?;
    rejected(
        svc.refresh(&previous, CLIENT, RESOURCE, t + Duration::seconds(5))
            .await?,
        "invalid_grant",
    );
    sqlx::query("UPDATE oauth_grants SET expires_at=$2 WHERE id=$1")
        .bind(grant.id)
        .bind(grant.expires_at)
        .execute(&db.pool)
        .await?;
    svc.store
        .revoke_owned_grant(grant.id, user, t + Duration::seconds(6))
        .await?;
    rejected(
        svc.refresh(&previous, CLIENT, RESOURCE, t + Duration::seconds(7))
            .await?,
        "invalid_grant",
    );
    let cached: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM oauth_credentials WHERE private_refresh_response IS NOT NULL",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(cached, 0);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn encrypted_refresh_retry_response_cannot_be_swapped_between_credentials()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "retry-aad", "user@test").await?;
    let svc = service(&db);
    let t = now();
    let mut originals = Vec::new();
    let mut rotated = Vec::new();
    for code in ["first", "second"] {
        approved(&db, user, code, "mcp offline_access", t).await?;
        let previous = issued(exchange(&svc, code, t).await?)
            .refresh_token
            .ok_or("missing refresh token")?;
        rotated.push(issued(
            svc.refresh(&previous, CLIENT, RESOURCE, t + Duration::seconds(1))
                .await?,
        ));
        originals.push(previous);
    }
    let first_hash = PiiCrypto::test().oauth_hash("token", &originals[0])?;
    let second_hash = PiiCrypto::test().oauth_hash("token", &originals[1])?;
    sqlx::query("UPDATE oauth_credentials SET private_refresh_response=(SELECT private_refresh_response FROM oauth_credentials WHERE token_hash=$1) WHERE token_hash=$2")
        .bind(first_hash).bind(second_hash).execute(&db.pool).await?;
    assert!(
        svc.refresh(&originals[1], CLIENT, RESOURCE, t + Duration::seconds(2))
            .await
            .is_err()
    );
    for response in rotated {
        assert_eq!(
            svc.access_user(
                &response.access_token,
                RESOURCE,
                "mcp",
                t + Duration::seconds(2)
            )
            .await?,
            Some(user)
        );
    }
    assert_eq!(svc.store.list_grants(user, t).await?.len(), 2);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn refresh_retry_schema_preserves_existing_grants_and_credentials()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup_before(63).await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "retry-upgrade", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "upgrade", "mcp offline_access", t).await?;
    let initial = issued(exchange(&svc, "upgrade", t).await?);
    let query = "SELECT jsonb_build_object('accounts',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM accounts t),'users',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM users t),'grants',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM oauth_grants t),'credentials',(SELECT jsonb_agg(to_jsonb(t)-'private_refresh_response' ORDER BY token_hash) FROM oauth_credentials t))";
    let before: serde_json::Value = sqlx::query_scalar(query).fetch_one(&db.pool).await?;
    db.apply_migration(63).await?;
    notegate_db::check_readiness(&db.pool).await?;
    let after: serde_json::Value = sqlx::query_scalar(query).fetch_one(&db.pool).await?;
    assert!(before == after);
    assert_eq!(
        svc.access_user(
            &initial.access_token,
            RESOURCE,
            "mcp",
            t + Duration::seconds(1)
        )
        .await?,
        Some(user)
    );
    let previous = initial.refresh_token.ok_or("missing refresh token")?;
    let rotated = issued(
        svc.refresh(&previous, CLIENT, RESOURCE, t + Duration::seconds(1))
            .await?,
    );
    assert_eq!(
        svc.access_user(
            &rotated.access_token,
            RESOURCE,
            "mcp",
            t + Duration::seconds(2)
        )
        .await?,
        Some(user)
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn refresh_idle_and_absolute_expiry_are_independent() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "lifetime", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "idle", "mcp offline_access", t).await?;
    let idle = issued(exchange(&svc, "idle", t).await?)
        .refresh_token
        .unwrap();
    rejected(
        svc.refresh(&idle, CLIENT, RESOURCE, t + Duration::days(7))
            .await?,
        "invalid_grant",
    );
    approved(&db, user, "absolute", "mcp offline_access", t).await?;
    let mut refresh = issued(exchange(&svc, "absolute", t).await?)
        .refresh_token
        .unwrap();
    for day in [6, 12, 18, 24, 29] {
        refresh = issued(
            svc.refresh(&refresh, CLIENT, RESOURCE, t + Duration::days(day))
                .await?,
        )
        .refresh_token
        .unwrap();
    }
    let near = issued(
        svc.refresh(
            &refresh,
            CLIENT,
            RESOURCE,
            t + Duration::days(30) - Duration::seconds(1),
        )
        .await?,
    );
    assert_eq!(near.expires_in, 1);
    rejected(
        svc.refresh(
            &near.refresh_token.unwrap(),
            CLIENT,
            RESOURCE,
            t + Duration::days(30),
        )
        .await?,
        "invalid_grant",
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn device_pending_slowdown_approval_and_single_use() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "device", "user@test").await?;
    let svc = service(&db);
    let t = now();
    let resource = "https://notes.example.test/cli";
    let device = svc
        .device(
            "cli",
            resource,
            "cli offline_access",
            "https://notes.example.test",
            t,
        )
        .await?;
    assert_eq!(device.interval, 5);
    rejected(
        svc.poll_device(&device.device_code, "wrong", resource, t, None)
            .await?,
        "invalid_grant",
    );
    rejected(
        svc.poll_device(&device.device_code, "cli", resource, t, None)
            .await?,
        "authorization_pending",
    );
    rejected(
        svc.poll_device(
            &device.device_code,
            "cli",
            resource,
            t + Duration::seconds(1),
            None,
        )
        .await?,
        "slow_down",
    );
    assert!(
        svc.approve_device(
            &device.user_code.to_lowercase().replace('-', ""),
            user,
            true,
            t + Duration::seconds(2)
        )
        .await?
    );
    assert!(
        !svc.approve_device(&device.user_code, user, true, t + Duration::seconds(2))
            .await?
    );
    let tokens = issued(
        svc.poll_device(
            &device.device_code,
            "cli",
            resource,
            t + Duration::seconds(11),
            None,
        )
        .await?,
    );
    assert_eq!(
        svc.access_user(
            &tokens.access_token,
            resource,
            "cli",
            t + Duration::seconds(11)
        )
        .await?,
        Some(user)
    );
    rejected(
        svc.poll_device(
            &device.device_code,
            "cli",
            resource,
            t + Duration::seconds(30),
            None,
        )
        .await?,
        "invalid_grant",
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn device_denial_and_expiry_never_issue_tokens() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "denied", "user@test").await?;
    let svc = service(&db);
    let t = now();
    let denied = svc
        .device(
            "cli",
            RESOURCE,
            "cli offline_access",
            "https://notes.example.test",
            t,
        )
        .await?;
    assert!(
        svc.approve_device(&denied.user_code, user, false, t)
            .await?
    );
    rejected(
        svc.poll_device(&denied.device_code, "cli", RESOURCE, t, None)
            .await?,
        "access_denied",
    );
    let expired = svc
        .device(
            "cli",
            RESOURCE,
            "cli offline_access",
            "https://notes.example.test",
            t,
        )
        .await?;
    assert!(
        !svc.approve_device(&expired.user_code, user, true, t + Duration::seconds(300))
            .await?
    );
    rejected(
        svc.poll_device(
            &expired.device_code,
            "cli",
            RESOURCE,
            t + Duration::seconds(300),
            None,
        )
        .await?,
        "expired_token",
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_code_and_refresh_requests_issue_once() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "concurrent", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "code", "mcp offline_access", t).await?;
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let svc = svc.clone();
        tasks.push(tokio::spawn(async move { exchange(&svc, "code", t).await }));
    }
    let mut results = Vec::new();
    for task in tasks {
        if let TokenOutcome::Issued(tokens) = task.await?? {
            results.push(tokens);
        }
    }
    assert_eq!(results.len(), 1);
    approved(&db, user, "refresh-code", "mcp offline_access", t).await?;
    let refresh = issued(exchange(&svc, "refresh-code", t).await?)
        .refresh_token
        .unwrap();
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let svc = svc.clone();
        let refresh = refresh.clone();
        tasks.push(tokio::spawn(async move {
            svc.refresh(&refresh, CLIENT, RESOURCE, t + Duration::seconds(1))
                .await
        }));
    }
    let mut responses = Vec::new();
    for task in tasks {
        responses.push(issued(task.await??));
    }
    assert_eq!(responses.len(), 8);
    let winner = responses.first().ok_or("missing refresh response")?;
    for response in &responses {
        assert!(response.access_token == winner.access_token);
        assert!(response.refresh_token == winner.refresh_token);
        assert_eq!(
            svc.access_user(
                &response.access_token,
                RESOURCE,
                "mcp",
                t + Duration::seconds(2)
            )
            .await?,
            Some(user)
        );
    }
    let grant = svc.store.list_grants(user, t).await?.remove(0);
    let credential_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM oauth_credentials WHERE grant_id=$1")
            .bind(grant.id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(credential_count, 4);
    let grants: i64 = sqlx::query_scalar("SELECT count(*) FROM oauth_grants")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(grants, 2);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn issuance_failure_rolls_back_code_consumption_and_grant()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "rollback", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "code", "mcp offline_access", t).await?;
    sqlx::raw_sql("CREATE FUNCTION reject_oauth() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected credential failure'; END $$; CREATE TRIGGER reject_oauth BEFORE INSERT ON oauth_credentials FOR EACH ROW EXECUTE FUNCTION reject_oauth();").execute(&db.pool).await?;
    assert!(exchange(&svc, "code", t).await.is_err());
    let grants: i64 = sqlx::query_scalar("SELECT count(*) FROM oauth_grants")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(grants, 0);
    sqlx::query("DROP TRIGGER reject_oauth ON oauth_credentials")
        .execute(&db.pool)
        .await?;
    issued(exchange(&svc, "code", t).await?);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn revoke_and_account_deletion_immediately_disable_existing_tokens()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "revoked", "user@test").await?;
    let svc = service(&db);
    let t = Utc::now();
    approved(&db, user, "first", "mcp offline_access", t).await?;
    let tokens = issued(exchange(&svc, "first", t).await?);
    svc.revoke(&tokens.access_token, "other", t).await?;
    assert_eq!(
        svc.access_user(&tokens.access_token, RESOURCE, "mcp", t)
            .await?,
        Some(user)
    );
    svc.revoke(&tokens.refresh_token.unwrap(), CLIENT, t)
        .await?;
    assert_eq!(
        svc.access_user(&tokens.access_token, RESOURCE, "mcp", t)
            .await?,
        None
    );
    approved(&db, user, "second", "mcp offline_access", t).await?;
    let tokens = issued(exchange(&svc, "second", t).await?);
    notegate_db::AccountRepo::new(db.pool.clone())
        .soft_delete_user(user, user)
        .await?;
    assert_eq!(
        svc.access_user(&tokens.access_token, RESOURCE, "mcp", t)
            .await?,
        None
    );
    rejected(
        svc.refresh(&tokens.refresh_token.unwrap(), CLIENT, RESOURCE, t)
            .await?,
        "invalid_grant",
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn refresh_accepts_original_scope_but_never_expands_it()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "scope", "user@test").await?;
    let svc = service(&db);
    let t = now();
    approved(&db, user, "code", "mcp offline_access", t).await?;
    let token = issued(exchange(&svc, "code", t).await?)
        .refresh_token
        .unwrap();
    rejected(
        svc.refresh_with_scope(&token, CLIENT, RESOURCE, Some("cli offline_access"), t)
            .await?,
        "invalid_scope",
    );
    issued(
        svc.refresh_with_scope(&token, CLIENT, RESOURCE, Some("mcp offline_access"), t)
            .await?,
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_approvals_respect_the_connection_limit()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "limit", "user@test").await?;
    let svc = service(&db);
    let t = now();
    sqlx::query("INSERT INTO oauth_grants (id,user_id,client_id,client_name,resource,scope,created_at,expires_at,idle_expires_at) SELECT gen_random_uuid(),$1,'seed','Seed',$2,'mcp offline_access',$3,$3::timestamptz+interval '30 days',$3::timestamptz+interval '7 days' FROM generate_series(1,99)")
        .bind(user).bind(RESOURCE).bind(t).execute(&db.pool).await?;
    approved(&db, user, "one", "mcp offline_access", t).await?;
    approved(&db, user, "two", "mcp offline_access", t).await?;
    let one = svc.clone();
    let two = svc.clone();
    let (one, two) = tokio::join!(exchange(&one, "one", t), exchange(&two, "two", t));
    let mut success = 0;
    for outcome in [one?, two?] {
        match outcome {
            TokenOutcome::Issued(_) => success += 1,
            TokenOutcome::Rejected(e) => assert_eq!(e, "access_denied"),
        }
    }
    assert_eq!(success, 1);
    assert_eq!(svc.store.list_grants(user, t).await?.len(), 100);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn cleanup_removes_expired_access_but_preserves_refresh_reuse_detection()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "cleanup", "user@test").await?;
    let svc = service(&db);
    let t = Utc::now() - Duration::minutes(11);
    approved(&db, user, "code", "mcp offline_access", t).await?;
    let tokens = issued(exchange(&svc, "code", t).await?);
    let refresh = tokens.refresh_token.unwrap();
    let current = issued(
        svc.refresh(
            &refresh,
            CLIENT,
            RESOURCE,
            Utc::now() - Duration::minutes(10) - Duration::seconds(1),
        )
        .await?,
    );
    notegate_db::PurgeRepo::new(db.pool.clone())
        .run_once()
        .await?;
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE kind='access'),count(*) FILTER (WHERE kind='refresh') FROM oauth_credentials",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(counts, (0, 2));
    let cached: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM oauth_credentials WHERE private_refresh_response IS NOT NULL",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(cached, 0);
    rejected(
        svc.refresh(&refresh, CLIENT, RESOURCE, Utc::now()).await?,
        "invalid_grant",
    );
    rejected(
        svc.refresh(
            &current.refresh_token.unwrap(),
            CLIENT,
            RESOURCE,
            Utc::now(),
        )
        .await?,
        "invalid_grant",
    );
    db.cleanup().await;
    Ok(())
}
