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
use notegate_model::{ConnectAgent, CreateAgent, CreateSpace, Permission};
use notegate_service::connections::ConnectionService;
use notegate_service::files::FilesService;
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
                recursive: false,
            },
        )
        .await
        .expect("delete folder");
    db.cleanup().await;
    Ok(())
}
