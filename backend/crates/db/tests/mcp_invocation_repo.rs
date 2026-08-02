//! Integration tests for append-only MCP invocation history.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::unwrap_in_result)]
mod common;

use common::{TestDb, insert_user_account};
use notegate_db::{McpInvocationRepo, NewMcpInvocation};
use notegate_model::McpInvocationCursor;

#[tokio::test]
async fn insert_records_only_the_bounded_invocation_summary()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "mcp-history", "mcp-history@example.test").await?;
    let repo = McpInvocationRepo::new(db.pool.clone());

    repo.insert(NewMcpInvocation {
        owner_user_id: owner,
        actor_account_id: owner,
        caller_kind: "user",
        tool: "search",
        op: Some("grep"),
        purpose: Some("find the cache design notes"),
        outcome: "error",
        error_code: Some("not_found"),
        duration_ms: 17,
    })
    .await?;

    let row = sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid, String, String, Option<String>, Option<String>, String, Option<String>, i64)>(
        "SELECT owner_user_id, actor_account_id, caller_kind, tool, op, purpose, outcome, error_code, duration_ms \
         FROM mcp_invocations ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(row.0, owner);
    assert_eq!(row.1, owner);
    assert_eq!(row.2, "user");
    assert_eq!(row.3, "search");
    assert_eq!(row.4.as_deref(), Some("grep"));
    assert_eq!(row.5.as_deref(), Some("find the cache design notes"));
    assert_eq!(row.6, "error");
    assert_eq!(row.7.as_deref(), Some("not_found"));
    assert_eq!(row.8, 17);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn me_is_the_only_tool_allowed_without_a_purpose() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "mcp-me", "mcp-me@example.test").await?;
    let repo = McpInvocationRepo::new(db.pool.clone());

    repo.insert(NewMcpInvocation {
        owner_user_id: owner,
        actor_account_id: owner,
        caller_kind: "user",
        tool: "me",
        op: None,
        purpose: None,
        outcome: "success",
        error_code: None,
        duration_ms: 0,
    })
    .await?;

    let missing_search_purpose = repo
        .insert(NewMcpInvocation {
            owner_user_id: owner,
            actor_account_id: owner,
            caller_kind: "user",
            tool: "search",
            op: Some("find"),
            purpose: None,
            outcome: "success",
            error_code: None,
            duration_ms: 0,
        })
        .await;
    assert!(missing_search_purpose.is_err());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn list_by_owner_is_newest_first_scoped_and_cursor_paginated()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "mcp-list", "mcp-list@example.test").await?;
    let other = insert_user_account(&db.pool, "mcp-other", "mcp-other@example.test").await?;
    let repo = McpInvocationRepo::new(db.pool.clone());

    for purpose in ["first", "second", "third"] {
        repo.insert(NewMcpInvocation {
            owner_user_id: owner,
            actor_account_id: owner,
            caller_kind: "user",
            tool: "search",
            op: Some("grep"),
            purpose: Some(purpose),
            outcome: "success",
            error_code: None,
            duration_ms: 1,
        })
        .await?;
    }
    repo.insert(NewMcpInvocation {
        owner_user_id: other,
        actor_account_id: other,
        caller_kind: "user",
        tool: "search",
        op: Some("find"),
        purpose: Some("must stay private"),
        outcome: "success",
        error_code: None,
        duration_ms: 1,
    })
    .await?;

    let first_page = repo.list_by_owner(owner, 2, None).await?;
    assert_eq!(first_page.len(), 2);
    let mut first_page_items = first_page.iter();
    let newest = first_page_items.next().expect("newest invocation");
    let next = first_page_items.next().expect("next invocation");
    assert_eq!(newest.purpose.as_deref(), Some("third"));
    assert_eq!(next.purpose.as_deref(), Some("second"));

    let cursor = McpInvocationCursor {
        created_at: next.created_at,
        id: next.id,
    };
    let second_page = repo.list_by_owner(owner, 2, Some(&cursor)).await?;
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

    db.cleanup().await;
    Ok(())
}
