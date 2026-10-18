mod common;

use chrono::{Duration, Utc};
use common::{TestDb, insert_pre_marker_removal_session, space_with_root};
use notegate_core::security::PiiCrypto;
use notegate_db::browser_session_repo::{InsertBrowserSession, token_prefix};
use notegate_db::oauth_repo::OAuthRepo;
use notegate_db::{BrowserSessionRepo, FilesRepo};
use notegate_model::files::{StoredContent, WriteTextBody};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

async fn snapshot(pool: &PgPool) -> TestResult<Value> {
    let mut state = serde_json::Map::new();
    for table in [
        "accounts",
        "users",
        "external_identities",
        "spaces",
        "nodes",
        "text_objects",
        "text_revisions",
        "file_change_events",
        "command_invocations",
        "audit_events",
        "browser_sessions",
        "oauth_grants",
        "oauth_credentials",
        "crypto_key_epochs",
    ] {
        let rows: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT coalesce(jsonb_agg(value ORDER BY value::text),'[]'::jsonb) \
             FROM (SELECT to_jsonb(t)-$1::text[] AS value FROM {table} t) s"
        )))
        .bind(if table == "browser_sessions" {
            vec!["local_session"]
        } else {
            vec![]
        })
        .fetch_one(pool)
        .await?;
        state.insert(table.into(), rows);
    }
    Ok(Value::Object(state))
}

#[tokio::test]
async fn removing_session_marker_preserves_tokens_issuers_and_owned_data() -> TestResult {
    let Some(db) = TestDb::setup_before(60).await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "session-migration").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (node, _) = files
        .insert_text(
            space,
            root,
            "retained.md",
            &StoredContent {
                body: WriteTextBody::Plain("retained".into()),
                content_sha256: "a".repeat(64),
                byte_len: 8,
                line_count: 1,
            },
            owner,
        )
        .await?;
    let crypto = PiiCrypto::test();
    let now = Utc::now();
    let issuer = "https://auth.example.test";
    let current = Uuid::new_v4();
    let unbound = Uuid::new_v4();
    for (id, bound) in [(current, Some(issuer)), (unbound, None)] {
        let hash = crypto.browser_session_hash(&id.to_string(), "retained-secret")?;
        insert_pre_marker_removal_session(
            &db.pool,
            InsertBrowserSession {
                session_id: id,
                user_id: owner,
                token_prefix: &token_prefix(id),
                token_hash: &hash,
                validated_until: now + Duration::hours(1),
                expires_at: now + Duration::days(7),
            },
            bound,
        )
        .await?;
    }
    let grant = Uuid::new_v4();
    let resource = "https://notegate.example.test/cli";
    let access = crypto.oauth_hash("token", "retained-access")?;
    let refresh = crypto.oauth_hash("token", "retained-refresh")?;
    sqlx::query(
        "INSERT INTO oauth_grants(id,user_id,client_id,client_name,resource,scope,expires_at,idle_expires_at) \
         VALUES ($1,$2,'notegate-cli','NoteGate CLI',$3,'cli offline_access',$4,$4)",
    ).bind(grant).bind(owner).bind(resource).bind(now + Duration::days(7)).execute(&db.pool).await?;
    sqlx::query(
        "INSERT INTO oauth_credentials(token_hash,hash_key_id,hash_version,grant_id,kind,expires_at) \
         VALUES ($1,'test-lookup',1,$3,'access',$4),($2,'test-lookup',1,$3,'refresh',$5)",
    ).bind(&access).bind(&refresh).bind(grant).bind(now + Duration::minutes(10))
        .bind(now + Duration::days(7)).execute(&db.pool).await?;

    let before = snapshot(&db.pool).await?;
    db.apply_migration(60).await?;
    assert_eq!(snapshot(&db.pool).await?, before);
    let retired: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM information_schema.columns \
         WHERE table_schema=current_schema() AND table_name='browser_sessions' AND column_name='local_session'), \
         (SELECT count(*) FROM pg_constraint WHERE conrelid='browser_sessions'::regclass AND conname='browser_session_issuer')",
    ).fetch_one(&db.pool).await?;
    assert_eq!(retired, (0, 0));
    notegate_db::check_readiness(&db.pool).await?;
    let sessions = BrowserSessionRepo::new(db.pool.clone());
    for (id, expected_issuer) in [(current, Some(issuer)), (unbound, None)] {
        let hash = crypto.browser_session_hash(&id.to_string(), "retained-secret")?;
        let session = sessions
            .find_live_caller_by_token(id, &hash, &crypto)
            .await?
            .ok_or("retained session missing")?;
        assert_eq!(session.account.id, owner);
        assert_eq!(session.oidc_issuer.as_deref(), expected_issuer);
    }
    assert!(files.find_text(space, node.id).await?.is_some());
    assert_eq!(
        OAuthRepo::new(db.pool.clone())
            .access_user(&access, resource, "cli", now)
            .await?,
        Some(owner)
    );
    let id = Uuid::new_v4();
    sessions
        .insert_session(
            InsertBrowserSession {
                session_id: id,
                user_id: owner,
                token_prefix: &token_prefix(id),
                token_hash: "new-session-hash",
                validated_until: now + Duration::hours(1),
                expires_at: now + Duration::days(7),
            },
            issuer,
        )
        .await?;
    assert!(sessions.revoke_session_for_logout(id).await?);
    let events: Vec<String> =
        sqlx::query_scalar("SELECT op_type FROM audit_events WHERE resource_id=$1 ORDER BY id")
            .bind(id)
            .fetch_all(&db.pool)
            .await?;
    assert_eq!(events, ["session.login", "session.logout"]);
    db.cleanup().await;
    Ok(())
}
