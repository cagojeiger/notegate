//! Integration tests for append-only command invocation history.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::unwrap_in_result)]
mod common;

use common::{TestDb, insert_legacy_user_account, insert_user_account};
use notegate_db::{CommandInvocationRepo, NewCommandInvocation};
use notegate_model::{CommandInvocationCursor, CommandInvocationSurface};

#[tokio::test]
async fn legacy_payload_encryption_is_bounded_restartable_and_row_bound()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup_before(49).await? else {
        return Ok(());
    };
    let owner =
        insert_user_account(&db.pool, "legacy-history", "legacy-history@example.test").await?;
    sqlx::query(
        "INSERT INTO command_invocations \
         (owner_user_id, actor_account_id, caller_kind, surface, tool, op, purpose, input, response, outcome, duration_ms) \
         SELECT $1, $1, 'user', 'mcp', 'read', 'spaces', 'legacy purpose', \
                jsonb_build_object('target', 'private-' || n), '{\"ok\":true}'::jsonb, 'success', 1 \
         FROM generate_series(1,101) n",
    ).bind(owner).execute(&db.pool).await?;
    db.apply_migration(49).await?;
    let repo = CommandInvocationRepo::new(db.pool.clone());
    let before = repo
        .list_by_owner(owner, CommandInvocationSurface::Mcp, 101, None)
        .await?;
    assert_eq!(repo.encrypt_legacy_payloads().await?, 100);
    // The old binary's compatibility view remains writable during rollout.
    sqlx::query(
        "INSERT INTO mcp_invocations \
         (owner_user_id, actor_account_id, caller_kind, tool, op, purpose, input, outcome, duration_ms) \
         VALUES ($1, $1, 'user', 'read', 'spaces', 'rolling purpose', '{}'::jsonb, 'success', 1)",
    ).bind(owner).execute(&db.pool).await?;
    assert_eq!(repo.encrypt_legacy_payloads().await?, 2);
    assert_eq!(repo.encrypt_legacy_payloads().await?, 0);
    let after = repo
        .list_by_owner(owner, CommandInvocationSurface::Mcp, 102, None)
        .await?;
    assert_eq!(after.len(), 102);
    for prior in before {
        let migrated = after
            .iter()
            .find(|item| item.id == prior.id)
            .expect("same history identity");
        assert_eq!(migrated.purpose, prior.purpose);
        assert_eq!(migrated.input, prior.input);
        assert_eq!(migrated.response, prior.response);
    }
    let plaintext_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM command_invocations WHERE purpose IS NOT NULL OR space_name IS NOT NULL \
         OR input != '{}'::jsonb OR response IS NOT NULL OR private_payload IS NULL",
    ).fetch_one(&db.pool).await?;
    assert_eq!(plaintext_count, 0);
    let source = after.first().expect("first row").id;
    let target = after.last().expect("last row").id;
    sqlx::query("UPDATE command_invocations SET private_payload=(SELECT private_payload FROM command_invocations WHERE id=$1) WHERE id=$2")
        .bind(source).bind(target).execute(&db.pool).await?;
    assert!(
        repo.list_by_owner(owner, CommandInvocationSurface::Mcp, 102, None)
            .await
            .is_err()
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn migration_preserves_mcp_rows_and_legacy_writes_during_rolling_deployments()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup_before(40).await? else {
        return Ok(());
    };
    let owner = insert_legacy_user_account(&db.pool).await?;
    let original_id: i64 = sqlx::query_scalar(
        "INSERT INTO mcp_invocations \
         (owner_user_id, actor_account_id, caller_kind, tool, op, purpose, input, outcome, duration_ms) \
         VALUES ($1, $1, 'user', 'read', 'spaces', 'list spaces', '{}'::jsonb, 'success', 3) \
         RETURNING id",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;

    db.apply_migration(40).await?;

    let migrated: (i64, String, String) = sqlx::query_as(
        "SELECT id, surface, purpose FROM command_invocations WHERE owner_user_id = $1",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        migrated,
        (original_id, "mcp".to_owned(), "list spaces".to_owned())
    );

    let legacy_relation_kind: String = sqlx::query_scalar(
        "SELECT relkind::text FROM pg_class WHERE oid = 'mcp_invocations'::regclass",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(legacy_relation_kind, "v");

    let legacy_id: i64 = sqlx::query_scalar(
        "INSERT INTO mcp_invocations \
         (owner_user_id, actor_account_id, caller_kind, tool, op, purpose, input, outcome, duration_ms) \
         VALUES ($1, $1, 'user', 'read', 'spaces', 'legacy rolling insert', '{}'::jsonb, 'success', 4) \
         RETURNING id",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    let legacy_surface: String =
        sqlx::query_scalar("SELECT surface FROM command_invocations WHERE id = $1")
            .bind(legacy_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(legacy_surface, "mcp");

    let deleted_count: i64 = sqlx::query_scalar(
        "WITH due AS ( \
             SELECT id FROM mcp_invocations \
             WHERE created_at <= now() - make_interval(days => $1::int) \
             ORDER BY created_at, id \
             LIMIT $2 \
         ), deleted AS ( \
             DELETE FROM mcp_invocations m USING due \
             WHERE m.id = due.id \
             RETURNING m.id \
         ) \
         SELECT count(*) FROM deleted",
    )
    .bind(0_i32)
    .bind(10_i64)
    .fetch_one(&db.pool)
    .await?;
    assert!(deleted_count >= 1);
    let deleted: bool =
        sqlx::query_scalar("SELECT NOT EXISTS (SELECT 1 FROM command_invocations WHERE id = $1)")
            .bind(legacy_id)
            .fetch_one(&db.pool)
            .await?;
    assert!(deleted);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn insert_records_invocation_summary_and_payloads() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner =
        insert_user_account(&db.pool, "command-history", "command-history@example.test").await?;
    let repo = CommandInvocationRepo::new(db.pool.clone());
    let input = serde_json::json!({
        "purpose": "review recent changes",
        "op": "changes",
        "target": "Research:/"
    });
    let response = serde_json::json!({
        "kind": "error",
        "error": {"code": "not_found"}
    });

    repo.insert(NewCommandInvocation {
        invocation_id: None,
        owner_user_id: owner,
        actor_account_id: owner,
        caller_kind: "user",
        surface: "cli",
        tool: "read",
        op: Some("changes"),
        purpose: Some("review recent changes"),
        space_name: Some("Research"),
        input: &input,
        response: Some(&response),
        outcome: "error",
        error_code: Some("not_found"),
        duration_ms: 17,
    })
    .await?;

    let row = sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid, String, String, String, Option<String>, Option<String>, Option<String>, serde_json::Value, Option<serde_json::Value>, String, Option<String>, i64)>(
        "SELECT owner_user_id, actor_account_id, caller_kind, surface, tool, op, purpose, space_name, input, response, outcome, error_code, duration_ms \
         FROM command_invocations ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(row.0, owner);
    assert_eq!(row.1, owner);
    assert_eq!(row.2, "user");
    assert_eq!(row.3, "cli");
    assert_eq!(row.4, "read");
    assert_eq!(row.5.as_deref(), Some("changes"));
    assert_eq!(row.6, None);
    assert_eq!(row.7, None);
    assert_eq!(row.8, serde_json::json!({}));
    assert_eq!(row.9, None);
    assert_eq!(row.10, "error");
    assert_eq!(row.11.as_deref(), Some("not_found"));
    assert_eq!(row.12, 17);
    let listed = repo
        .list_by_owner(owner, CommandInvocationSurface::Cli, 1, None)
        .await?;
    assert_eq!(
        listed.first().and_then(|item| item.response.as_ref()),
        Some(&response)
    );
    assert_eq!(
        listed.first().map(|item| item.surface.as_str()),
        Some("cli")
    );

    let item = listed.first().expect("recorded invocation");
    assert_eq!(item.input, input);
    assert_eq!(item.purpose.as_deref(), Some("review recent changes"));
    assert_eq!(item.space_name.as_deref(), Some("Research"));
    let encrypted: serde_json::Value =
        sqlx::query_scalar("SELECT private_payload FROM command_invocations WHERE id=$1")
            .bind(item.id)
            .fetch_one(&db.pool)
            .await?;
    assert!(!encrypted.to_string().contains("Research"));
    assert!(!encrypted.to_string().contains("review recent changes"));
    assert!(
        repo.list_by_owner(
            uuid::Uuid::new_v4(),
            CommandInvocationSurface::Cli,
            10,
            None
        )
        .await?
        .is_empty()
    );
    // Changing the owner must not make another owner's encrypted payload readable.
    let other_owner =
        insert_user_account(&db.pool, "history-other", "history-other@example.test").await?;
    sqlx::query("UPDATE command_invocations SET owner_user_id=$2 WHERE id=$1")
        .bind(item.id)
        .bind(other_owner)
        .execute(&db.pool)
        .await?;
    assert!(
        repo.list_by_owner(other_owner, CommandInvocationSurface::Cli, 10, None)
            .await
            .is_err()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn failed_calls_may_be_recorded_without_a_valid_purpose()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "command-me", "command-me@example.test").await?;
    let repo = CommandInvocationRepo::new(db.pool.clone());
    let empty_input = serde_json::json!({});

    repo.insert(NewCommandInvocation {
        invocation_id: None,
        owner_user_id: owner,
        actor_account_id: owner,
        caller_kind: "user",
        surface: "mcp",
        tool: "me",
        op: None,
        purpose: None,
        space_name: None,
        input: &empty_input,
        response: None,
        outcome: "success",
        error_code: None,
        duration_ms: 0,
    })
    .await?;

    repo.insert(NewCommandInvocation {
        invocation_id: None,
        owner_user_id: owner,
        actor_account_id: owner,
        caller_kind: "user",
        surface: "mcp",
        tool: "search",
        op: Some("find"),
        purpose: None,
        space_name: None,
        input: &empty_input,
        response: None,
        outcome: "error",
        error_code: Some("invalid_params"),
        duration_ms: 0,
    })
    .await?;

    for padded_purpose in ["\tsearch notes", "search notes\n"] {
        let padded_search_purpose = repo
            .insert(NewCommandInvocation {
                invocation_id: None,
                owner_user_id: owner,
                actor_account_id: owner,
                caller_kind: "user",
                surface: "mcp",
                tool: "search",
                op: Some("find"),
                purpose: Some(padded_purpose),
                space_name: None,
                input: &empty_input,
                response: None,
                outcome: "success",
                error_code: None,
                duration_ms: 0,
            })
            .await;
        assert!(padded_search_purpose.is_err());
    }

    let space_on_non_changes_call = repo
        .insert(NewCommandInvocation {
            invocation_id: None,
            owner_user_id: owner,
            actor_account_id: owner,
            caller_kind: "user",
            surface: "mcp",
            tool: "search",
            op: Some("find"),
            purpose: Some("search notes"),
            space_name: Some("Research"),
            input: &empty_input,
            response: None,
            outcome: "success",
            error_code: None,
            duration_ms: 0,
        })
        .await;
    assert!(space_on_non_changes_call.is_err());

    let non_object_response = serde_json::json!(["not", "an", "object"]);
    let invalid_response = repo
        .insert(NewCommandInvocation {
            invocation_id: None,
            owner_user_id: owner,
            actor_account_id: owner,
            caller_kind: "user",
            surface: "mcp",
            tool: "read",
            op: Some("spaces"),
            purpose: Some("list spaces"),
            space_name: None,
            input: &empty_input,
            response: Some(&non_object_response),
            outcome: "success",
            error_code: None,
            duration_ms: 0,
        })
        .await;
    assert!(invalid_response.is_err());

    let invalid_surface = repo
        .insert(NewCommandInvocation {
            invocation_id: None,
            owner_user_id: owner,
            actor_account_id: owner,
            caller_kind: "user",
            surface: "command_api",
            tool: "read",
            op: Some("spaces"),
            purpose: Some("list spaces"),
            space_name: None,
            input: &empty_input,
            response: None,
            outcome: "success",
            error_code: None,
            duration_ms: 0,
        })
        .await;
    assert!(invalid_surface.is_err());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn list_by_owner_is_newest_first_scoped_and_cursor_paginated()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "command-list", "command-list@example.test").await?;
    let other =
        insert_user_account(&db.pool, "command-other", "command-other@example.test").await?;
    let repo = CommandInvocationRepo::new(db.pool.clone());
    let input = serde_json::json!({});

    for purpose in ["first", "second", "third"] {
        repo.insert(NewCommandInvocation {
            invocation_id: None,
            owner_user_id: owner,
            actor_account_id: owner,
            caller_kind: "user",
            surface: "mcp",
            tool: "search",
            op: Some("grep"),
            purpose: Some(purpose),
            space_name: None,
            input: &input,
            response: None,
            outcome: "success",
            error_code: None,
            duration_ms: 1,
        })
        .await?;
    }
    for purpose in ["cli first", "cli second"] {
        repo.insert(NewCommandInvocation {
            invocation_id: None,
            owner_user_id: owner,
            actor_account_id: owner,
            caller_kind: "user",
            surface: "cli",
            tool: "search",
            op: Some("find"),
            purpose: Some(purpose),
            space_name: None,
            input: &input,
            response: None,
            outcome: "success",
            error_code: None,
            duration_ms: 1,
        })
        .await?;
    }
    repo.insert(NewCommandInvocation {
        invocation_id: None,
        owner_user_id: other,
        actor_account_id: other,
        caller_kind: "user",
        surface: "mcp",
        tool: "search",
        op: Some("find"),
        purpose: Some("must stay private"),
        space_name: None,
        input: &input,
        response: None,
        outcome: "success",
        error_code: None,
        duration_ms: 1,
    })
    .await?;

    let first_page = repo
        .list_by_owner(owner, CommandInvocationSurface::Mcp, 2, None)
        .await?;
    assert_eq!(first_page.len(), 2);
    let mut first_page_items = first_page.iter();
    let newest = first_page_items.next().expect("newest invocation");
    let next = first_page_items.next().expect("next invocation");
    assert_eq!(newest.purpose.as_deref(), Some("third"));
    assert_eq!(next.purpose.as_deref(), Some("second"));
    assert!(newest.response.is_none());

    let cursor = CommandInvocationCursor {
        created_at: next.created_at,
        id: next.id,
        surface: CommandInvocationSurface::Mcp,
    };
    let second_page = repo
        .list_by_owner(owner, CommandInvocationSurface::Mcp, 2, Some(&cursor))
        .await?;
    assert_eq!(second_page.len(), 1);
    assert_eq!(
        second_page
            .first()
            .expect("remaining invocation")
            .purpose
            .as_deref(),
        Some("first")
    );
    assert!(
        first_page
            .iter()
            .chain(&second_page)
            .all(|item| item.purpose.as_deref() != Some("must stay private"))
    );

    let cli_page = repo
        .list_by_owner(owner, CommandInvocationSurface::Cli, 10, None)
        .await?;
    assert_eq!(cli_page.len(), 2);
    let mut cli_items = cli_page.iter();
    assert_eq!(
        cli_items
            .next()
            .expect("newest CLI invocation")
            .purpose
            .as_deref(),
        Some("cli second")
    );
    assert_eq!(
        cli_items
            .next()
            .expect("older CLI invocation")
            .purpose
            .as_deref(),
        Some("cli first")
    );
    assert!(cli_page.iter().all(|item| item.surface == "cli"));

    db.cleanup().await;
    Ok(())
}
