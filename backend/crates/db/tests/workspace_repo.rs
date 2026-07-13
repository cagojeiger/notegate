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

    let usage: (i64, i64, i64) = sqlx::query_as(
        "SELECT live_node_count, live_text_bytes, live_file_bytes \
         FROM space_usage WHERE space_id = $1",
    )
    .bind(space.id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(usage, (1, 0, 0));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn create_space_appends_sort_order() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = create_user(&db, "owner-space-append@example.com").await?;
    common::set_user_tier(&db.pool, owner, "system_max").await?;
    let repo = SpaceRepo::new(db.pool.clone());

    let first = repo
        .create_space(
            owner,
            &CreateSpace {
                name: "first".to_owned(),
            },
        )
        .await?;
    let second = repo
        .create_space(
            owner,
            &CreateSpace {
                name: "second".to_owned(),
            },
        )
        .await?;

    assert_eq!(first.sort_order, 1000);
    assert_eq!(second.sort_order, 2000);

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

#[tokio::test]
async fn list_spaces_uses_manual_order_name_id_cursor() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = create_user(&db, "owner-space-order@example.com").await?;
    common::set_user_tier(&db.pool, owner, "system_max").await?;
    let repo = SpaceRepo::new(db.pool.clone());

    let zeta = repo
        .create_space(
            owner,
            &CreateSpace {
                name: "zeta".to_owned(),
            },
        )
        .await?;
    repo.create_space(
        owner,
        &CreateSpace {
            name: "alpha".to_owned(),
        },
    )
    .await?;
    repo.update_space(zeta.id, owner, None, Some(-10)).await?;

    let first_page = repo.list_space_views_for(owner, 1, None).await?;
    let first = first_page.first().ok_or("first page missing")?;
    assert_eq!(first_page.len(), 1);
    assert_eq!(first.space.name, "zeta");
    assert_eq!(first.space.sort_order, -10);

    let cursor = notegate_model::SpaceCursor {
        sort_order: first.space.sort_order,
        name: first.space.name.clone(),
        id: first.space.id,
    };
    let second_page = repo.list_space_views_for(owner, 10, Some(&cursor)).await?;
    let names: Vec<_> = second_page
        .iter()
        .map(|view| view.space.name.as_str())
        .collect();
    assert_eq!(names, vec!["alpha"]);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn space_name_suggestion_lookup_is_case_insensitive_only()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = create_user(&db, "owner-space-suggest@example.com").await?;
    let other = create_user(&db, "other-space-suggest@example.com").await?;
    let repo = SpaceRepo::new(db.pool.clone());

    repo.create_space(
        owner,
        &CreateSpace {
            name: "Beringlab".to_owned(),
        },
    )
    .await?;

    let exact = repo
        .list_space_views_by_name_for(owner, "beringlab", 10)
        .await?;
    assert!(exact.is_empty(), "exact lookup must remain case-sensitive");

    let suggestions = repo
        .list_space_views_by_name_case_insensitive_for(owner, "beringlab", 10)
        .await?;
    let names: Vec<_> = suggestions
        .iter()
        .map(|view| view.space.name.as_str())
        .collect();
    assert_eq!(names, vec!["Beringlab"]);

    let inaccessible = repo
        .list_space_views_by_name_case_insensitive_for(other, "beringlab", 10)
        .await?;
    assert!(
        inaccessible.is_empty(),
        "suggestions must only include caller-accessible spaces"
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn tier0_limits_owner_to_one_space() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = create_user(&db, "owner-tier0@example.com").await?;
    common::set_user_tier(&db.pool, owner, "tier0").await?;
    let repo = SpaceRepo::new(db.pool.clone());

    repo.create_space(
        owner,
        &CreateSpace {
            name: "one".to_owned(),
        },
    )
    .await?;
    let result = repo
        .create_space(
            owner,
            &CreateSpace {
                name: "two".to_owned(),
            },
        )
        .await;
    let Err(err) = result else {
        return Err("tier0 should allow only one live space".into());
    };
    assert!(err.to_string().contains("maximum of 1 spaces"));

    db.cleanup().await;
    Ok(())
}
