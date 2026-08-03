//! Integration tests for append-only MCP invocation history.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::unwrap_in_result)]
mod common;

use common::{TestDb, insert_user_account};
use notegate_db::{McpInvocationRepo, NewMcpInvocation};
use notegate_model::McpInvocationCursor;

#[tokio::test]
async fn insert_records_invocation_summary_and_payloads() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "mcp-history", "mcp-history@example.test").await?;
    let repo = McpInvocationRepo::new(db.pool.clone());
    let input = serde_json::json!({
        "purpose": "review recent changes",
        "op": "changes",
        "target": "Research:/"
    });
    let response = serde_json::json!({
        "kind": "error",
        "error": {"code": "not_found"}
    });

    repo.insert(NewMcpInvocation {
        owner_user_id: owner,
        actor_account_id: owner,
        caller_kind: "user",
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

    let row = sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid, String, String, Option<String>, Option<String>, Option<String>, serde_json::Value, Option<serde_json::Value>, String, Option<String>, i64)>(
        "SELECT owner_user_id, actor_account_id, caller_kind, tool, op, purpose, space_name, input, response, outcome, error_code, duration_ms \
         FROM mcp_invocations ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(row.0, owner);
    assert_eq!(row.1, owner);
    assert_eq!(row.2, "user");
    assert_eq!(row.3, "read");
    assert_eq!(row.4.as_deref(), Some("changes"));
    assert_eq!(row.5.as_deref(), Some("review recent changes"));
    assert_eq!(row.6.as_deref(), Some("Research"));
    assert_eq!(row.7, input);
    assert_eq!(row.8.as_ref(), Some(&response));
    assert_eq!(row.9, "error");
    assert_eq!(row.10.as_deref(), Some("not_found"));
    assert_eq!(row.11, 17);
    let listed = repo.list_by_owner(owner, 1, None).await?;
    assert_eq!(
        listed.first().and_then(|item| item.response.as_ref()),
        Some(&response)
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
    let owner = insert_user_account(&db.pool, "mcp-me", "mcp-me@example.test").await?;
    let repo = McpInvocationRepo::new(db.pool.clone());
    let empty_input = serde_json::json!({});

    repo.insert(NewMcpInvocation {
        owner_user_id: owner,
        actor_account_id: owner,
        caller_kind: "user",
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

    repo.insert(NewMcpInvocation {
        owner_user_id: owner,
        actor_account_id: owner,
        caller_kind: "user",
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
            .insert(NewMcpInvocation {
                owner_user_id: owner,
                actor_account_id: owner,
                caller_kind: "user",
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
        .insert(NewMcpInvocation {
            owner_user_id: owner,
            actor_account_id: owner,
            caller_kind: "user",
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
        .insert(NewMcpInvocation {
            owner_user_id: owner,
            actor_account_id: owner,
            caller_kind: "user",
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
    let input = serde_json::json!({});

    for purpose in ["first", "second", "third"] {
        repo.insert(NewMcpInvocation {
            owner_user_id: owner,
            actor_account_id: owner,
            caller_kind: "user",
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
    repo.insert(NewMcpInvocation {
        owner_user_id: other,
        actor_account_id: other,
        caller_kind: "user",
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

    let first_page = repo.list_by_owner(owner, 2, None).await?;
    assert_eq!(first_page.len(), 2);
    let mut first_page_items = first_page.iter();
    let newest = first_page_items.next().expect("newest invocation");
    let next = first_page_items.next().expect("next invocation");
    assert_eq!(newest.purpose.as_deref(), Some("third"));
    assert_eq!(next.purpose.as_deref(), Some("second"));
    assert!(newest.response.is_none());

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
