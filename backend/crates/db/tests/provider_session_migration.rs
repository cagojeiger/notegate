mod common;

use common::{TestDb, space_with_root};
use notegate_core::security::PiiCrypto;
use notegate_db::browser_session_migration::prepare_browser_sessions;
use notegate_db::browser_session_repo::token_prefix;
use notegate_db::{AccountRepo, BrowserSessionRepo};
use serde_json::Value;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const OLD_ISSUER: &str = "https://auth.example.test";
const NEW_ISSUER: &str = "https://accounts.google.com";

async fn old_session(db: &TestDb, owner: Uuid, case: &str) -> TestResult<(Uuid, String)> {
    let id = Uuid::new_v4();
    let hash = PiiCrypto::test().browser_session_hash(&id.to_string(), "retained-secret")?;
    sqlx::query(
        "INSERT INTO browser_sessions \
         (id,user_id,token_prefix,token_hash,hash_key_id,hash_version,oidc_issuer, \
          client_environment,idle_expires_at,expires_at,revoked_at,revoked_reason) \
         VALUES ($1,$2,$3,$4,'test-lookup',1, \
          CASE WHEN $5='unbound' THEN NULL WHEN $5='foreign' THEN 'https://foreign.test' ELSE $6 END, \
          'Firefox on Linux', \
          CASE WHEN $5 IN ('idle','expired') THEN now()-interval '1 hour' ELSE now()+interval '1 day' END, \
          CASE WHEN $5='expired' THEN now()-interval '1 minute' ELSE now()+interval '7 days' END, \
          CASE WHEN $5='revoked' THEN now() END,CASE WHEN $5='revoked' THEN 'user_revoked' END)",
    )
    .bind(id).bind(owner).bind(token_prefix(id)).bind(&hash).bind(case).bind(OLD_ISSUER)
    .execute(&db.pool).await?;
    Ok((id, hash))
}

async fn rows(db: &TestDb, table: &str, omit_issuer: bool) -> TestResult<Value> {
    let projection = if omit_issuer {
        "to_jsonb(t)-'oidc_issuer'"
    } else {
        "to_jsonb(t)"
    };
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT coalesce(jsonb_agg(v ORDER BY v::text),'[]'::jsonb) \
         FROM (SELECT {projection} AS v FROM {table} t) s"
    )))
    .fetch_one(&db.pool)
    .await?)
}

#[tokio::test]
async fn unprepared_or_changed_session_origins_block_schema_removal() -> TestResult {
    let Some(db) = TestDb::setup_before(62).await? else {
        return Ok(());
    };
    let (owner, _, _) = space_with_root(&db.pool, "unprepared-session").await?;
    let (id, _) = old_session(&db, owner, "live").await?;
    let before = rows(&db, "browser_sessions", false).await?;
    let audit = rows(&db, "audit_events", false).await?;
    assert!(db.apply_migration(62).await.is_err());
    assert_eq!(rows(&db, "browser_sessions", false).await?, before);
    assert_eq!(rows(&db, "audit_events", false).await?, audit);
    assert!(notegate_db::check_readiness(&db.pool).await.is_err());
    prepare_browser_sessions(&db.pool, &PiiCrypto::test(), OLD_ISSUER).await?;
    sqlx::query("UPDATE browser_sessions SET oidc_issuer='https://changed.test' WHERE id=$1")
        .bind(id)
        .execute(&db.pool)
        .await?;
    let changed = rows(&db, "browser_sessions", false).await?;
    assert!(db.apply_migration(62).await.is_err());
    assert!(
        prepare_browser_sessions(&db.pool, &PiiCrypto::test(), OLD_ISSUER)
            .await
            .is_err()
    );
    assert_eq!(rows(&db, "browser_sessions", false).await?, changed);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn wrong_preparation_keys_leave_sessions_audit_and_epochs_unchanged() -> TestResult {
    let Some(db) = TestDb::setup_before(62).await? else {
        return Ok(());
    };
    let (owner, _, _) = space_with_root(&db.pool, "wrong-preparation-keys").await?;
    old_session(&db, owner, "live").await?;
    for wrong_lookup in [true, false] {
        let crypto = PiiCrypto::from_root_secrets(
            "test-enc",
            &secrecy::SecretString::from(
                if wrong_lookup {
                    "notegate-test-enc-root-secret-32-bytes"
                } else {
                    "incorrect-enc-root-secret-32-bytes-long"
                }
                .to_owned(),
            ),
            "test-lookup",
            &secrecy::SecretString::from(
                if wrong_lookup {
                    "incorrect-lookup-root-secret-32-bytes-long"
                } else {
                    "notegate-test-lookup-root-secret-32-bytes"
                }
                .to_owned(),
            ),
        )?;
        let before = rows(&db, "browser_sessions", false).await?;
        let audit = rows(&db, "audit_events", false).await?;
        let epochs = rows(&db, "crypto_key_epochs", false).await?;
        assert!(
            prepare_browser_sessions(&db.pool, &crypto, OLD_ISSUER)
                .await
                .is_err()
        );
        assert_eq!(rows(&db, "browser_sessions", false).await?, before);
        assert_eq!(rows(&db, "audit_events", false).await?, audit);
        assert_eq!(rows(&db, "crypto_key_epochs", false).await?, epochs);
        assert!(db.apply_migration(62).await.is_err());
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn session_preparation_is_atomic_idempotent_and_binds_the_previous_issuer() -> TestResult {
    let Some(db) = TestDb::setup_before(62).await? else {
        return Ok(());
    };
    let (owner, _, _) = space_with_root(&db.pool, "prepare-session").await?;
    old_session(&db, owner, "live").await?;
    old_session(&db, owner, "foreign").await?;
    let before = rows(&db, "browser_sessions", false).await?;
    let audit = rows(&db, "audit_events", false).await?;
    sqlx::query("ALTER TABLE audit_events ADD CONSTRAINT reject_migration_revoke CHECK(op_type<>'session.revoke') NOT VALID")
        .execute(&db.pool).await?;
    assert!(
        prepare_browser_sessions(&db.pool, &PiiCrypto::test(), OLD_ISSUER)
            .await
            .is_err()
    );
    assert_eq!(rows(&db, "browser_sessions", false).await?, before);
    assert_eq!(rows(&db, "audit_events", false).await?, audit);
    sqlx::query("ALTER TABLE audit_events DROP CONSTRAINT reject_migration_revoke")
        .execute(&db.pool)
        .await?;
    let first = prepare_browser_sessions(&db.pool, &PiiCrypto::test(), OLD_ISSUER).await?;
    assert_eq!((first.recorded, first.revoked), (2, 1));
    let prepared = rows(&db, "browser_sessions", false).await?;
    let audit = rows(&db, "audit_events", false).await?;
    let second = prepare_browser_sessions(&db.pool, &PiiCrypto::test(), OLD_ISSUER).await?;
    assert_eq!((second.recorded, second.revoked), (0, 0));
    assert!(
        prepare_browser_sessions(&db.pool, &PiiCrypto::test(), NEW_ISSUER)
            .await
            .is_err()
    );
    assert_eq!(rows(&db, "browser_sessions", false).await?, prepared);
    assert_eq!(rows(&db, "audit_events", false).await?, audit);
    // A session created after preparation must not bypass the migration guard.
    old_session(&db, owner, "live").await?;
    assert!(db.apply_migration(62).await.is_err());
    let third = prepare_browser_sessions(&db.pool, &PiiCrypto::test(), OLD_ISSUER).await?;
    assert_eq!((third.recorded, third.revoked), (1, 0));
    db.apply_migration(62).await?;
    db.apply_migration(63).await?;
    notegate_db::check_readiness(&db.pool).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn provider_migration_preserves_accounts_sessions_documents_and_both_oauth_kinds()
-> TestResult {
    let Some(db) = TestDb::setup_before(62).await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "provider-migration").await?;
    let crypto = PiiCrypto::test();
    let accounts = AccountRepo::with_crypto(db.pool.clone(), crypto.clone());
    let original_accounts = rows(&db, "accounts", false).await?;
    let original_users = rows(&db, "users", false).await?;
    let link = notegate_db::account_repo::IdentityLink {
        account_id: owner,
        issuer: NEW_ISSUER.to_owned(),
        subject: "verified-google-subject".to_owned(),
    };
    accounts.import_identity_links(&[link]).await?;
    assert_eq!(rows(&db, "accounts", false).await?, original_accounts);
    assert_eq!(rows(&db, "users", false).await?, original_users);
    let files = notegate_db::FilesRepo::new(db.pool.clone());
    let (node, _) = files
        .insert_text(
            space,
            root,
            "retained.md",
            &notegate_model::files::StoredContent {
                body: notegate_model::files::WriteTextBody::Plain("retained".to_owned()),
                content_sha256: "a".repeat(64),
                byte_len: 8,
                line_count: 1,
            },
            owner,
        )
        .await?;
    let mut sessions = Vec::new();
    for case in ["live", "idle", "expired", "revoked", "unbound", "foreign"] {
        let (id, hash) = old_session(&db, owner, case).await?;
        sessions.push((case, id, hash));
    }
    let now = chrono::Utc::now();
    let mut credentials = Vec::new();
    for kind in ["cli", "mcp"] {
        let id = Uuid::new_v4();
        let client = format!("notegate-{kind}");
        let resource = format!("https://notegate.example.test/{kind}");
        let access = crypto.oauth_hash("token", &format!("retained-{kind}-access"))?;
        let refresh = crypto.oauth_hash("token", &format!("retained-{kind}-refresh"))?;
        sqlx::query("INSERT INTO oauth_grants(id,user_id,client_id,client_name,resource,scope,created_at,expires_at,idle_expires_at) \
             VALUES($1,$2,$3,$3,$4,$5,now(),now()+interval '7 days',now()+interval '7 days')")
            .bind(id).bind(owner).bind(&client).bind(&resource).bind(format!("{kind} offline_access"))
            .execute(&db.pool).await?;
        sqlx::query("INSERT INTO oauth_credentials(token_hash,hash_key_id,hash_version,grant_id,kind,expires_at) \
             VALUES($1,'test-lookup',1,$3,'access',now()+interval '10 minutes'), \
                   ($2,'test-lookup',1,$3,'refresh',now()+interval '7 days')")
            .bind(&access).bind(&refresh).bind(id).execute(&db.pool).await?;
        credentials.push((kind, client, resource, access, refresh));
    }
    let tables = [
        "accounts",
        "users",
        "external_identities",
        "spaces",
        "nodes",
        "text_objects",
        "text_revisions",
        "file_change_events",
        "command_invocations",
        "oauth_grants",
        "oauth_credentials",
        "crypto_key_epochs",
    ];
    let mut before = Vec::new();
    for table in tables {
        before.push((table, rows(&db, table, false).await?));
    }
    let old_audit = rows(&db, "audit_events", false).await?;
    let live_before: Value =
        sqlx::query_scalar("SELECT to_jsonb(t)-'oidc_issuer' FROM browser_sessions t WHERE id=$1")
            .bind(sessions.first().ok_or("missing live session")?.1)
            .fetch_one(&db.pool)
            .await?;
    let result = prepare_browser_sessions(&db.pool, &crypto, OLD_ISSUER).await?;
    assert_eq!((result.recorded, result.revoked), (6, 2));
    let prepared = rows(&db, "browser_sessions", true).await?;
    db.apply_migration(62).await?;
    assert_eq!(rows(&db, "browser_sessions", false).await?, prepared);
    let live_after: Value =
        sqlx::query_scalar("SELECT to_jsonb(t) FROM browser_sessions t WHERE id=$1")
            .bind(sessions.first().ok_or("missing live session")?.1)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(live_after, live_before);
    for (table, expected) in before {
        assert_eq!(rows(&db, table, false).await?, expected, "{table}");
    }
    db.apply_migration(63).await?;
    notegate_db::check_readiness(&db.pool).await?;
    let audit = rows(&db, "audit_events", false).await?;
    for event in old_audit.as_array().ok_or("missing Audit")? {
        assert!(audit.as_array().ok_or("missing Audit")?.contains(event));
    }
    assert!(files.find_text(space, node.id).await?.is_some());
    assert_eq!(
        accounts
            .find_user_id_by_identity(NEW_ISSUER, "verified-google-subject")
            .await?,
        Some(owner)
    );
    let repo = BrowserSessionRepo::new(db.pool.clone());
    for (case, id, hash) in sessions {
        assert_eq!(
            repo.find_live_caller_by_token(id, &hash, &crypto)
                .await?
                .is_some(),
            case == "live",
            "{case}"
        );
        assert_eq!(
            repo.renew_idle_expiry(id, &hash).await?,
            case == "live",
            "{case}"
        );
        let provenance: (Option<String>, String) = sqlx::query_as("SELECT metadata->>'issuer',metadata->>'trusted_issuer' FROM audit_events WHERE resource_id=$1 AND op_type='session.migrate'")
            .bind(id).fetch_one(&db.pool).await?;
        assert_eq!(provenance.1, OLD_ISSUER);
        assert_eq!(
            provenance.0.as_deref(),
            match case {
                "unbound" => None,
                "foreign" => Some("https://foreign.test"),
                _ => Some(OLD_ISSUER),
            }
        );
        if case == "live" {
            assert_eq!(
                repo.list_owned_live(owner, id).await?.first().map(|s| s.id),
                Some(id)
            );
            assert_eq!(repo.revoke_owned_sessions(owner, Some(id), None).await?, 1);
            assert!(!repo.renew_idle_expiry(id, &hash).await?);
        }
    }
    let oauth = notegate_db::oauth_repo::OAuthRepo::new(db.pool.clone());
    for (kind, client, resource, access, refresh) in credentials {
        assert_eq!(
            oauth.access_user(&access, &resource, kind, now).await?,
            Some(owner)
        );
        let outcome = oauth
            .rotate_refresh(
                &refresh,
                &client,
                &resource,
                None,
                &notegate_db::oauth_repo::CredentialIssue {
                    client_environment: None,
                    access_hash: &format!("new-{kind}-access"),
                    refresh_hash: None,
                    key_id: crypto.lookup_key_id(),
                    version: crypto.version(),
                    access_expires_at: now + chrono::Duration::minutes(10),
                    grant_expires_at: now + chrono::Duration::days(7),
                    idle_expires_at: now + chrono::Duration::days(7),
                    private_refresh_response: None,
                },
                now,
            )
            .await?;
        assert!(
            matches!(outcome,notegate_db::oauth_repo::ExchangeOutcome::Issued(g) if g.user_id==owner)
        );
    }
    db.cleanup().await;
    Ok(())
}
