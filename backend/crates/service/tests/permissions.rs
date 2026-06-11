#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::TestDb;
use notegate_db::{ConnectionRepo, FilesRepo, SpaceRepo};
use notegate_model::files::{CreateFolder, DeleteNode};
use notegate_model::{
    AccountKind, ConnectAgent, CreateAgent, CreateSpace, Permission, RenameSpace,
};
use notegate_service::connections::ConnectionService;
use notegate_service::files::FilesService;
use notegate_service::spaces::SpaceService;
use uuid::Uuid;

async fn create_user(db: &TestDb, email: &str) -> Result<Uuid, Box<dyn std::error::Error>> {
    let account_id = common::insert_user_account(&db.pool, email, email).await?;
    Ok(account_id)
}

async fn create_agent(
    db: &TestDb,
    owner: Uuid,
    name: &str,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let repo = notegate_db::AgentRepo::new(db.pool.clone());
    Ok(repo
        .insert_agent(
            &CreateAgent {
                name: name.to_owned(),
            },
            owner,
        )
        .await?
        .id)
}

#[tokio::test]
async fn connected_agent_read_cannot_mutate_files() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = create_user(&db, "svc-owner-read@example.com").await?;
    let agent = create_agent(&db, owner, "reader").await?;
    let space = SpaceRepo::new(db.pool.clone())
        .create_space(
            owner,
            &CreateSpace {
                name: "personal".to_owned(),
            },
        )
        .await?;
    let root = SpaceRepo::new(db.pool.clone())
        .root_node_id(space.id)
        .await?
        .expect("root");

    let connections = ConnectionService::new(ConnectionRepo::new(db.pool.clone()));
    connections
        .connect(
            AccountKind::User,
            owner,
            ConnectAgent {
                space_id: space.id,
                agent_id: agent,
                permission: Permission::Read,
            },
        )
        .await
        .expect("connect agent");

    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let result = files
        .create_folder(
            agent,
            space.id,
            CreateFolder {
                parent_node_id: root,
                name: "blocked".to_owned(),
            },
        )
        .await;
    assert!(result.is_err(), "read permission must not mutate");
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn connected_agent_write_can_mutate_files() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = create_user(&db, "svc-owner-write@example.com").await?;
    let agent = create_agent(&db, owner, "writer").await?;
    let space_repo = SpaceRepo::new(db.pool.clone());
    let space = space_repo
        .create_space(
            owner,
            &CreateSpace {
                name: "personal".to_owned(),
            },
        )
        .await?;
    let root = space_repo.root_node_id(space.id).await?.expect("root");

    ConnectionService::new(ConnectionRepo::new(db.pool.clone()))
        .connect(
            AccountKind::User,
            owner,
            ConnectAgent {
                space_id: space.id,
                agent_id: agent,
                permission: Permission::Write,
            },
        )
        .await
        .expect("connect agent");

    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let folder = files
        .create_folder(
            agent,
            space.id,
            CreateFolder {
                parent_node_id: root,
                name: "ok".to_owned(),
            },
        )
        .await
        .expect("create folder");
    files
        .delete_node(
            agent,
            space.id,
            DeleteNode {
                node_id: folder.node.id,
                recursive: true,
            },
        )
        .await
        .expect("delete folder");
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn connected_agent_write_cannot_manage_space_or_connections()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = create_user(&db, "svc-owner-manage@example.com").await?;
    let agent = create_agent(&db, owner, "manager-blocked").await?;
    let space_repo = SpaceRepo::new(db.pool.clone());
    let space = space_repo
        .create_space(
            owner,
            &CreateSpace {
                name: "personal".to_owned(),
            },
        )
        .await?;

    let connections = ConnectionService::new(ConnectionRepo::new(db.pool.clone()));
    connections
        .connect(
            AccountKind::User,
            owner,
            ConnectAgent {
                space_id: space.id,
                agent_id: agent,
                permission: Permission::Write,
            },
        )
        .await
        .expect("connect agent");

    let spaces = SpaceService::new(SpaceRepo::new(db.pool.clone()));
    let rename = spaces
        .rename(
            AccountKind::Agent,
            agent,
            RenameSpace {
                space_id: space.id,
                new_name: "renamed".to_owned(),
            },
        )
        .await;
    assert!(rename.is_err(), "agent must not rename spaces");

    let delete = spaces.delete(AccountKind::Agent, agent, space.id).await;
    assert!(delete.is_err(), "agent must not delete spaces");

    let list_connections = connections
        .list_page(AccountKind::Agent, agent, space.id, Default::default())
        .await;
    assert!(
        list_connections.is_err(),
        "agent must not list or manage space connections"
    );

    db.cleanup().await;
    Ok(())
}
