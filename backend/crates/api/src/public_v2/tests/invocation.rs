use axum::body::{Body, to_bytes};
use axum::http::Request;
use notegate_model::{AccountKind, CommandInvocationSurface, ListCommandInvocations};
use tower::ServiceExt as _;

use super::*;
use crate::invocations::test_support::{assert_completed, capture};

#[tokio::test]
async fn summaries_are_encrypted_owner_scoped_and_linked_to_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space, root) = caller_and_space(&state).await?;
    let caller = agent_caller(&state, owner.account_id(), space, Permission::Write).await?;
    let app = Router::new()
        .nest("/api/v2", super::super::routes(state.clone()))
        .layer(Extension(caller.clone()))
        .with_state(state.clone());
    let create_purpose = "새 문서 작성";
    let update_purpose = "설계 보완";
    let created = app.clone().oneshot(Request::builder()
        .method("POST")
        .uri(format!("/api/v2/spaces/{space}/nodes"))
        .header("content-type", "application/json")
        .header(super::super::PURPOSE_HEADER, percent_encoding::utf8_percent_encode(create_purpose, percent_encoding::NON_ALPHANUMERIC).to_string())
        .body(Body::from(json!({"kind": "text", "parent_id": root, "name": "private-name.md", "content": "private-body"}).to_string()))?
    ).await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: serde_json::Value =
        serde_json::from_slice(&to_bytes(created.into_body(), usize::MAX).await?)?;
    let node = created["id"].as_str().expect("node id");
    let request = Request::builder()
        .uri(format!(
            "/api/v2/spaces/{space}/text/{node}?secret=private-query"
        ))
        .header("authorization", "Bearer private-token")
        .header("x-request-id", "untrusted-client-id")
        .body(Body::empty())?;
    let response = app.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(
        response["text"]["content"], "private-body",
        "logging preserves the response body"
    );
    let session = Uuid::new_v4();
    let updated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v2/spaces/{space}/text/{node}"))
                .header("content-type", "application/json")
                .header(
                    super::super::PURPOSE_HEADER,
                    percent_encoding::utf8_percent_encode(
                        update_purpose,
                        percent_encoding::NON_ALPHANUMERIC,
                    )
                    .to_string(),
                )
                .body(Body::from(
                    json!({"content": "replacement-private-body", "edit_session_id": session})
                        .to_string(),
                ))?,
        )
        .await?;
    assert_eq!(updated.status(), StatusCode::OK);

    let page = state
        .history
        .list_command_invocations(
            AccountKind::User,
            owner.account_id(),
            ListCommandInvocations {
                surface: CommandInvocationSurface::Api,
                limit: Some(10),
                cursor: None,
            },
        )
        .await?;
    assert_eq!(page.items.len(), 3);
    for row in &page.items {
        assert_eq!(row.actor_account_id, caller.account_id());
        assert_eq!(row.caller_kind, "agent");
        assert_eq!(row.surface, "api");
        assert_eq!(row.tool, "http");
        assert_eq!(row.input["path_ids"]["space_id"], space.to_string());
        assert_eq!(row.input["body_recorded"], false);
        assert_eq!(row.outcome, "success");
        assert!(row.invocation_id.is_some());
        let summary = serde_json::to_string(row)?;
        for secret in [
            "private-name",
            "private-body",
            "private-query",
            "private-token",
            "untrusted-client-id",
        ] {
            assert!(
                !summary.contains(secret),
                "unexpected sensitive field: {secret}"
            );
        }
    }
    let read = page
        .items
        .iter()
        .find(|row| row.op.as_deref() == Some("GET /api/v2/spaces/{space_id}/text/{node_id}"))
        .expect("read summary");
    assert_eq!(read.input["path_ids"]["node_id"], node);
    assert_eq!(read.purpose, None, "omitted purpose stays absent");
    assert_eq!(
        read.response,
        Some(json!({"status": 200, "body_recorded": false}))
    );
    let raw: (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE input='{}'::jsonb AND response IS NULL AND purpose IS NULL AND private_payload IS NOT NULL AND snapshot_id IS NOT NULL) FROM command_invocations"
    ).fetch_one(&db.pool).await?;
    assert_eq!(raw, (3, 3));
    let events = state
        .files
        .list_file_change_events(
            owner.account_id(),
            space,
            notegate_service::files::ListFileChangeEvents {
                node_id: Some(Uuid::parse_str(node)?),
                limit: Some(10),
                cursor: None,
            },
        )
        .await?
        .items;
    assert_eq!(events.len(), 2);
    for event in events {
        assert_eq!(event.metadata["source"], "api");
        let linked = page
            .items
            .iter()
            .find(|row| {
                row.invocation_id.map(|id| json!(id)).as_ref()
                    == Some(&event.metadata["invocation_id"])
            })
            .expect("each change belongs to a recorded API call");
        assert_ne!(linked.invocation_id, read.invocation_id);
        let expected = if linked.op.as_deref() == Some("POST /api/v2/spaces/{space_id}/nodes") {
            create_purpose
        } else {
            update_purpose
        };
        assert_eq!(linked.purpose.as_deref(), Some(expected));
        assert_eq!(event.metadata["purpose"], expected);
    }
    let revisions = state
        .files
        .text_revisions(owner.account_id(), space, Uuid::parse_str(node)?, 10, None)
        .await?;
    assert_eq!(
        revisions.current.expect("current body").purpose.as_deref(),
        Some(update_purpose)
    );
    assert_eq!(revisions.revisions.len(), 1);
    assert_eq!(
        revisions.revisions[0].purpose.as_deref(),
        Some(create_purpose)
    );
    let stored_session: Option<Uuid> =
        sqlx::query_scalar("SELECT revision_session_id FROM text_objects WHERE node_id=$1")
            .bind(Uuid::parse_str(node)?)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(stored_session, Some(session));
    for surface in [CommandInvocationSurface::Mcp, CommandInvocationSurface::Cli] {
        assert!(
            state
                .command_invocations
                .list_by_owner(owner.account_id(), surface, 10, None)
                .await?
                .is_empty()
        );
    }
    assert!(
        state
            .command_invocations
            .list_by_owner(Uuid::new_v4(), CommandInvocationSurface::Api, 10, None)
            .await?
            .is_empty()
    );
    assert!(
        state
            .history
            .list_command_invocations(
                AccountKind::Agent,
                caller.account_id(),
                ListCommandInvocations {
                    surface: CommandInvocationSurface::Api,
                    limit: None,
                    cursor: None
                },
            )
            .await
            .is_err()
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn history_failure_does_not_change_http_success_or_extractor_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let mut state = state(&db);
    std::sync::Arc::make_mut(&mut state.config).metrics_enabled = true;
    let (owner, space, _) = caller_and_space(&state).await?;
    let caller = agent_caller(&state, owner.account_id(), space, Permission::Read).await?;
    let app = app(state.clone(), caller);
    for history in ["success", "error"] {
        if history == "error" {
            sqlx::query("ALTER TABLE command_invocations ADD CONSTRAINT reject_history CHECK (false) NOT VALID")
                .execute(&db.pool).await?;
        }
        for (path, status, outcome, purpose) in [
            ("/me", StatusCode::OK, "success", Some("read%20caller")),
            ("/spaces/not-a-uuid", StatusCode::BAD_REQUEST, "error", None),
            ("/me", StatusCode::BAD_REQUEST, "error", Some("%FF")),
        ] {
            let mut request = Request::builder().uri(path);
            if let Some(purpose) = purpose {
                request = request.header(super::super::PURPOSE_HEADER, purpose);
            }
            let request = request.body(Body::empty())?;
            let (response, metrics) = capture(app.clone().oneshot(request)).await;
            assert_eq!(response?.status(), status);
            assert_completed(&metrics, outcome, history);
        }
    }
    let rows = state
        .command_invocations
        .list_by_owner(owner.account_id(), CommandInvocationSurface::Api, 10, None)
        .await?;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2].purpose.as_deref(), Some("read caller"));
    assert_eq!(rows[0].purpose, None);
    assert_eq!(rows[0].error_code.as_deref(), Some("http_400"));
    assert_eq!(rows[0].input["path_ids"], json!({}));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn invalid_purpose_records_failure_without_executing_the_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space, root) = caller_and_space(&state).await?;
    let caller = agent_caller(&state, owner.account_id(), space, Permission::Write).await?;
    let app = app(state.clone(), caller);
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes WHERE space_id=$1")
        .bind(space)
        .fetch_one(&db.pool)
        .await?;
    let response = app.oneshot(Request::builder()
        .method("POST")
        .uri(format!("/spaces/{space}/nodes"))
        .header("content-type", "application/json")
        .header(super::super::PURPOSE_HEADER, "%20private-invalid-purpose")
        .body(Body::from(json!({"kind": "text", "parent_id": root, "name": "should-not-exist.md", "content": "private-body"}).to_string()))?
    ).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes WHERE space_id=$1")
        .bind(space)
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(before, after);
    let changes: i64 =
        sqlx::query_scalar("SELECT count(*) FROM file_change_events WHERE space_id=$1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(changes, 0);
    let rows = state
        .command_invocations
        .list_by_owner(owner.account_id(), CommandInvocationSurface::Api, 10, None)
        .await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].error_code.as_deref(), Some("http_400"));
    assert_eq!(rows[0].purpose, None);
    let stored = serde_json::to_string(&rows[0])?;
    for secret in [
        "private-invalid-purpose",
        "private-body",
        "should-not-exist",
    ] {
        assert!(!stored.contains(secret));
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn browser_write_keeps_changes_and_revisions_without_invocation_history()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space, root) = caller_and_space(&state).await?;
    let node = state
        .files
        .create_text(
            owner.account_id(),
            space,
            notegate_service::files::CreateText {
                parent_node_id: root,
                name: "browser.md".to_owned(),
            },
        )
        .await?
        .node
        .node
        .id;
    let browser = Router::new()
        .merge(crate::rest::text::routes())
        .layer(Extension(owner.clone()))
        .with_state(state.clone());
    for (method, body) in [
        (
            "PUT",
            Body::from(json!({"content": "browser saved content"}).to_string()),
        ),
        ("GET", Body::empty()),
    ] {
        let response = browser
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!("/v1/spaces/{space}/text/{node}"))
                    .header("content-type", "application/json")
                    .header(super::super::PURPOSE_HEADER, "ignored%20on%20v1")
                    .body(body)?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let calls: i64 = sqlx::query_scalar("SELECT count(*) FROM command_invocations")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(calls, 0);
    let events = state
        .files
        .list_file_change_events(
            owner.account_id(),
            space,
            notegate_service::files::ListFileChangeEvents {
                node_id: Some(node),
                limit: Some(10),
                cursor: None,
            },
        )
        .await?
        .items;
    assert_eq!(events.len(), 2);
    for event in events {
        assert_eq!(event.metadata["source"], "browser");
        assert!(
            event
                .metadata
                .get("invocation_id")
                .is_none_or(serde_json::Value::is_null)
        );
        assert!(
            event
                .metadata
                .get("purpose")
                .is_none_or(serde_json::Value::is_null)
        );
    }
    let revisions = state
        .files
        .text_revisions(owner.account_id(), space, node, 10, None)
        .await?;
    assert_eq!(revisions.revisions.len(), 1);
    assert!(
        revisions
            .current
            .expect("current version")
            .purpose
            .is_none()
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn authentication_rejection_does_not_create_an_owned_api_history()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let response = crate::routes::app(state)
        .oneshot(Request::builder().uri("/api/v2/me").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM command_invocations")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(count, 0);
    db.cleanup().await;
    Ok(())
}
