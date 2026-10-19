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
        let value = if table == "browser_sessions" {
            "(to_jsonb(t)-$1::text[]-'validated_until') || jsonb_build_object('idle_expires_at', \
             coalesce(to_jsonb(t)->'idle_expires_at',to_jsonb(t)->'validated_until'))"
        } else {
            "to_jsonb(t)-$1::text[]"
        };
        let rows: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT coalesce(jsonb_agg(value ORDER BY value::text),'[]'::jsonb) \
             FROM (SELECT {value} AS value FROM {table} t) s"
        )))
        .bind(if table == "browser_sessions" {
            vec!["local_session", "client_environment"]
        } else if table == "oauth_grants" {
            vec!["client_environment", "last_token_at"]
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
                client_environment: None,
                session_id: id,
                user_id: owner,
                token_prefix: &token_prefix(id),
                token_hash: &hash,
                idle_expires_at: now + Duration::hours(1),
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
        "INSERT INTO oauth_grants(id,user_id,client_id,client_name,resource,scope,created_at,expires_at,idle_expires_at) \
         VALUES ($1,$2,'notegate-cli','NoteGate CLI',$3,'cli offline_access',$4,$5,$5)",
    ).bind(grant).bind(owner).bind(resource).bind(now).bind(now + Duration::days(7)).execute(&db.pool).await?;
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
    db.apply_migration(61).await?;
    assert_eq!(snapshot(&db.pool).await?, before);
    let unknown: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM browser_sessions WHERE client_environment IS NOT NULL",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(unknown, 0);
    let unknown: i64 = sqlx::query_scalar("SELECT count(*) FROM oauth_grants WHERE client_environment IS NOT NULL OR last_token_at IS NOT NULL")
        .fetch_one(&db.pool).await?;
    assert_eq!(unknown, 0);
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
                client_environment: None,
                session_id: id,
                user_id: owner,
                token_prefix: &token_prefix(id),
                token_hash: "new-session-hash",
                idle_expires_at: now + Duration::hours(1),
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

#[tokio::test]
async fn idle_expiry_migration_preserves_expired_and_revoked_sessions() -> TestResult {
    let Some(db) = TestDb::setup_before(61).await? else {
        return Ok(());
    };
    let (user, _, _) = space_with_root(&db.pool, "idle-migration").await?;
    let crypto = PiiCrypto::test();
    let mut sessions = Vec::new();
    for case in ["live", "idle", "absolute", "revoked", "unbound"] {
        let id = Uuid::new_v4();
        let hash = crypto.browser_session_hash(&id.to_string(), "retained-secret")?;
        sqlx::query("INSERT INTO browser_sessions(id,user_id,token_prefix,token_hash,hash_key_id,hash_version,oidc_issuer,validated_until,expires_at,revoked_at) \
          VALUES($1,$2,$3,$4,'test-lookup',1,CASE WHEN $5='unbound' THEN NULL ELSE 'https://auth.example.test' END, \
          CASE WHEN $5 IN ('idle','absolute') THEN now()-interval '1 hour' ELSE now()+interval '1 hour' END, \
          CASE WHEN $5='absolute' THEN now()-interval '1 second' ELSE now()+interval '7 days' END, \
          CASE WHEN $5='revoked' THEN now() ELSE NULL END)")
            .bind(id).bind(user).bind(token_prefix(id)).bind(&hash).bind(case).execute(&db.pool).await?;
        sessions.push((case, id, hash));
    }
    let before = snapshot(&db.pool).await?;
    db.apply_migration(61).await?;
    assert_eq!(snapshot(&db.pool).await?, before);
    let columns: (i64, i64) = sqlx::query_as("SELECT \
      count(*) FILTER (WHERE column_name='validated_until'),count(*) FILTER (WHERE column_name='idle_expires_at') \
      FROM information_schema.columns WHERE table_schema=current_schema() AND table_name='browser_sessions'")
        .fetch_one(&db.pool).await?;
    assert_eq!(columns, (0, 1));
    notegate_db::check_readiness(&db.pool).await?;
    let repo = BrowserSessionRepo::new(db.pool.clone());
    for (case, id, hash) in sessions {
        assert_eq!(
            repo.renew_idle_expiry(id, &hash, "https://auth.example.test")
                .await?,
            case == "live",
            "{case}"
        );
        let retained = repo
            .find_by_token(id, &hash)
            .await?
            .ok_or("session row lost")?;
        assert_eq!(retained.user_id, user);
    }
    db.cleanup().await;
    Ok(())
}
