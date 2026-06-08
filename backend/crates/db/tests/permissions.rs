//! Permission matrix over a real Postgres schema.
//!
//! Drives `FilesService`, `AccessService`, and `WorkspaceService` over the real
//! db repos and asserts the spec's role gate (`docs/spec/rest/README.md`
//! "Workspace authorization" and `db.md` role table) at the service-error level, which the API
//! layer maps 1:1 to HTTP status codes:
//!
//! - `ServiceError::Forbidden`  -> 403 (lesser role for the command)
//! - `ServiceError::NotFound`   -> 404 (no live role / cross-workspace node)
//!
//! Matrix asserted here:
//! - viewer  CAN list/stat/read/find/grep, CANNOT mkdir/touch/write/patch/mv/rm (403)
//! - editor  CAN mutate, CANNOT manage access (403)
//! - owner   CAN manage access
//! - a node id from another workspace -> 404
//! - a caller with no live grant -> 404
//!
//! The 401 (unauthenticated) and 403 not_registered / inactive_account cases are
//! auth-layer concerns asserted in the API e2e harness (`api` crate); the service
//! layer below is reached only after a caller is resolved.
//!
//! Run with:
//! `NOTEGATE_TEST_DATABASE_URL=postgres://notegate:notegate@localhost:5433/notegate \
//!  cargo test -p notegate-db --test permissions`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

mod common;

use common::{TestDb, insert_user_account};
use notegate_db::{AccessRepo, FilesRepo, WorkspaceRepo};
use notegate_model::Role;
use notegate_service::access::{AccessService, GrantAccess};
use notegate_service::error::ServiceError;
use notegate_service::files::input::Edit;
use notegate_service::files::{
    CreateDocument, CreateFolder, DeleteNode, FilesService, MoveNode, PatchDocument, ReadDocument,
    WriteDocument, WriteTarget,
};
use notegate_service::search::{FindRequest, GrepRequest, SearchService};
use notegate_service::workspaces::{CreateWorkspace, WorkspaceStore};
use uuid::Uuid;

/// Assert a result is `Forbidden` (403).
fn assert_forbidden<T: std::fmt::Debug>(result: Result<T, ServiceError>, what: &str) {
    match result {
        Err(ServiceError::Forbidden(_)) => {}
        other => panic!("{what}: expected Forbidden (403), got {other:?}"),
    }
}

/// Assert a result is `NotFound` (404).
fn assert_not_found<T: std::fmt::Debug>(result: Result<T, ServiceError>, what: &str) {
    match result {
        Err(ServiceError::NotFound(_)) => {}
        other => panic!("{what}: expected NotFound (404), got {other:?}"),
    }
}

/// A fixture: an owner with a workspace containing one folder and one document,
/// plus a viewer and an editor account already granted access.
struct Fixture {
    files: FilesService<FilesRepo>,
    search: SearchService<FilesRepo>,
    access: AccessService<AccessRepo>,
    workspace_id: Uuid,
    root_id: Uuid,
    folder_id: Uuid,
    doc_id: Uuid,
    owner: Uuid,
    editor: Uuid,
    viewer: Uuid,
    /// A second workspace owned by the same owner, used for cross-workspace 404.
    other_workspace_id: Uuid,
    other_folder_id: Uuid,
}

async fn setup(db: &TestDb) -> Result<Fixture, Box<dyn std::error::Error>> {
    let ws_repo = WorkspaceRepo::new(db.pool.clone());
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let search = SearchService::new(FilesRepo::new(db.pool.clone()));
    let access = AccessService::new(AccessRepo::new(db.pool.clone()));

    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let editor = insert_user_account(&db.pool, "editor", "e@example.test").await?;
    let viewer = insert_user_account(&db.pool, "viewer", "v@example.test").await?;

    let ws = ws_repo
        .create_workspace(
            &CreateWorkspace {
                owner_account_id: owner,
                name: "personal".to_owned(),
            },
            owner,
        )
        .await?;
    let workspace_id = ws.id;
    let root_id = WorkspaceStore::root_node_id(&ws_repo, workspace_id)
        .await?
        .expect("root id");

    // Owner seeds a folder and a document (owner is editor+).
    let folder = files
        .create_folder(
            owner,
            workspace_id,
            CreateFolder {
                parent_node_id: root_id,
                name: "projects".to_owned(),
            },
        )
        .await?;
    let folder_id = folder.node.id;
    let doc = files
        .write_document(
            owner,
            workspace_id,
            WriteDocument {
                target: WriteTarget::Create {
                    parent_node_id: folder_id,
                    name: "note.md".to_owned(),
                },
                content_md: "# Title\nalpha beta gamma\n".to_owned(),
                expected_sha256: None,
            },
        )
        .await?;
    let doc_id = doc.node.node.id;

    // Grant editor + viewer their roles.
    access
        .grant(
            owner,
            GrantAccess {
                workspace_id,
                account_id: editor,
                role: Role::Editor,
            },
        )
        .await?;
    access
        .grant(
            owner,
            GrantAccess {
                workspace_id,
                account_id: viewer,
                role: Role::Viewer,
            },
        )
        .await?;

    // A second workspace (same owner) for the cross-workspace 404 case.
    let other = ws_repo
        .create_workspace(
            &CreateWorkspace {
                owner_account_id: owner,
                name: "other".to_owned(),
            },
            owner,
        )
        .await?;
    let other_workspace_id = other.id;
    let other_root = WorkspaceStore::root_node_id(&ws_repo, other_workspace_id)
        .await?
        .expect("other root id");
    let other_folder = files
        .create_folder(
            owner,
            other_workspace_id,
            CreateFolder {
                parent_node_id: other_root,
                name: "elsewhere".to_owned(),
            },
        )
        .await?;
    let other_folder_id = other_folder.node.id;

    Ok(Fixture {
        files,
        search,
        access,
        workspace_id,
        root_id,
        folder_id,
        doc_id,
        owner,
        editor,
        viewer,
        other_workspace_id,
        other_folder_id,
    })
}

/// viewer CAN list/stat/read/find/grep; CANNOT mkdir/touch/write/patch/mv/rm (403).
#[tokio::test]
async fn viewer_can_read_but_not_mutate() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let f = setup(&db).await?;
    let v = f.viewer;
    let ws = f.workspace_id;

    // --- viewer CAN read-class commands ---
    f.files.stat(v, ws, f.folder_id).await?;
    f.files
        .children(
            v,
            ws,
            f.folder_id,
            notegate_service::files::ChildrenRequest {
                limit: None,
                cursor: None,
            },
        )
        .await?;
    f.files.resolve_path(v, ws, "/projects/note.md").await?;
    f.files
        .read_document(
            v,
            ws,
            ReadDocument {
                node_id: f.doc_id,
                start_line: None,
                max_lines: None,
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await?;
    f.search
        .find(
            v,
            ws,
            FindRequest {
                q: "note".to_owned(),
                path: None,
                kind: None,
                limit: None,
                cursor: None,
            },
        )
        .await?;
    f.search
        .grep(
            v,
            ws,
            GrepRequest {
                q: "alpha".to_owned(),
                path: None,
                context: None,
                limit: None,
                cursor: None,
            },
        )
        .await?;

    // --- viewer CANNOT mutate (403) ---
    assert_forbidden(
        f.files
            .create_folder(
                v,
                ws,
                CreateFolder {
                    parent_node_id: f.root_id,
                    name: "x".to_owned(),
                },
            )
            .await,
        "viewer mkdir",
    );
    assert_forbidden(
        f.files
            .create_document(
                v,
                ws,
                CreateDocument {
                    parent_node_id: f.folder_id,
                    name: "x.md".to_owned(),
                },
            )
            .await,
        "viewer touch",
    );
    assert_forbidden(
        f.files
            .write_document(
                v,
                ws,
                WriteDocument {
                    target: WriteTarget::Existing { node_id: f.doc_id },
                    content_md: "new".to_owned(),
                    expected_sha256: None,
                },
            )
            .await,
        "viewer write",
    );
    assert_forbidden(
        f.files
            .patch_document(
                v,
                ws,
                PatchDocument {
                    node_id: f.doc_id,
                    edits: vec![Edit {
                        old_text: "alpha".to_owned(),
                        new_text: "ALPHA".to_owned(),
                    }],
                    expected_sha256: None,
                },
            )
            .await,
        "viewer patch",
    );
    assert_forbidden(
        f.files
            .move_node(
                v,
                ws,
                MoveNode {
                    node_id: f.doc_id,
                    new_parent_node_id: f.root_id,
                    new_name: None,
                    expected_parent_id: None,
                },
            )
            .await,
        "viewer mv",
    );
    assert_forbidden(
        f.files
            .delete_node(
                v,
                ws,
                DeleteNode {
                    node_id: f.doc_id,
                    recursive: false,
                },
            )
            .await,
        "viewer rm",
    );
    // viewer cannot manage access (owner-only).
    assert_forbidden(
        f.access
            .grant(
                v,
                GrantAccess {
                    workspace_id: ws,
                    account_id: f.editor,
                    role: Role::Viewer,
                },
            )
            .await,
        "viewer manage-access",
    );

    db.cleanup().await;
    Ok(())
}

/// editor CAN mutate; CANNOT manage access (403).
#[tokio::test]
async fn editor_can_mutate_but_not_manage_access() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let f = setup(&db).await?;
    let e = f.editor;
    let ws = f.workspace_id;

    // --- editor CAN mutate ---
    let made = f
        .files
        .create_folder(
            e,
            ws,
            CreateFolder {
                parent_node_id: f.root_id,
                name: "editordir".to_owned(),
            },
        )
        .await?;
    f.files
        .create_document(
            e,
            ws,
            CreateDocument {
                parent_node_id: made.node.id,
                name: "e.md".to_owned(),
            },
        )
        .await?;
    f.files
        .write_document(
            e,
            ws,
            WriteDocument {
                target: WriteTarget::Existing { node_id: f.doc_id },
                content_md: "# Title\nalpha beta gamma\n".to_owned(),
                expected_sha256: None,
            },
        )
        .await?;
    f.files
        .patch_document(
            e,
            ws,
            PatchDocument {
                node_id: f.doc_id,
                edits: vec![Edit {
                    old_text: "beta".to_owned(),
                    new_text: "delta".to_owned(),
                }],
                expected_sha256: None,
            },
        )
        .await?;
    f.files
        .move_node(
            e,
            ws,
            MoveNode {
                node_id: f.doc_id,
                new_parent_node_id: made.node.id,
                new_name: None,
                expected_parent_id: None,
            },
        )
        .await?;
    f.files
        .delete_node(
            e,
            ws,
            DeleteNode {
                node_id: f.doc_id,
                recursive: false,
            },
        )
        .await?;

    // --- editor CANNOT manage access (403) ---
    assert_forbidden(
        f.access
            .grant(
                e,
                GrantAccess {
                    workspace_id: ws,
                    account_id: f.viewer,
                    role: Role::Editor,
                },
            )
            .await,
        "editor grant",
    );
    assert_forbidden(f.access.revoke(e, ws, f.viewer).await, "editor revoke");
    assert_forbidden(f.access.list(e, ws).await, "editor list-access");

    db.cleanup().await;
    Ok(())
}

/// owner CAN manage access (grant/list/revoke).
#[tokio::test]
async fn owner_can_manage_access() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let f = setup(&db).await?;
    let o = f.owner;
    let ws = f.workspace_id;

    // owner lists access (owner + editor + viewer = 3 live grants).
    let listed = f.access.list(o, ws).await?;
    assert_eq!(listed.len(), 3, "owner sees all live grants");

    // owner grants a new account, then revokes it.
    let newcomer = insert_user_account(&db.pool, "newcomer", "n@example.test").await?;
    let grant = f
        .access
        .grant(
            o,
            GrantAccess {
                workspace_id: ws,
                account_id: newcomer,
                role: Role::Viewer,
            },
        )
        .await?;
    assert_eq!(grant.role, Role::Viewer);
    f.access.revoke(o, ws, newcomer).await?;
    let after = f.access.list(o, ws).await?;
    assert_eq!(after.len(), 3, "revoked grant drops out of the live list");

    db.cleanup().await;
    Ok(())
}

/// A node id that belongs to a different workspace is hidden as not-found (404),
/// even for the owner of both workspaces (the URL workspace scopes the lookup).
#[tokio::test]
async fn cross_workspace_node_is_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let f = setup(&db).await?;
    let o = f.owner;

    // The owner is owner of BOTH workspaces, so this is purely the cross-workspace
    // scoping check (not an access denial): stat the other workspace's folder id
    // through THIS workspace -> 404.
    assert_not_found(
        f.files.stat(o, f.workspace_id, f.other_folder_id).await,
        "cross-workspace stat",
    );
    assert_not_found(
        f.files
            .move_node(
                o,
                f.workspace_id,
                MoveNode {
                    node_id: f.other_folder_id,
                    new_parent_node_id: f.root_id,
                    new_name: None,
                    expected_parent_id: None,
                },
            )
            .await,
        "cross-workspace mv",
    );
    // And the symmetric direction: this workspace's doc through the OTHER workspace.
    assert_not_found(
        f.files.stat(o, f.other_workspace_id, f.doc_id).await,
        "cross-workspace stat (reverse)",
    );

    db.cleanup().await;
    Ok(())
}

/// A caller with no live grant for the workspace sees it as not-found (404),
/// across read and mutate commands and access management.
#[tokio::test]
async fn no_role_is_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let f = setup(&db).await?;
    let ws = f.workspace_id;
    let stranger = insert_user_account(&db.pool, "stranger", "s@example.test").await?;

    assert_not_found(
        f.files.stat(stranger, ws, f.folder_id).await,
        "stranger stat",
    );
    assert_not_found(
        f.files
            .resolve_path(stranger, ws, "/projects/note.md")
            .await,
        "stranger resolve",
    );
    assert_not_found(
        f.search
            .find(
                stranger,
                ws,
                FindRequest {
                    q: "note".to_owned(),
                    path: None,
                    kind: None,
                    limit: None,
                    cursor: None,
                },
            )
            .await,
        "stranger find",
    );
    assert_not_found(
        f.files
            .create_folder(
                stranger,
                ws,
                CreateFolder {
                    parent_node_id: f.root_id,
                    name: "x".to_owned(),
                },
            )
            .await,
        "stranger mkdir",
    );
    assert_not_found(
        f.access
            .grant(
                stranger,
                GrantAccess {
                    workspace_id: ws,
                    account_id: f.viewer,
                    role: Role::Viewer,
                },
            )
            .await,
        "stranger manage-access",
    );

    db.cleanup().await;
    Ok(())
}
