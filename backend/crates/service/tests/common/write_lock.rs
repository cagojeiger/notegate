use std::error::Error;
use std::fmt::Debug;

use notegate_db::{AccountRepo, FilesRepo, SpaceRepo};
use notegate_model::files::UpdateNodeWriteLock;
use notegate_model::{Caller, CallerIdentity, Channel};
use notegate_service::ServiceError;
use notegate_service::files::{CreateFolder, CreateText, FilesService};
use notegate_service::spaces::CreateSpace;
use uuid::Uuid;

use crate::common::{TestDb, insert_user_account};

pub(crate) type TestResult<T = ()> = Result<T, Box<dyn Error>>;

pub(crate) struct Fixture {
    pub(crate) db: TestDb,
    pub(crate) owner: Uuid,
    pub(crate) space_id: Uuid,
    pub(crate) root_id: Uuid,
    pub(crate) files: FilesService,
    pub(crate) browser: Caller,
}

impl Fixture {
    pub(crate) async fn setup(label: &str) -> TestResult<Option<Self>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(None);
        };
        let username = format!("write-lock-{label}");
        let email = format!("{label}@example.test");
        let owner = insert_user_account(&db.pool, &username, &email).await?;
        set_system_max(&db, owner).await?;

        let spaces = SpaceRepo::new(db.pool.clone());
        let space = spaces
            .create_space(
                owner,
                &CreateSpace {
                    name: username.clone(),
                },
            )
            .await?;
        let root_id = spaces.root_node_id(space.id).await?.expect("root node");
        let browser = load_caller(&db, owner, Channel::Browser).await;

        Ok(Some(Self {
            files: FilesService::new(FilesRepo::new(db.pool.clone())),
            db,
            owner,
            space_id: space.id,
            root_id,
            browser,
        }))
    }

    pub(crate) async fn folder(&self, parent_node_id: Uuid, name: &str) -> TestResult<Uuid> {
        Ok(self
            .files
            .create_folder(
                self.owner,
                self.space_id,
                CreateFolder {
                    parent_node_id,
                    name: name.to_owned(),
                },
            )
            .await?
            .node
            .id)
    }

    pub(crate) async fn text(&self, parent_node_id: Uuid, name: &str) -> TestResult<Uuid> {
        Ok(self
            .files
            .create_text(
                self.owner,
                self.space_id,
                CreateText {
                    parent_node_id,
                    name: name.to_owned(),
                },
            )
            .await?
            .node
            .node
            .id)
    }

    pub(crate) async fn set_lock(&self, node_id: Uuid, enabled: bool) -> TestResult {
        self.files
            .update_node_write_lock(
                &self.browser,
                self.space_id,
                UpdateNodeWriteLock {
                    node_id,
                    enabled,
                    expected_revision: crate::common::node_revision(&self.db.pool, node_id).await?,
                },
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn cleanup(self) {
        self.db.cleanup().await;
    }
}

pub(crate) async fn load_caller(db: &TestDb, account_id: Uuid, channel: Channel) -> Caller {
    let (account, user) = AccountRepo::new(db.pool.clone())
        .find_caller_by_account_id(account_id)
        .await
        .expect("load caller")
        .expect("caller exists");
    Caller {
        account,
        identity: CallerIdentity::User(user),
        channel,
    }
}

pub(crate) fn assert_write_locked<T: Debug>(result: Result<T, ServiceError>) {
    assert!(
        matches!(&result, Err(ServiceError::WriteLocked { .. })),
        "expected write-lock conflict, got {result:?}"
    );
}

pub(crate) fn assert_forbidden<T: Debug>(result: Result<T, ServiceError>) {
    assert!(
        matches!(&result, Err(ServiceError::Forbidden(_))),
        "expected forbidden result, got {result:?}"
    );
}

async fn set_system_max(db: &TestDb, owner: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET tier = 'system_max' WHERE id = $1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    Ok(())
}
