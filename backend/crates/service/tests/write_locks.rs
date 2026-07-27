//! Integration coverage for the node write-lock contract.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use std::error::Error;
use std::fmt::Debug;

use common::{TestDb, insert_user_account};
use notegate_db::{AccountRepo, AgentRepo, ConnectionRepo, FilesRepo, SpaceRepo};
use notegate_model::files::{ObjectUploadMode, ObjectUploadRegistration};
use notegate_model::{
    AccountKind, Caller, CallerIdentity, Channel, ConnectAgent, CreateAgent, FileEncryptionMode,
    Permission,
};
use notegate_service::ServiceError;
use notegate_service::connections::ConnectionService;
use notegate_service::files::{
    BeginObjectUpload, ChildrenRequest, CopyNode, CreateFolder, CreateText, DeleteNode,
    FilesService, MoveNode, ReadText, UpdateNode, UpdateNodeSearchPolicy, UpdateNodeWriteLock,
    UpdateTextEncryption, WriteTarget, WriteText, WriteTextBody,
};
use notegate_service::search::{GrepLineMode, GrepMatchMode, GrepRequest, SearchService};
use notegate_service::spaces::{CreateSpace, SpaceService};
use serde_json::json;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct Fixture {
    db: TestDb,
    owner: Uuid,
    space_id: Uuid,
    root_id: Uuid,
    files: FilesService,
    browser: Caller,
}

impl Fixture {
    async fn setup(label: &str) -> TestResult<Option<Self>> {
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

    async fn folder(&self, parent_node_id: Uuid, name: &str) -> TestResult<Uuid> {
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

    async fn text(&self, parent_node_id: Uuid, name: &str) -> TestResult<Uuid> {
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

    async fn set_lock(&self, node_id: Uuid, enabled: bool) -> TestResult {
        self.files
            .update_node_write_lock(
                &self.browser,
                self.space_id,
                UpdateNodeWriteLock { node_id, enabled },
            )
            .await?;
        Ok(())
    }

    async fn cleanup(self) {
        self.db.cleanup().await;
    }
}

async fn load_caller(db: &TestDb, account_id: Uuid, channel: Channel) -> Caller {
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

async fn set_system_max(db: &TestDb, owner: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET tier = 'system_max' WHERE id = $1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    Ok(())
}

fn assert_write_locked<T: Debug>(result: Result<T, ServiceError>) {
    assert!(
        matches!(&result, Err(ServiceError::WriteLocked { .. })),
        "expected write-lock conflict, got {result:?}"
    );
}

fn assert_forbidden<T: Debug>(result: Result<T, ServiceError>) {
    assert!(
        matches!(&result, Err(ServiceError::Forbidden(_))),
        "expected forbidden result, got {result:?}"
    );
}

#[tokio::test]
async fn ancestor_lock_blocks_descendant_changes_and_reports_its_source() -> TestResult {
    let Some(fixture) = Fixture::setup("inheritance").await? else {
        return Ok(());
    };
    let folder_id = fixture.folder(fixture.root_id, "Policies").await?;
    let text_id = fixture.text(folder_id, "access.md").await?;
    fixture
        .files
        .write_text(
            fixture.owner,
            fixture.space_id,
            WriteText {
                target: WriteTarget::Existing { node_id: text_id },
                body: WriteTextBody::Plain("alpha\n".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;

    fixture.set_lock(folder_id, true).await?;

    let detail = fixture
        .files
        .stat(fixture.owner, fixture.space_id, text_id)
        .await?;
    assert!(!detail.node.write_locked);
    assert_eq!(detail.write_lock_sources.len(), 1);
    assert_eq!(detail.write_lock_sources[0].node_id, folder_id);
    assert_eq!(detail.write_lock_sources[0].path, "/Policies");

    let children = fixture
        .files
        .children(
            fixture.owner,
            fixture.space_id,
            folder_id,
            ChildrenRequest {
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert!(
        children
            .items
            .iter()
            .find(|item| item.node.id == text_id)
            .expect("locked child")
            .effective_write_locked
    );

    let grep = SearchService::new(FilesRepo::new(fixture.db.pool.clone()))
        .grep(
            fixture.owner,
            fixture.space_id,
            GrepRequest {
                q: "alpha".to_owned(),
                path: Some("/Policies".to_owned()),
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::First,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(grep.items.len(), 1);
    assert_eq!(grep.items[0].node.write_lock_sources[0].node_id, folder_id);

    assert_write_locked(
        fixture
            .files
            .write_text(
                fixture.owner,
                fixture.space_id,
                WriteText {
                    target: WriteTarget::Existing { node_id: text_id },
                    body: WriteTextBody::Plain("blocked".to_owned()),
                    expected_sha256: None,
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .create_text(
                fixture.owner,
                fixture.space_id,
                CreateText {
                    parent_node_id: folder_id,
                    name: "new.md".to_owned(),
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .replace_metadata(
                fixture.owner,
                fixture.space_id,
                text_id,
                json!({ "classification": "restricted" }),
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .update_node_search_policy(
                AccountKind::User,
                fixture.owner,
                fixture.space_id,
                UpdateNodeSearchPolicy {
                    node_id: text_id,
                    enabled: false,
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .update_text_encryption(
                AccountKind::User,
                fixture.owner,
                fixture.space_id,
                UpdateTextEncryption {
                    node_id: text_id,
                    enabled: true,
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .delete_node(
                fixture.owner,
                fixture.space_id,
                DeleteNode {
                    node_id: text_id,
                    recursive: false,
                },
            )
            .await,
    );

    let read = fixture
        .files
        .read_text(
            fixture.owner,
            fixture.space_id,
            ReadText {
                node_id: text_id,
                start_line: None,
                max_lines: None,
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await?;
    assert_eq!(read.node.node.id, text_id);

    fixture.set_lock(folder_id, false).await?;
    fixture
        .files
        .write_text(
            fixture.owner,
            fixture.space_id,
            WriteText {
                target: WriteTarget::Existing { node_id: text_id },
                body: WriteTextBody::Plain("unlocked".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn locked_descendant_protects_subtree_structure_without_freezing_parent() -> TestResult {
    let Some(fixture) = Fixture::setup("subtree").await? else {
        return Ok(());
    };
    let folder_id = fixture.folder(fixture.root_id, "Archive").await?;
    let destination_id = fixture.folder(fixture.root_id, "Elsewhere").await?;
    let child_id = fixture.text(folder_id, "release.md").await?;
    fixture.set_lock(child_id, true).await?;

    fixture.text(folder_id, "sibling.md").await?;
    fixture
        .files
        .replace_metadata(
            fixture.owner,
            fixture.space_id,
            folder_id,
            json!({ "classification": "archive" }),
        )
        .await?;

    assert_write_locked(
        fixture
            .files
            .update_node(
                fixture.owner,
                fixture.space_id,
                UpdateNode {
                    node_id: folder_id,
                    name: Some("Renamed".to_owned()),
                    sort_order: None,
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .move_node(
                fixture.owner,
                fixture.space_id,
                MoveNode {
                    node_id: folder_id,
                    new_parent_node_id: destination_id,
                    new_name: None,
                    expected_parent_id: Some(fixture.root_id),
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .delete_node(
                fixture.owner,
                fixture.space_id,
                DeleteNode {
                    node_id: folder_id,
                    recursive: true,
                },
            )
            .await,
    );

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn lock_management_is_browser_owner_only_and_tier_gated() -> TestResult {
    let Some(fixture) = Fixture::setup("policy").await? else {
        return Ok(());
    };
    let text_id = fixture.text(fixture.root_id, "policy.md").await?;
    let other =
        insert_user_account(&fixture.db.pool, "write-lock-other", "other@example.test").await?;

    assert_forbidden(
        fixture
            .files
            .update_node_write_lock(
                &load_caller(&fixture.db, fixture.owner, Channel::Mcp).await,
                fixture.space_id,
                UpdateNodeWriteLock {
                    node_id: text_id,
                    enabled: true,
                },
            )
            .await,
    );
    assert_forbidden(
        fixture
            .files
            .update_node_write_lock(
                &load_caller(&fixture.db, other, Channel::Browser).await,
                fixture.space_id,
                UpdateNodeWriteLock {
                    node_id: text_id,
                    enabled: true,
                },
            )
            .await,
    );

    let agent = AgentRepo::new(fixture.db.pool.clone())
        .insert_agent(
            &CreateAgent {
                name: "write-lock-agent".to_owned(),
            },
            fixture.owner,
        )
        .await?;
    let agent_id = agent.id;
    ConnectionService::new(ConnectionRepo::new(fixture.db.pool.clone()))
        .connect(
            AccountKind::User,
            fixture.owner,
            ConnectAgent {
                space_id: fixture.space_id,
                agent_id,
                permission: Permission::Write,
            },
        )
        .await?;
    let agent_account = AccountRepo::new(fixture.db.pool.clone())
        .find_account(agent_id)
        .await?
        .expect("agent account");
    assert_forbidden(
        fixture
            .files
            .update_node_write_lock(
                &Caller {
                    account: agent_account,
                    identity: CallerIdentity::Agent(agent),
                    channel: Channel::Browser,
                },
                fixture.space_id,
                UpdateNodeWriteLock {
                    node_id: text_id,
                    enabled: true,
                },
            )
            .await,
    );

    assert!(matches!(
        fixture
            .files
            .update_node_write_lock(
                &fixture.browser,
                fixture.space_id,
                UpdateNodeWriteLock {
                    node_id: fixture.root_id,
                    enabled: true,
                },
            )
            .await,
        Err(ServiceError::Conflict(_))
    ));

    fixture.set_lock(text_id, true).await?;
    assert_write_locked(
        fixture
            .files
            .write_text(
                agent_id,
                fixture.space_id,
                WriteText {
                    target: WriteTarget::Existing { node_id: text_id },
                    body: WriteTextBody::Plain("agent write".to_owned()),
                    expected_sha256: None,
                },
            )
            .await,
    );
    sqlx::query("UPDATE users SET tier = 'tier0' WHERE id = $1")
        .bind(fixture.owner)
        .execute(&fixture.db.pool)
        .await?;
    fixture.set_lock(text_id, false).await?;
    assert!(matches!(
        fixture
            .files
            .update_node_write_lock(
                &fixture.browser,
                fixture.space_id,
                UpdateNodeWriteLock {
                    node_id: text_id,
                    enabled: true,
                },
            )
            .await,
        Err(ServiceError::Conflict(_))
    ));

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn lock_sources_follow_ancestors_and_are_not_copied() -> TestResult {
    let Some(fixture) = Fixture::setup("sources").await? else {
        return Ok(());
    };
    let outer_id = fixture.folder(fixture.root_id, "outer").await?;
    let inner_id = fixture.folder(outer_id, "inner").await?;
    let text_id = fixture.text(inner_id, "note.md").await?;

    for node_id in [outer_id, inner_id, text_id] {
        fixture.set_lock(node_id, true).await?;
    }
    let detail = fixture
        .files
        .stat(fixture.owner, fixture.space_id, text_id)
        .await?;
    assert_eq!(
        detail
            .write_lock_sources
            .iter()
            .map(|source| source.path.as_str())
            .collect::<Vec<_>>(),
        ["/outer", "/outer/inner", "/outer/inner/note.md"]
    );

    fixture.set_lock(text_id, false).await?;
    fixture.set_lock(inner_id, false).await?;
    let inherited = fixture
        .files
        .stat(fixture.owner, fixture.space_id, text_id)
        .await?;
    assert!(!inherited.node.write_locked);
    assert_eq!(inherited.write_lock_sources[0].node_id, outer_id);

    let copied = fixture
        .files
        .copy_node(
            fixture.owner,
            fixture.space_id,
            CopyNode {
                node_id: outer_id,
                new_parent_node_id: fixture.root_id,
                new_name: "outer-copy".to_owned(),
                recursive: true,
            },
        )
        .await?;
    assert!(!copied.node.node.write_locked);
    let copied_descendant = fixture
        .files
        .resolve_path(fixture.owner, fixture.space_id, "/outer-copy/inner/note.md")
        .await?;
    assert!(!copied_descendant.node.write_locked);
    assert!(copied_descendant.write_lock_sources.is_empty());

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn file_completion_and_deletion_recheck_the_current_lock() -> TestResult {
    let Some(fixture) = Fixture::setup("files").await? else {
        return Ok(());
    };
    let folder_id = fixture.folder(fixture.root_id, "uploads").await?;
    let upload_id = Uuid::new_v4();
    let command = BeginObjectUpload {
        parent_node_id: folder_id,
        name: "report.bin".to_owned(),
        byte_len: 4,
        media_type: "application/octet-stream".to_owned(),
        original_filename: Some("report.bin".to_owned()),
        encryption_mode: FileEncryptionMode::None,
        encryption_metadata: None,
    };
    fixture
        .files
        .prepare_object_upload(fixture.owner, fixture.space_id, &command)
        .await?;
    fixture
        .files
        .record_registered_object_upload(
            &ObjectUploadRegistration {
                id: upload_id,
                object_key: format!("objects/{upload_id}"),
                upload_mode: ObjectUploadMode::Single,
                multipart_upload_id: None,
                multipart_part_size: None,
            },
            fixture.owner,
            fixture.space_id,
            &command,
        )
        .await?;

    fixture.set_lock(folder_id, true).await?;
    assert_write_locked(
        fixture
            .files
            .record_registered_object_upload(
                &ObjectUploadRegistration {
                    id: Uuid::new_v4(),
                    object_key: format!("objects/{}", Uuid::new_v4()),
                    upload_mode: ObjectUploadMode::Single,
                    multipart_upload_id: None,
                    multipart_part_size: None,
                },
                fixture.owner,
                fixture.space_id,
                &BeginObjectUpload {
                    name: "blocked.bin".to_owned(),
                    ..command.clone()
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .complete_object_upload(fixture.owner, fixture.space_id, upload_id, None)
            .await,
    );
    let upload_state: String =
        sqlx::query_scalar("SELECT state FROM object_storage_objects WHERE id = $1")
            .bind(upload_id)
            .fetch_one(&fixture.db.pool)
            .await?;
    assert_eq!(upload_state, "expire_pending");

    fixture.set_lock(folder_id, false).await?;
    let retry_upload_id = Uuid::new_v4();
    fixture
        .files
        .record_registered_object_upload(
            &ObjectUploadRegistration {
                id: retry_upload_id,
                object_key: format!("objects/{retry_upload_id}"),
                upload_mode: ObjectUploadMode::Single,
                multipart_upload_id: None,
                multipart_part_size: None,
            },
            fixture.owner,
            fixture.space_id,
            &command,
        )
        .await?;
    let file = fixture
        .files
        .complete_object_upload(fixture.owner, fixture.space_id, retry_upload_id, None)
        .await?;
    let file_id = file.node.node.id;
    fixture.set_lock(file_id, true).await?;

    let repeated = fixture
        .files
        .complete_object_upload(fixture.owner, fixture.space_id, retry_upload_id, None)
        .await?;
    assert_eq!(repeated.node.write_lock_sources[0].node_id, file_id);
    assert_eq!(
        fixture
            .files
            .file_for_download(fixture.owner, fixture.space_id, file_id)
            .await?
            .node
            .node
            .id,
        file_id
    );
    assert_write_locked(
        fixture
            .files
            .delete_node(
                fixture.owner,
                fixture.space_id,
                DeleteNode {
                    node_id: file_id,
                    recursive: false,
                },
            )
            .await,
    );

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_lock_and_write_are_serialized() -> TestResult {
    let Some(fixture) = Fixture::setup("race").await? else {
        return Ok(());
    };
    let text_id = fixture.text(fixture.root_id, "race.md").await?;
    let before_event_id: i64 =
        sqlx::query_scalar("SELECT COALESCE(max(id), 0) FROM file_change_events")
            .fetch_one(&fixture.db.pool)
            .await?;

    let write = fixture.files.write_text(
        fixture.owner,
        fixture.space_id,
        WriteText {
            target: WriteTarget::Existing { node_id: text_id },
            body: WriteTextBody::Plain("concurrent write".to_owned()),
            expected_sha256: None,
        },
    );
    let lock = fixture.files.update_node_write_lock(
        &fixture.browser,
        fixture.space_id,
        UpdateNodeWriteLock {
            node_id: text_id,
            enabled: true,
        },
    );
    let (write_result, lock_result) = tokio::join!(write, lock);
    lock_result?;

    let events: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT op_type, metadata FROM file_change_events \
         WHERE space_id = $1 AND node_id = $2 AND id > $3 ORDER BY id",
    )
    .bind(fixture.space_id)
    .bind(text_id)
    .bind(before_event_id)
    .fetch_all(&fixture.db.pool)
    .await?;
    let lock_event_index = events
        .iter()
        .position(|(_, metadata)| metadata["write_lock_changed"] == true)
        .expect("write-lock event");

    match write_result {
        Ok(_) => {
            let write_event_index = events
                .iter()
                .position(|(op_type, _)| op_type == "text.write")
                .expect("text write event");
            assert!(write_event_index < lock_event_index);
        }
        Err(ServiceError::WriteLocked { .. }) => {}
        Err(error) => panic!("unexpected write result: {error:?}"),
    }
    assert!(
        fixture
            .files
            .stat(fixture.owner, fixture.space_id, text_id)
            .await?
            .node
            .write_locked
    );

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn space_deletion_remains_an_owner_lifecycle_action() -> TestResult {
    let Some(fixture) = Fixture::setup("space-delete").await? else {
        return Ok(());
    };
    let text_id = fixture.text(fixture.root_id, "locked.md").await?;
    fixture.set_lock(text_id, true).await?;

    SpaceService::new(SpaceRepo::new(fixture.db.pool.clone()))
        .delete(AccountKind::User, fixture.owner, fixture.space_id)
        .await?;
    assert!(
        SpaceRepo::new(fixture.db.pool.clone())
            .find_space(fixture.space_id)
            .await?
            .is_none()
    );

    fixture.cleanup().await;
    Ok(())
}
