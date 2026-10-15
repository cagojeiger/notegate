#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use axum::Router;
use axum::extract::Extension;
use axum::http::StatusCode;
use notegate_db::{AccountRepo, AgentRepo, ConnectionRepo, SpaceRepo, test_support::TestDb};
use notegate_model::files::{BeginObjectUpload, ObjectUploadMode, ObjectUploadRegistration};
use notegate_model::{
    Caller, CallerIdentity, Channel, ConnectAgent, FileEncryptionMode, Permission, ResolveAttrs,
};
use notegate_service::agents::CreateAgent;
use notegate_service::spaces::CreateSpace;
use serde_json::json;
use uuid::Uuid;

use crate::rest::test_support::{caller_and_space, empty_request, get_json, json_request, state};

mod invocation;

fn app(state: crate::state::AppState, caller: Caller) -> Router {
    Router::new()
        .merge(super::routes(state.clone()))
        .layer(Extension(caller))
        .with_state(state)
}

async fn agent_caller(
    state: &crate::state::AppState,
    owner_id: uuid::Uuid,
    space_id: uuid::Uuid,
    permission: Permission,
) -> Result<Caller, Box<dyn std::error::Error>> {
    let agent = AgentRepo::new(state.db.clone())
        .insert_agent(
            &CreateAgent {
                name: format!("v2-{}-agent", permission.as_str()),
            },
            owner_id,
        )
        .await?;
    ConnectionRepo::new(state.db.clone())
        .upsert_connection(
            &ConnectAgent {
                space_id,
                agent_id: agent.id,
                permission,
            },
            owner_id,
        )
        .await?;
    let account = AccountRepo::with_crypto_and_default_user_tier(
        state.db.clone(),
        state.security.clone(),
        state.config.default_user_tier,
    )
    .find_account(agent.id)
    .await?
    .expect("agent account");
    Ok(Caller {
        account,
        identity: CallerIdentity::Agent(agent),
        channel: Channel::Api,
    })
}

#[tokio::test]
async fn empty_children_parent_tracks_direct_and_inherited_write_locks()
-> Result<(), Box<dyn std::error::Error>> {
    use notegate_service::files::{CreateFolder, UpdateNodeWriteLock};

    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    sqlx::query("UPDATE users SET tier = 'system_max' WHERE id = $1")
        .bind(owner.account_id())
        .execute(&db.pool)
        .await?;
    let caller = agent_caller(&state, owner.account_id(), space_id, Permission::Read).await?;
    let outer = state
        .files
        .create_folder(
            owner.account_id(),
            space_id,
            CreateFolder {
                parent_node_id: root_id,
                name: "outer".to_owned(),
            },
        )
        .await?;
    let inner = state
        .files
        .create_folder(
            owner.account_id(),
            space_id,
            CreateFolder {
                parent_node_id: outer.node.id,
                name: "inner".to_owned(),
            },
        )
        .await?;
    let outer_id = outer.node.id;
    let inner_id = inner.node.id;

    for (target, enabled, direct, sources) in [
        (None, false, false, vec![]),
        (Some(inner_id), true, true, vec![inner_id]),
        (Some(outer_id), true, true, vec![outer_id, inner_id]),
        (Some(inner_id), false, false, vec![outer_id]),
        (Some(outer_id), false, false, vec![]),
    ] {
        if let Some(node_id) = target {
            state
                .files
                .update_node_write_lock(
                    &owner,
                    space_id,
                    UpdateNodeWriteLock { node_id, enabled },
                )
                .await?;
        }
        let (status, response) = get_json(
            app(state.clone(), caller.clone()),
            format!("/spaces/{space_id}/nodes/{inner_id}/children"),
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{response}");
        assert_eq!(response["children"], json!([]));
        assert_eq!(response["parent"]["has_children"], false);
        assert_eq!(response["parent"]["write_locked"], direct);
        assert_eq!(
            response["parent"]["effective_write_locked"],
            !sources.is_empty()
        );
        let actual_sources = response["parent"]["write_lock_sources"]
            .as_array()
            .expect("parent lock sources")
            .iter()
            .map(|source| source["node_id"].clone())
            .collect::<Vec<_>>();
        assert_eq!(json!(actual_sources), json!(sources));
        assert_eq!(response["page"]["returned"], 0);
        assert_eq!(response["page"]["has_more"], false);
        assert!(response["page"]["next_cursor"].is_null());

        let (status, detail) = get_json(
            app(state.clone(), caller.clone()),
            format!("/spaces/{space_id}/nodes/{inner_id}"),
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{detail}");
        assert_eq!(response["parent"], detail);
    }

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn paged_children_keep_parent_and_child_write_locks_independent()
-> Result<(), Box<dyn std::error::Error>> {
    use notegate_service::files::{CreateFolder, CreateText, UpdateNodeWriteLock};

    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    sqlx::query("UPDATE users SET tier = 'system_max' WHERE id = $1")
        .bind(owner.account_id())
        .execute(&db.pool)
        .await?;
    let caller = agent_caller(&state, owner.account_id(), space_id, Permission::Read).await?;
    let parent = state
        .files
        .create_folder(
            owner.account_id(),
            space_id,
            CreateFolder {
                parent_node_id: root_id,
                name: "notes".to_owned(),
            },
        )
        .await?;
    let parent_id = parent.node.id;
    let mut children = Vec::new();
    for name in ["a.md", "b.md"] {
        let child = state
            .files
            .create_text(
                owner.account_id(),
                space_id,
                CreateText {
                    parent_node_id: parent_id,
                    name: name.to_owned(),
                },
            )
            .await?;
        children.push(child.node.node.id);
    }
    state
        .files
        .update_node_write_lock(
            &owner,
            space_id,
            UpdateNodeWriteLock {
                node_id: children[0],
                enabled: true,
            },
        )
        .await?;

    for parent_locked in [false, true] {
        state
            .files
            .update_node_write_lock(
                &owner,
                space_id,
                UpdateNodeWriteLock {
                    node_id: parent_id,
                    enabled: parent_locked,
                },
            )
            .await?;
        let mut cursor: Option<String> = None;
        for (index, child_id) in children.iter().enumerate() {
            let mut uri = format!("/spaces/{space_id}/nodes/{parent_id}/children?limit=1");
            if let Some(cursor) = cursor.as_deref() {
                uri.push_str(&format!("&cursor={cursor}"));
            }
            let (status, response) = get_json(app(state.clone(), caller.clone()), uri).await?;
            assert_eq!(status, StatusCode::OK, "{response}");
            assert_eq!(response["parent"]["id"], parent_id.to_string());
            assert_eq!(response["parent"]["has_children"], true);
            assert_eq!(response["parent"]["write_locked"], parent_locked);
            assert_eq!(response["parent"]["effective_write_locked"], parent_locked);
            let sources = response["parent"]["write_lock_sources"]
                .as_array()
                .expect("parent lock sources");
            assert_eq!(sources.len(), usize::from(parent_locked));
            if parent_locked {
                assert_eq!(sources[0]["node_id"], parent_id.to_string());
            }
            assert_eq!(response["children"].as_array().map(Vec::len), Some(1));
            assert_eq!(response["children"][0]["id"], child_id.to_string());
            assert_eq!(
                response["children"][0]["effective_write_locked"],
                parent_locked || index == 0
            );
            assert_eq!(response["page"]["limit"], 1);
            assert_eq!(response["page"]["returned"], 1);
            assert_eq!(response["page"]["has_more"], index == 0);
            cursor = response["page"]["next_cursor"].as_str().map(str::to_owned);
            assert_eq!(cursor.is_some(), index == 0);
        }
    }

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn private_text_replace_checks_access_before_hash() -> Result<(), Box<dyn std::error::Error>>
{
    use notegate_model::AccountKind;
    use notegate_service::files::{CreateText, UpdateNodeExternalAccessPolicy};

    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    let caller = agent_caller(&state, owner.account_id(), space_id, Permission::Write).await?;
    let node = state
        .files
        .create_text(
            owner.account_id(),
            space_id,
            CreateText {
                parent_node_id: root_id,
                name: "private.md".to_owned(),
            },
        )
        .await?;
    state
        .files
        .update_node_external_access_policy(
            AccountKind::User,
            owner.account_id(),
            space_id,
            UpdateNodeExternalAccessPolicy {
                node_id: node.node.node.id,
                enabled: false,
            },
        )
        .await?;
    for expected_sha256 in [
        None,
        Some("0".repeat(64)),
        Some(node.text.content_sha256.clone()),
    ] {
        let (status, response) = json_request(
            app(state.clone(), caller.clone()),
            "PUT",
            format!("/spaces/{space_id}/text/{}", node.node.node.id),
            json!({"content": "replacement", "expected_sha256": expected_sha256}),
        )
        .await?;
        assert_eq!(status, StatusCode::NOT_FOUND, "{response}");
        assert_eq!(response["error"], "not_found");
    }
    assert_eq!(
        state
            .files
            .stat(owner.account_id(), space_id, node.node.node.id)
            .await?
            .text
            .expect("text stats")
            .content_sha256,
        node.text.content_sha256
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn connected_write_agent_can_use_v2_resource_flow() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    let caller = agent_caller(&state, owner.account_id(), space_id, Permission::Write).await?;

    let (status, spaces) = get_json(app(state.clone(), caller.clone()), "/spaces".into()).await?;
    assert_eq!(status, StatusCode::OK, "{spaces}");
    assert_eq!(spaces["spaces"].as_array().map(Vec::len), Some(1));

    let (status, created) = json_request(
        app(state.clone(), caller.clone()),
        "POST",
        format!("/spaces/{space_id}/nodes"),
        json!({
            "parent_id": root_id,
            "name": "agent-note.md",
            "kind": "text",
            "content": "first line\nneedle\n",
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let node_id = created["id"].as_str().expect("created node id");

    let (status, read) = get_json(
        app(state.clone(), caller.clone()),
        format!("/spaces/{space_id}/text/{node_id}"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["text"]["content"], "first line\nneedle\n");

    for operation in ["find", "grep"] {
        let (status, body) = json_request(
            app(state.clone(), caller.clone()),
            "POST",
            format!("/spaces/{space_id}/search/{operation}"),
            json!({"q": "needle"}),
        )
        .await?;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }

    let (status, deleted) = empty_request(
        app(state, caller),
        "DELETE",
        format!("/spaces/{space_id}/nodes/{node_id}"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{deleted}");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn copy_rejects_a_folder_containing_a_file() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    let caller = agent_caller(&state, owner.account_id(), space_id, Permission::Write).await?;

    let (status, folder) = json_request(
        app(state.clone(), caller.clone()),
        "POST",
        format!("/spaces/{space_id}/nodes"),
        json!({
            "parent_id": root_id,
            "name": "assets",
            "kind": "folder",
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{folder}");
    let folder_id = Uuid::parse_str(folder["id"].as_str().expect("folder id"))?;

    let upload_id = Uuid::new_v4();
    let upload = BeginObjectUpload {
        parent_node_id: folder_id,
        name: "diagram.bin".to_owned(),
        byte_len: 11,
        media_type: "application/octet-stream".to_owned(),
        original_filename: None,
        encryption_mode: FileEncryptionMode::None,
        encryption_metadata: None,
    };
    state
        .files
        .prepare_object_upload(caller.account_id(), space_id, &upload)
        .await?;
    state
        .files
        .record_registered_object_upload(
            &ObjectUploadRegistration {
                id: upload_id,
                object_key: format!("objects/{upload_id}"),
                upload_mode: ObjectUploadMode::Single,
                multipart_upload_id: None,
                multipart_part_size: None,
            },
            caller.account_id(),
            space_id,
            &upload,
        )
        .await?;
    state
        .files
        .complete_object_upload(caller.account_id(), space_id, upload_id, None, None)
        .await?;

    let (status, conflict) = json_request(
        app(state, caller),
        "POST",
        format!("/spaces/{space_id}/nodes/{folder_id}/copy"),
        json!({
            "new_parent_id": root_id,
            "new_name": "assets-copy",
            "recursive": true,
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"], "conflict");
    assert_eq!(conflict["message"], "copy does not support file nodes");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn v2_respects_connection_permission_and_space_visibility()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    let caller = agent_caller(&state, owner.account_id(), space_id, Permission::Read).await?;

    let (status, denied) = json_request(
        app(state.clone(), caller.clone()),
        "POST",
        format!("/spaces/{space_id}/nodes"),
        json!({
            "parent_id": root_id,
            "name": "denied.md",
            "kind": "text",
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");

    let (hidden_owner, _) = AccountRepo::with_crypto_and_default_user_tier(
        state.db.clone(),
        state.security.clone(),
        state.config.default_user_tier,
    )
    .upsert_user_by_sub(&ResolveAttrs {
        sub: "v2-hidden-owner".to_owned(),
        email: "v2-hidden-owner@example.test".to_owned(),
        name: "V2 Hidden Owner".to_owned(),
    })
    .await?;
    let hidden = SpaceRepo::new(state.db.clone())
        .create_space(
            hidden_owner.id,
            &CreateSpace {
                name: "v2-hidden".to_owned(),
            },
        )
        .await?;
    let (status, hidden_response) =
        get_json(app(state.clone(), caller), format!("/spaces/{}", hidden.id)).await?;
    assert_eq!(status, StatusCode::NOT_FOUND, "{hidden_response}");
    let calls = state
        .command_invocations
        .list_by_owner(
            owner.account_id(),
            notegate_model::CommandInvocationSurface::Api,
            10,
            None,
        )
        .await?;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].error_code.as_deref(), Some("http_404"));
    assert_eq!(calls[1].error_code.as_deref(), Some("http_403"));
    assert!(
        state
            .command_invocations
            .list_by_owner(
                hidden_owner.id,
                notegate_model::CommandInvocationSurface::Api,
                10,
                None,
            )
            .await?
            .is_empty(),
        "denied calls belong to the caller owner, not the target owner"
    );

    db.cleanup().await;
    Ok(())
}
