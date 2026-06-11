mod common;

use common::TestDb;
use notegate_db::{ConnectionRepo, SpaceRepo};
use notegate_model::{ConnectAgent, CreateAgent, CreateSpace, Permission};
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
async fn create_space_creates_root_and_owner_write_permission()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = create_user(&db, "owner-space@example.com").await?;
    let repo = SpaceRepo::new(db.pool.clone());

    let space = repo
        .create_space(
            owner,
            &CreateSpace {
                name: "personal".to_owned(),
            },
        )
        .await?;
    let root = repo
        .root_node_id(space.id)
        .await?
        .ok_or("root node missing")?;

    assert_eq!(space.owner_user_id, owner);
    assert_eq!(
        repo.permission_for(space.id, owner).await?,
        Some(Permission::Write)
    );

    let (kind, parent_id): (String, Option<Uuid>) =
        sqlx::query_as("SELECT kind, parent_id FROM nodes WHERE id = $1")
            .bind(root)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(kind, "folder");
    assert_eq!(parent_id, None);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn agent_connection_grants_and_disconnects_permission()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = create_user(&db, "owner-connect@example.com").await?;
    let space_repo = SpaceRepo::new(db.pool.clone());
    let connection_repo = ConnectionRepo::new(db.pool.clone());
    let agent = create_agent(&db, owner, "assistant").await?;
    let space = space_repo
        .create_space(
            owner,
            &CreateSpace {
                name: "lab".to_owned(),
            },
        )
        .await?;

    let connection = connection_repo
        .upsert_connection(
            &ConnectAgent {
                space_id: space.id,
                agent_id: agent,
                permission: Permission::Read,
            },
            owner,
        )
        .await?;

    assert_eq!(connection.permission, Permission::Read);
    assert_eq!(
        space_repo.permission_for(space.id, agent).await?,
        Some(Permission::Read)
    );

    connection_repo.disconnect(space.id, agent, owner).await?;
    assert_eq!(space_repo.permission_for(space.id, agent).await?, None);
    db.cleanup().await;
    Ok(())
}
