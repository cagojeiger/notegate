//! Integration tests for node search and Text encryption policy.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, insert_user_account, setup_space};
use notegate_db::{AgentRepo, ConnectionRepo, FilesRepo, SpaceRepo};
use notegate_model::{
    AccountKind, Channel, ConnectAgent, CreateAgent, Permission, TextAtRestEncryption, UpdateSpace,
};
use notegate_search::{GrepLineMode, GrepMatchMode, GrepRequest, SearchService};
use notegate_service::ServiceError;
use notegate_service::connections::ConnectionService;
use notegate_service::files::{
    CreateText, FilesService, ReadText, ReadTextBody, UpdateNodeExternalAccessPolicy,
    UpdateTextEncryption, WriteTarget, WriteText, WriteTextBody,
};

#[tokio::test]
async fn disabled_external_access_preserves_browser_access()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let owner = insert_user_account(&db.pool, "access-owner", "access-owner@example.test").await?;
    let (space, root) = setup_space(&SpaceRepo::new(db.pool.clone()), owner, "node-access").await;
    let created = files
        .create_text(
            owner,
            space,
            CreateText {
                parent_node_id: root,
                name: "private.md".to_owned(),
            },
        )
        .await?;
    let node_id = created.node.node.id;
    files
        .update_node_external_access_policy(
            AccountKind::User,
            owner,
            space,
            UpdateNodeExternalAccessPolicy {
                node_id,
                enabled: false,
            },
        )
        .await?;

    for channel in [Channel::Mcp, Channel::Api] {
        let external = files.for_channel(channel);
        assert!(matches!(
            external.stat(owner, space, node_id).await,
            Err(ServiceError::NotFound(_))
        ));
        assert!(matches!(
            external.resolve_path(owner, space, "/private.md").await,
            Err(ServiceError::NotFound(_))
        ));
        assert!(matches!(
            external
                .read_text(
                    owner,
                    space,
                    ReadText {
                        node_id,
                        start_line: None,
                        max_lines: None,
                        max_bytes: None,
                        if_none_match_sha256: Some(created.text.content_sha256.clone()),
                    }
                )
                .await,
            Err(ServiceError::NotFound(_))
        ));
        assert!(matches!(
            external
                .write_text(
                    owner,
                    space,
                    WriteText {
                        target: WriteTarget::Existing { node_id },
                        body: WriteTextBody::Plain("must not be saved".to_owned()),
                        expected_sha256: None,
                    }
                )
                .await,
            Err(ServiceError::NotFound(_))
        ));
    }

    let browser = files.for_channel(Channel::Browser);
    let read = browser
        .read_text(
            owner,
            space,
            ReadText {
                node_id,
                start_line: None,
                max_lines: None,
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await?;
    assert!(matches!(read.body, ReadTextBody::Content(ref content) if content.content.is_empty()));
    browser
        .update_node_external_access_policy(
            AccountKind::User,
            owner,
            space,
            UpdateNodeExternalAccessPolicy {
                node_id,
                enabled: true,
            },
        )
        .await?;
    for channel in [Channel::Mcp, Channel::Api] {
        assert!(
            files
                .for_channel(channel)
                .stat(owner, space, node_id)
                .await
                .is_ok()
        );
    }
    Ok(())
}

#[tokio::test]
async fn text_encryption_toggle_rewrites_existing_content_immediately()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let ws_repo = SpaceRepo::new(db.pool.clone());
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let owner = insert_user_account(&db.pool, "toggle-owner", "toggle-owner@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "toggle-encryption").await;

    let text = files
        .create_text(
            owner,
            ws,
            CreateText {
                parent_node_id: root,
                name: "existing.md".to_owned(),
            },
        )
        .await?;
    let node_id = text.node.node.id;
    files
        .write_text(
            owner,
            ws,
            WriteText {
                target: WriteTarget::Existing { node_id },
                body: WriteTextBody::Plain("existing plaintext".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;
    let denied = files
        .update_text_encryption(
            AccountKind::User,
            owner,
            ws,
            UpdateTextEncryption {
                node_id,
                enabled: true,
            },
        )
        .await
        .expect_err("tier0 cannot enable text encryption");
    assert!(matches!(
        denied,
        ServiceError::Conflict(message)
            if message == "text encryption is not available for the space owner's tier"
    ));

    sqlx::query("UPDATE users SET tier = 'system_max' WHERE id = $1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    files
        .update_text_encryption(
            AccountKind::User,
            owner,
            ws,
            UpdateTextEncryption {
                node_id,
                enabled: true,
            },
        )
        .await?;
    let immediately_encrypted: (Option<String>, Option<Vec<u8>>, String) = sqlx::query_as(
        "SELECT content_text, content_ciphertext, at_rest_encryption \
             FROM text_objects WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(node_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(immediately_encrypted.0.is_none());
    assert!(immediately_encrypted.1.is_some());
    assert_eq!(immediately_encrypted.2, "server");

    sqlx::query("UPDATE users SET tier = 'tier0' WHERE id = $1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    files
        .update_text_encryption(
            AccountKind::User,
            owner,
            ws,
            UpdateTextEncryption {
                node_id,
                enabled: false,
            },
        )
        .await?;
    let decrypted: (Option<String>, Option<Vec<u8>>, String) = sqlx::query_as(
        "SELECT content_text, content_ciphertext, at_rest_encryption \
         FROM text_objects WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(node_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(decrypted.0.as_deref(), Some("existing plaintext"));
    assert!(decrypted.1.is_none());
    assert_eq!(decrypted.2, "none");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn write_agent_cannot_change_node_settings() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let ws_repo = SpaceRepo::new(db.pool.clone());
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let owner = insert_user_account(
        &db.pool,
        "agent-encryption-owner",
        "agent-encryption-owner@example.test",
    )
    .await?;
    sqlx::query("UPDATE users SET tier = 'system_max' WHERE id = $1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    let (ws, root) = setup_space(&ws_repo, owner, "agent-encryption").await;
    let agent = AgentRepo::new(db.pool.clone())
        .insert_agent(
            &CreateAgent {
                name: "encryption-writer".to_owned(),
            },
            owner,
        )
        .await?
        .id;
    ConnectionService::new(ConnectionRepo::new(db.pool.clone()))
        .connect(
            AccountKind::User,
            owner,
            ConnectAgent {
                space_id: ws,
                agent_id: agent,
                permission: Permission::Write,
            },
        )
        .await?;

    let text = files
        .create_text(
            owner,
            ws,
            CreateText {
                parent_node_id: root,
                name: "owner-controlled.md".to_owned(),
            },
        )
        .await?;
    let node_id = text.node.node.id;

    let search = files
        .update_node_external_access_policy(
            AccountKind::Agent,
            agent,
            ws,
            UpdateNodeExternalAccessPolicy {
                node_id,
                enabled: false,
            },
        )
        .await;
    assert!(matches!(search, Err(ServiceError::Forbidden(_))));

    let enable = files
        .update_text_encryption(
            AccountKind::Agent,
            agent,
            ws,
            UpdateTextEncryption {
                node_id,
                enabled: true,
            },
        )
        .await;
    assert!(matches!(enable, Err(ServiceError::Forbidden(_))));

    files
        .update_text_encryption(
            AccountKind::User,
            owner,
            ws,
            UpdateTextEncryption {
                node_id,
                enabled: true,
            },
        )
        .await?;

    let disable = files
        .update_text_encryption(
            AccountKind::Agent,
            agent,
            ws,
            UpdateTextEncryption {
                node_id,
                enabled: false,
            },
        )
        .await;
    assert!(matches!(disable, Err(ServiceError::Forbidden(_))));

    let stored: String = sqlx::query_scalar(
        "SELECT at_rest_encryption FROM text_objects WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(node_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(stored, "server");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn server_encrypted_text_stays_readable_and_searchable()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let ws_repo = SpaceRepo::new(db.pool.clone());
    let files_repo = FilesRepo::new(db.pool.clone());
    let files = FilesService::new(files_repo.clone());
    let search = SearchService::new(files_repo);
    let owner =
        insert_user_account(&db.pool, "encrypted-owner", "encrypted-owner@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "encrypted").await;

    sqlx::query("UPDATE users SET tier = 'system_max' WHERE id = $1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    ws_repo
        .update_space_with_features(
            owner,
            &UpdateSpace {
                space_id: ws,
                name: None,
                sort_order: None,
                navigation_pinned: None,
                user_mcp_enabled: None,
                default_external_access_enabled: None,
                default_text_encryption_enabled: Some(true),
            },
        )
        .await?;

    let encrypted = files
        .create_text(
            owner,
            ws,
            CreateText {
                parent_node_id: root,
                name: "secret.md".to_owned(),
            },
        )
        .await?;
    let node_id = encrypted.node.node.id;
    files
        .write_text(
            owner,
            ws,
            WriteText {
                target: WriteTarget::Existing { node_id },
                body: WriteTextBody::Plain("searchable secret".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;

    let stored: (Option<String>, Option<Vec<u8>>, String) = sqlx::query_as(
        "SELECT content_text, content_ciphertext, at_rest_encryption \
         FROM text_objects WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(node_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(stored.0.is_none());
    assert!(stored.1.is_some());
    assert_eq!(stored.2, "server");

    let read = files
        .read_text(
            owner,
            ws,
            ReadText {
                node_id,
                start_line: None,
                max_lines: None,
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await?;
    let ReadTextBody::Content(content) = read.body else {
        panic!("server-encrypted text must be returned as plain content");
    };
    assert_eq!(content.content, "searchable secret");
    assert_eq!(
        read.node.text.expect("text stats").at_rest_encryption,
        TextAtRestEncryption::Server
    );

    let grep = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "searchable secret".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(grep.items.len(), 1);
    assert_eq!(grep.items[0].node.node.id, node_id);

    files
        .update_node_external_access_policy(
            AccountKind::User,
            owner,
            ws,
            UpdateNodeExternalAccessPolicy {
                node_id,
                enabled: false,
            },
        )
        .await?;
    let hidden = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "searchable secret".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert!(hidden.items.is_empty());
    let still_encrypted: String = sqlx::query_scalar(
        "SELECT at_rest_encryption FROM text_objects WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(node_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(still_encrypted, "server");
    files
        .update_node_external_access_policy(
            AccountKind::User,
            owner,
            ws,
            UpdateNodeExternalAccessPolicy {
                node_id,
                enabled: true,
            },
        )
        .await?;

    sqlx::query("UPDATE users SET tier = 'tier0' WHERE id = $1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    let read_after_downgrade = files
        .read_text(
            owner,
            ws,
            ReadText {
                node_id,
                start_line: None,
                max_lines: None,
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await?;
    assert!(matches!(
        read_after_downgrade.body,
        ReadTextBody::Content(ref content) if content.content == "searchable secret"
    ));
    let grep_after_downgrade = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "searchable secret".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(grep_after_downgrade.items.len(), 1);
    assert_eq!(grep_after_downgrade.items[0].node.node.id, node_id);
    let rejected_write = files
        .write_text(
            owner,
            ws,
            WriteText {
                target: WriteTarget::Existing { node_id },
                body: WriteTextBody::Plain("blocked after downgrade".to_owned()),
                expected_sha256: None,
            },
        )
        .await
        .expect_err("downgraded owner cannot create a new encrypted revision");
    assert!(matches!(
        rejected_write,
        ServiceError::Conflict(message)
            if message == "text encryption is not available for the space owner's tier"
    ));

    db.cleanup().await;
    Ok(())
}
