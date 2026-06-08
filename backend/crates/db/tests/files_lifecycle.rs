//! DB-level file-tree lifecycle against a real Postgres schema.
//!
//! Drives the full command lifecycle through `FilesService` over the real
//! `FilesRepo` (so validation + SQL run end-to-end), plus search via the
//! `SearchStore` queries directly.
//!
//! Run with:
//! `NOTEGATE_TEST_DATABASE_URL=postgres://notegate:notegate@localhost:5433/notegate \
//!  cargo test -p notegate-db --test files_lifecycle`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, insert_user_account};
use notegate_db::{FilesRepo, WorkspaceRepo};
use notegate_service::files::input::Edit;
use notegate_service::files::{
    ChildrenCursor, CreateDocument, CreateFolder, DeleteNode, FilesService, FilesStore, MoveNode,
    PatchDocument, ReadDocument, WriteDocument, WriteTarget,
};
use notegate_service::search::{FindCursor, SearchStore};
use notegate_service::workspaces::{CreateWorkspace, WorkspaceStore};
use uuid::Uuid;

/// Create a workspace owned by `owner` and return `(workspace_id, root_id)`.
async fn setup_workspace(ws_repo: &WorkspaceRepo, owner: Uuid, name: &str) -> (Uuid, Uuid) {
    let ws = ws_repo
        .create_workspace(
            &CreateWorkspace {
                owner_account_id: owner,
                name: name.to_owned(),
            },
            owner,
        )
        .await
        .expect("create workspace");
    let root = WorkspaceStore::root_node_id(ws_repo, ws.id)
        .await
        .expect("root id query")
        .expect("root id present");
    (ws.id, root)
}

/// The full lifecycle: create ws → mkdir → touch → write → read → patch →
/// find(name) → grep → mv → rm, asserting attribution is populated throughout
/// and the derived path follows an O(1) move.
#[tokio::test]
async fn full_files_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let ws_repo = WorkspaceRepo::new(db.pool.clone());
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let repo = FilesRepo::new(db.pool.clone());

    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_workspace(&ws_repo, owner, "personal").await;

    // --- mkdir: /projects ---
    let projects = files
        .create_folder(
            owner,
            ws,
            CreateFolder {
                parent_node_id: root,
                name: "projects".to_owned(),
            },
        )
        .await?;
    assert_eq!(projects.path, "/projects");
    assert_eq!(projects.node.created_by, owner, "mkdir sets created_by");
    assert_eq!(projects.node.updated_by, owner, "mkdir sets updated_by");
    let projects_id = projects.node.id;

    // --- touch: /projects/note.md ---
    let touched = files
        .create_document(
            owner,
            ws,
            CreateDocument {
                parent_node_id: projects_id,
                name: "note.md".to_owned(),
            },
        )
        .await?;
    assert_eq!(touched.node.path, "/projects/note.md");
    assert_eq!(touched.document.byte_len, 0);
    assert_eq!(
        touched.document.created_by, owner,
        "touch sets doc created_by"
    );
    let note_id = touched.node.node.id;

    // --- write: replace content of the existing document ---
    let written = files
        .write_document(
            owner,
            ws,
            WriteDocument {
                target: WriteTarget::Existing { node_id: note_id },
                content_md: "# Title\nalpha beta gamma\n".to_owned(),
                expected_sha256: None,
            },
        )
        .await?;
    assert_eq!(written.document.line_count, 2);
    assert_eq!(
        written.document.updated_by, owner,
        "write sets doc updated_by"
    );
    let after_write_sha = written.document.content_sha256.clone();

    // --- read: range slice returns the content + metrics ---
    let read = files
        .read_document(
            owner,
            ws,
            ReadDocument {
                node_id: note_id,
                start_line: Some(1),
                max_lines: Some(10),
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await?;
    let content = read.content.expect("content present");
    assert_eq!(content.returned_lines, 2);
    assert!(content.content_md.contains("alpha beta gamma"));
    assert_eq!(read.content_sha256, after_write_sha);

    // --- patch: exact-match replacement ---
    let patched = files
        .patch_document(
            owner,
            ws,
            PatchDocument {
                node_id: note_id,
                edits: vec![Edit {
                    old_text: "beta".to_owned(),
                    new_text: "delta".to_owned(),
                }],
                expected_sha256: Some(after_write_sha.clone()),
            },
        )
        .await?;
    assert_eq!(patched.previous_sha256, after_write_sha);
    assert_eq!(patched.edits_applied, 1);
    let (_, doc_now) = repo_find_document(&repo, ws, note_id).await;
    assert!(doc_now.content_md.contains("alpha delta gamma"));
    assert_eq!(doc_now.updated_by, owner, "patch sets doc updated_by");

    // --- find by NAME: q='note' matches the document by name ---
    let by_name = repo.find_nodes(ws, "note", None, None, 50, None).await?;
    assert!(
        by_name.iter().any(|(n, _, _)| n.id == note_id),
        "find by name must match the document"
    );

    // --- find remains name-only: q='projects' matches the folder, not the doc via path ---
    let by_path = repo
        .find_nodes(ws, "projects", None, None, 50, None)
        .await?;
    assert!(
        by_path.iter().all(|(n, _, _)| n.id != note_id),
        "find must not match the document by path substring"
    );

    // --- grep: content candidate by body substring, with derived path ---
    let candidates = repo
        .grep_candidates(ws, "alpha delta", None, 20, None)
        .await?;
    let hit = candidates
        .iter()
        .find(|c| c.node_id == note_id)
        .expect("grep candidate present");
    assert_eq!(hit.path, "/projects/note.md", "grep returns derived path");
    assert!(hit.content_md.contains("alpha delta gamma"));

    // --- mv: move /projects/note.md → /archive/note.md (rename parent) ---
    let archive = files
        .create_folder(
            owner,
            ws,
            CreateFolder {
                parent_node_id: root,
                name: "archive".to_owned(),
            },
        )
        .await?;
    let archive_id = archive.node.id;
    let moved = files
        .move_node(
            owner,
            ws,
            MoveNode {
                node_id: note_id,
                new_parent_node_id: archive_id,
                new_name: None,
                expected_parent_id: None,
            },
        )
        .await?;
    assert_eq!(moved.path, "/archive/note.md", "move derives the new path");
    assert_eq!(moved.node.updated_by, owner, "move sets updated_by");

    // find is name-only; the moved document keeps its name, and its derived
    // path (for display) reflects the move even though the match is on name.
    let by_name = repo.find_nodes(ws, "note", None, None, 50, None).await?;
    let hit = by_name.iter().find(|(n, _, _)| n.id == note_id);
    assert!(hit.is_some(), "find by name must hit the moved document");
    assert_eq!(
        hit.map(|(_, p, _)| p.as_str()),
        Some("/archive/note.md"),
        "derived path reflects the move",
    );

    // --- rm: hide the moved document and mark it purge-eligible later ---
    let deleted = files
        .delete_node(
            owner,
            ws,
            DeleteNode {
                node_id: note_id,
                recursive: false,
            },
        )
        .await?;
    assert_eq!(deleted.node_id, note_id);
    assert_eq!(deleted.path, "/archive/note.md");
    assert!(
        repo.find_node(ws, note_id).await?.is_none(),
        "rm hides the node from live reads"
    );
    let (deleted_by, purge_after): (Option<Uuid>, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as(
            "SELECT deleted_by, purge_after FROM nodes WHERE workspace_id = $1 AND id = $2",
        )
        .bind(ws)
        .bind(note_id)
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(deleted_by, Some(owner), "rm sets deleted_by");
    assert_eq!(purge_after, Some(deleted.purge_after));

    db.cleanup().await;
    Ok(())
}

/// Keyset stability: insert 250 children, page through at limit=100 via
/// `(name, id)`-equivalent children cursors, and assert exactly 250 distinct ids
/// in monotonic order with no repeats.
#[tokio::test]
async fn keyset_pagination_is_stable() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let ws_repo = WorkspaceRepo::new(db.pool.clone());
    let repo = FilesRepo::new(db.pool.clone());

    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_workspace(&ws_repo, owner, "wide").await;

    // 250 documents directly under root (the folder fanout cap is 200, so insert
    // via raw SQL to exercise paging beyond a single page without the cap).
    for index in 0..250 {
        sqlx::query(
            "INSERT INTO nodes (workspace_id, parent_id, name, kind, created_by, updated_by) \
             VALUES ($1, $2, $3, 'document', $4, $4)",
        )
        .bind(ws)
        .bind(root)
        // Zero-padded names so lexical `(name, id)` order is well-defined.
        .bind(format!("doc-{index:04}.md"))
        .bind(owner)
        .execute(&db.pool)
        .await?;
    }

    let mut seen: Vec<Uuid> = Vec::new();
    let mut last_name: Option<String> = None;
    let mut cursor: Option<ChildrenCursor> = None;

    loop {
        let (rows, has_more) =
            FilesStore::paged_children(&repo, ws, root, 100, cursor.as_ref()).await?;
        assert!(!rows.is_empty(), "each page returns at least one row");
        for node in &rows {
            // Strict monotonicity by name (names are unique here).
            if let Some(prev) = &last_name {
                assert!(
                    node.name.as_str() > prev.as_str(),
                    "names strictly increase"
                );
            }
            last_name = Some(node.name.clone());
            seen.push(node.id);
        }
        let last = rows.last().expect("non-empty page");
        cursor = Some(ChildrenCursor {
            sort_order: last.sort_order,
            name: last.name.clone(),
            id: last.id,
        });
        if !has_more {
            break;
        }
    }

    assert_eq!(seen.len(), 250, "all 250 children paged exactly once");
    let mut distinct = seen.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 250, "no duplicate ids across pages");

    db.cleanup().await;
    Ok(())
}

/// Scoped find: a `scope_path` restricts results to that subtree. Used as a
/// focused check of the recursive-CTE scope path resolution.
#[tokio::test]
async fn find_scope_restricts_to_subtree() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let ws_repo = WorkspaceRepo::new(db.pool.clone());
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let repo = FilesRepo::new(db.pool.clone());

    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_workspace(&ws_repo, owner, "scoped").await;

    // /inside/target.md and /outside/target.md
    let inside = files
        .create_folder(
            owner,
            ws,
            CreateFolder {
                parent_node_id: root,
                name: "inside".to_owned(),
            },
        )
        .await?
        .node
        .id;
    let outside = files
        .create_folder(
            owner,
            ws,
            CreateFolder {
                parent_node_id: root,
                name: "outside".to_owned(),
            },
        )
        .await?
        .node
        .id;
    let inside_doc = files
        .create_document(
            owner,
            ws,
            CreateDocument {
                parent_node_id: inside,
                name: "target.md".to_owned(),
            },
        )
        .await?
        .node
        .node
        .id;
    let outside_doc = files
        .create_document(
            owner,
            ws,
            CreateDocument {
                parent_node_id: outside,
                name: "target.md".to_owned(),
            },
        )
        .await?
        .node
        .node
        .id;

    // Unscoped find matches both target.md documents.
    let all = repo.find_nodes(ws, "target", None, None, 50, None).await?;
    assert!(all.iter().any(|(n, _, _)| n.id == inside_doc));
    assert!(all.iter().any(|(n, _, _)| n.id == outside_doc));

    // Scoped to /inside matches only the inside document.
    let scoped = repo
        .find_nodes(ws, "target", Some("/inside"), None, 50, None)
        .await?;
    assert!(scoped.iter().any(|(n, _, _)| n.id == inside_doc));
    assert!(
        !scoped.iter().any(|(n, _, _)| n.id == outside_doc),
        "scope must exclude nodes outside the subtree"
    );

    db.cleanup().await;
    Ok(())
}

/// Load a document through the repo, panicking if missing (test helper).
async fn repo_find_document(
    repo: &FilesRepo,
    workspace_id: Uuid,
    node_id: Uuid,
) -> (notegate_model::Node, notegate_model::Document) {
    FilesStore::find_document(repo, workspace_id, node_id)
        .await
        .expect("find_document query")
        .expect("document present")
}

// Keep a reference so `FindCursor` import is exercised even if the lifecycle test
// does not page find results; this documents the cursor type used by `find`.
#[allow(dead_code)]
fn _find_cursor_shape(c: FindCursor) -> Uuid {
    c.id
}
