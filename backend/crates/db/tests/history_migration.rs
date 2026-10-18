mod common;

use common::{TestDb, space_with_root};
use notegate_db::{CommandInvocationRepo, FilesRepo, NewCommandInvocation, TextMutationKind};
use notegate_model::CommandInvocationSurface;
use notegate_model::files::{FileMutationContext, StoredContent, WriteTextBody};
use serde_json::{Value, json};
use sqlx::PgPool;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn body(value: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(value.to_owned()),
        content_sha256: format!("{value:0<64}"),
        byte_len: value.len() as i64,
        line_count: 1,
    }
}

async fn snapshot(pool: &PgPool, exclude_retired_fields: bool) -> TestResult<Value> {
    let mut state = serde_json::Map::new();
    for (table, removed) in [
        ("accounts", vec![]),
        ("users", vec![]),
        ("external_identities", vec![]),
        ("spaces", vec![]),
        ("nodes", vec![]),
        ("audit_events", vec![]),
        ("browser_sessions", vec![]),
        ("oauth_grants", vec![]),
        ("oauth_credentials", vec![]),
        ("crypto_key_epochs", vec![]),
        ("file_change_events", vec![]),
        ("text_objects", vec!["revision_purpose"]),
        ("text_revisions", vec!["purpose"]),
        (
            "command_invocations",
            vec!["purpose", "space_name", "input", "response"],
        ),
    ] {
        let rows: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT coalesce(jsonb_agg(value ORDER BY value::text),'[]'::jsonb) FROM (SELECT to_jsonb(t)-$1::text[] AS value FROM {table} t) s"
        ))).bind(if exclude_retired_fields { removed } else { vec![] }).fetch_one(pool).await?;
        state.insert(table.into(), rows);
    }
    Ok(Value::Object(state))
}

#[tokio::test]
async fn plaintext_history_blocks_cleanup_before_any_ddl() -> TestResult {
    for plaintext in [
        "INSERT INTO file_change_events(space_id,op_type,metadata) SELECT id,'text.write','{\"item_name\":\"retained\"}'::jsonb FROM spaces",
        "INSERT INTO command_invocations(owner_user_id,actor_account_id,caller_kind,tool,purpose,input,outcome,duration_ms) SELECT id,id,'user','read','retained','{}'::jsonb,'success',0 FROM users",
        "UPDATE text_objects SET revision_private_purpose=NULL,revision_purpose='retained'",
        "UPDATE text_revisions SET private_purpose=NULL,purpose='retained'",
    ] {
        let Some(db) = TestDb::setup_before(59).await? else {
            return Ok(());
        };
        let (owner, space, root) = space_with_root(&db.pool, "blocked-history").await?;
        let files = FilesRepo::new(db.pool.clone());
        let (node, _) = files
            .insert_text(space, root, "note.md", &body("a"), owner)
            .await?;
        files
            .save_text_content(
                space,
                node.id,
                &body("b"),
                None,
                owner,
                TextMutationKind::Write,
            )
            .await?;
        sqlx::query(plaintext).execute(&db.pool).await?;
        let before = snapshot(&db.pool, false).await?;
        assert!(db.apply_migration(59).await.is_err());
        assert_eq!(snapshot(&db.pool, false).await?, before);
        let retained: (bool, bool, bool, i64) = sqlx::query_as(
            "SELECT to_regclass('mcp_invocations') IS NOT NULL, \
             to_regprocedure('preserve_revision_private_purpose()') IS NOT NULL, \
             to_regprocedure('clear_replaced_private_purpose()') IS NOT NULL, \
             (SELECT count(*) FROM information_schema.columns WHERE table_schema=current_schema() \
              AND ((table_name='command_invocations' AND column_name IN ('purpose','space_name','input','response')) \
              OR (table_name='text_objects' AND column_name='revision_purpose') \
              OR (table_name='text_revisions' AND column_name='purpose')))"
        ).fetch_one(&db.pool).await?;
        assert_eq!(retained, (true, true, true, 6));
        db.cleanup().await;
    }
    Ok(())
}

#[tokio::test]
async fn encrypted_history_cleanup_preserves_rows_and_current_writes() -> TestResult {
    let Some(db) = TestDb::setup_before(59).await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "preserved-history").await?;
    let files = FilesRepo::new(db.pool.clone()).with_mutation_context(FileMutationContext {
        purpose: Some("Preserve the recorded reason".into()),
        ..FileMutationContext::default()
    });
    let (node, _) = files
        .insert_text(space, root, "note.md", &body("a"), owner)
        .await?;
    files
        .save_text_content(
            space,
            node.id,
            &body("b"),
            None,
            owner,
            TextMutationKind::Write,
        )
        .await?;
    let history = files.list_text_revisions(space, node.id, 10, None).await?;
    let revision = history.revisions.first().ok_or("missing revision")?.id;
    let commands = CommandInvocationRepo::new(db.pool.clone());
    let crypto = notegate_core::security::PiiCrypto::test();
    let snapshot_id = uuid::Uuid::new_v4();
    let payload = json!({"purpose":"Preserve this invocation", "space_name":null,
        "input":{"op":"spaces"}, "response":{"ok":true}});
    let encrypted = serde_json::to_value(crypto.encrypt_history(
        &format!("invocations/{owner}/{snapshot_id}"),
        &payload.to_string(),
    )?)?;
    sqlx::query("INSERT INTO command_invocations(owner_user_id,actor_account_id,caller_kind,surface,tool,op,input,outcome,duration_ms,snapshot_id,private_payload) VALUES ($1,$1,'user','cli','read','spaces','{}'::jsonb,'success',1,$2,$3)")
        .bind(owner).bind(snapshot_id).bind(encrypted).execute(&db.pool).await?;
    let now = chrono::Utc::now();
    let session_id = uuid::Uuid::new_v4();
    let session_hash = crypto.browser_session_hash(&session_id.to_string(), "retained-session")?;
    let prefix = notegate_db::browser_session_repo::token_prefix(session_id);
    let sessions = notegate_db::BrowserSessionRepo::new(db.pool.clone());
    common::insert_pre_marker_removal_session(
        &db.pool,
        notegate_db::browser_session_repo::InsertBrowserSession {
            session_id,
            user_id: owner,
            token_prefix: &prefix,
            token_hash: &session_hash,
            validated_until: now + chrono::Duration::hours(1),
            expires_at: now + chrono::Duration::days(7),
        },
        Some("https://auth.example.test"),
    )
    .await?;
    let grant = uuid::Uuid::new_v4();
    let access_hash = crypto.oauth_hash("token", "retained-access")?;
    let refresh_hash = crypto.oauth_hash("token", "retained-refresh")?;
    let resource = "https://notegate.example.test/cli";
    sqlx::query("INSERT INTO oauth_grants(id,user_id,client_id,client_name,resource,scope,created_at,expires_at,idle_expires_at) VALUES ($1,$2,'notegate-cli','NoteGate CLI',$3,'cli offline_access',$4,$5,$5)")
        .bind(grant).bind(owner).bind(resource).bind(now).bind(now+chrono::Duration::days(7)).execute(&db.pool).await?;
    sqlx::query("INSERT INTO oauth_credentials(token_hash,hash_key_id,hash_version,grant_id,kind,expires_at) VALUES ($1,'test-lookup',1,$3,'access',$4),($2,'test-lookup',1,$3,'refresh',$5)")
        .bind(&access_hash).bind(&refresh_hash).bind(grant).bind(now+chrono::Duration::minutes(10)).bind(now+chrono::Duration::days(7)).execute(&db.pool).await?;
    let before = snapshot(&db.pool, true).await?;
    db.apply_migration(59).await?;
    assert_eq!(snapshot(&db.pool, true).await?, before);
    assert_eq!(
        sessions
            .find_live_by_token(session_id, &session_hash)
            .await?
            .ok_or("session missing")?
            .user_id,
        owner
    );
    assert_eq!(
        notegate_db::oauth_repo::OAuthRepo::new(db.pool.clone())
            .access_user(&access_hash, resource, "cli", now)
            .await?,
        Some(owner)
    );
    let removed: (bool, bool, bool, i64) = sqlx::query_as(
        "SELECT to_regclass('mcp_invocations') IS NULL, \
         to_regprocedure('preserve_revision_private_purpose()') IS NULL, \
         to_regprocedure('clear_replaced_private_purpose()') IS NULL, \
         (SELECT count(*) FROM information_schema.columns WHERE table_schema=current_schema() \
          AND ((table_name='command_invocations' AND column_name IN ('purpose','space_name','input','response')) \
          OR (table_name='text_objects' AND column_name='revision_purpose') \
          OR (table_name='text_revisions' AND column_name='purpose')))"
    ).fetch_one(&db.pool).await?;
    assert_eq!(removed, (true, true, true, 0));
    assert_eq!(
        files
            .read_text_revision(space, node.id, revision)
            .await?
            .content,
        "a"
    );
    assert_eq!(
        files
            .list_text_revisions(space, node.id, 10, None)
            .await?
            .current
            .ok_or("missing current revision")?
            .purpose,
        history.current.ok_or("missing current revision")?.purpose
    );
    assert_eq!(
        commands
            .list_by_owner(owner, CommandInvocationSurface::Cli, 10, None)
            .await?
            .first()
            .ok_or("missing invocation")?
            .purpose
            .as_deref(),
        Some("Preserve this invocation")
    );
    commands
        .insert(NewCommandInvocation {
            invocation_id: None,
            owner_user_id: owner,
            actor_account_id: owner,
            caller_kind: "user",
            surface: "cli",
            tool: "read",
            op: Some("spaces"),
            purpose: Some("Preserve this invocation"),
            space_name: None,
            input: &json!({"op":"spaces"}),
            response: Some(&json!({"ok":true})),
            outcome: "success",
            error_code: None,
            duration_ms: 1,
        })
        .await?;
    assert_eq!(
        commands
            .list_by_owner(owner, CommandInvocationSurface::Cli, 10, None)
            .await?
            .len(),
        2
    );
    files
        .save_text_content(
            space,
            node.id,
            &body("c"),
            None,
            owner,
            TextMutationKind::Write,
        )
        .await?;
    assert_eq!(
        files
            .list_text_revisions(space, node.id, 10, None)
            .await?
            .revisions
            .len(),
        2
    );
    assert!(
        sqlx::query("UPDATE file_change_events SET private_metadata=NULL")
            .execute(&db.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE command_invocations SET private_payload=NULL")
            .execute(&db.pool)
            .await
            .is_err()
    );
    db.cleanup().await;
    Ok(())
}
