//! DB-level file-tree lifecycle against a real Postgres schema.
//!
//! Drives the full command lifecycle through `FilesService` over the real
//! `FilesRepo` (so validation + SQL run end-to-end), plus search through the
//! service scanner.
//!
//! Run with:
//! `NOTEGATE_TEST_DATABASE_URL=postgres://notegate:notegate@localhost:5433/notegate \
//!  cargo test -p notegate-service --test files_lifecycle`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, insert_user_account};
use notegate_core::Error;
use notegate_db::{FilesRepo, SpaceRepo};
use notegate_model::FileEncryptionMode;
use notegate_service::files::Edit;
use notegate_service::files::{
    AppendText, ChildrenCursor, CreateFile, CreateFolder, CreateText, DeleteNode, FilesService,
    MoveNode, PatchText, ReadText, ReadTextBody, WriteTarget, WriteText, WriteTextBody,
};
use notegate_service::search::{
    FindMatchMode, FindRequest, GrepLineMode, GrepMatchMode, GrepRequest, SearchService,
};
use notegate_service::spaces::CreateSpace;
use serde_json::json;
use uuid::Uuid;

/// Create a space owned by `owner` and return `(space_id, root_id)`.
async fn setup_space(ws_repo: &SpaceRepo, owner: Uuid, name: &str) -> (Uuid, Uuid) {
    let ws = ws_repo
        .create_space(
            owner,
            &CreateSpace {
                name: name.to_owned(),
            },
        )
        .await
        .expect("create space");
    let root = ws_repo
        .root_node_id(ws.id)
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
    let ws_repo = SpaceRepo::new(db.pool.clone());
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let search = SearchService::new(FilesRepo::new(db.pool.clone()));
    let repo = FilesRepo::new(db.pool.clone());

    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "personal").await;

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
    assert_eq!(
        projects.node.created_by_account_id, owner,
        "mkdir sets created_by"
    );
    assert_eq!(
        projects.node.updated_by_account_id, owner,
        "mkdir sets updated_by"
    );
    let projects_id = projects.node.id;

    // --- metadata: replace, read, and merge-patch a node metadata object ---
    let metadata = json!({
        "title": "Project notes",
        "tags": ["project", "draft"],
        "nested": {"keep": true, "remove": true}
    });
    let metadata_updated = files
        .replace_metadata(owner, ws, projects_id, metadata.clone())
        .await?;
    assert_eq!(metadata_updated.node.metadata, metadata);

    let read_metadata = files.read_metadata(owner, ws, projects_id).await?;
    assert_eq!(read_metadata["title"], "Project notes");

    let metadata_patched = files
        .patch_metadata(
            owner,
            ws,
            projects_id,
            json!({
                "status": "active",
                "nested": {"remove": null},
                "tags": ["project"]
            }),
        )
        .await?;
    assert_eq!(metadata_patched.node.metadata["status"], "active");
    assert_eq!(metadata_patched.node.metadata["tags"], json!(["project"]));
    assert!(
        metadata_patched.node.metadata["nested"]
            .get("keep")
            .is_some()
    );
    assert!(
        metadata_patched.node.metadata["nested"]
            .get("remove")
            .is_none()
    );

    // --- touch: /projects/note.md ---
    let touched = files
        .create_text(
            owner,
            ws,
            CreateText {
                parent_node_id: projects_id,
                name: "note.md".to_owned(),
            },
        )
        .await?;
    assert_eq!(touched.node.path, "/projects/note.md");
    assert_eq!(touched.text.byte_len, 0);
    assert_eq!(
        touched.text.created_by_account_id, owner,
        "touch sets doc created_by"
    );
    let note_id = touched.node.node.id;

    // --- write: replace content of the existing text ---
    let written = files
        .write_text(
            owner,
            ws,
            WriteText {
                target: WriteTarget::Existing { node_id: note_id },
                body: WriteTextBody::Plain("# Title\nalpha beta gamma".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;
    assert_eq!(written.text.line_count, 2);
    assert_eq!(
        written.text.updated_by_account_id, owner,
        "write sets doc updated_by"
    );
    let after_initial_write_sha = written.text.content_sha256.clone();

    // --- append: exact EOF append, then newline-separated append ---
    let appended = files
        .append_text(
            owner,
            ws,
            AppendText {
                target: WriteTarget::Existing { node_id: note_id },
                content: "delta".to_owned(),
                expected_sha256: Some(after_initial_write_sha.clone()),
                ensure_newline: false,
            },
        )
        .await?;
    assert_eq!(appended.text.line_count, 2);
    let after_append_sha = appended.text.content_sha256.clone();

    let newline_appended = files
        .append_text(
            owner,
            ws,
            AppendText {
                target: WriteTarget::Existing { node_id: note_id },
                content: "epsilon\n".to_owned(),
                expected_sha256: Some(after_append_sha.clone()),
                ensure_newline: true,
            },
        )
        .await?;
    assert_eq!(newline_appended.text.line_count, 3);
    let after_write_sha = newline_appended.text.content_sha256.clone();

    let stale_append = files
        .append_text(
            owner,
            ws,
            AppendText {
                target: WriteTarget::Existing { node_id: note_id },
                content: "stale".to_owned(),
                expected_sha256: Some(after_initial_write_sha),
                ensure_newline: false,
            },
        )
        .await
        .expect_err("append must reject stale expected_sha256");
    assert!(stale_append.to_string().contains("expected_sha256"));

    // --- read: range slice returns the content + metrics ---
    let read = files
        .read_text(
            owner,
            ws,
            ReadText {
                node_id: note_id,
                start_line: Some(1),
                max_lines: Some(10),
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await?;
    let content = match read.body {
        notegate_service::files::ReadTextBody::Content(content) => content,
        other => panic!("expected content body, got {other:?}"),
    };
    assert_eq!(content.returned_lines, 3);
    assert!(content.content.contains("alpha beta gammadelta\nepsilon"));
    assert_eq!(read.content_sha256, after_write_sha);

    // --- encrypted text: REST-visible opaque payload, not plaintext-patchable ---
    let encrypted = files
        .write_text(
            owner,
            ws,
            WriteText {
                target: WriteTarget::Create {
                    parent_node_id: projects_id,
                    name: "secret.md".to_owned(),
                },
                body: WriteTextBody::Encrypted(json!({
                    "version": 1,
                    "alg": "AES-256-GCM",
                    "ciphertext_b64": "abc"
                })),
                expected_sha256: None,
            },
        )
        .await?;
    assert_eq!(encrypted.text.line_count, 0);
    assert!(encrypted.text.content.is_none());
    assert!(encrypted.text.encrypted_payload.is_some());
    let encrypted_read = files
        .read_text(
            owner,
            ws,
            ReadText {
                node_id: encrypted.node.node.id,
                start_line: None,
                max_lines: None,
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await?;
    match encrypted_read.body {
        ReadTextBody::Encrypted(payload) => assert_eq!(payload["alg"], "AES-256-GCM"),
        other => panic!("expected encrypted body, got {other:?}"),
    }
    let patch_err = files
        .patch_text(
            owner,
            ws,
            PatchText {
                node_id: encrypted.node.node.id,
                edits: vec![Edit {
                    old_text: "abc".to_owned(),
                    new_text: "def".to_owned(),
                }],
                expected_sha256: None,
            },
        )
        .await
        .expect_err("encrypted text patch must fail");
    assert!(patch_err.to_string().contains("plaintext"));

    // --- file: upload small inline bytes and read them back ---
    let uploaded = files
        .create_file(
            owner,
            ws,
            CreateFile {
                parent_node_id: projects_id,
                name: "diagram.bin".to_owned(),
                bytes: b"binary-data".to_vec(),
                media_type: "application/octet-stream".to_owned(),
                original_filename: Some("diagram.bin".to_owned()),
                encryption_mode: FileEncryptionMode::None,
                encryption_metadata: None,
            },
        )
        .await?;
    assert_eq!(uploaded.node.path, "/projects/diagram.bin");
    assert_eq!(uploaded.file.byte_len, 11);
    assert_eq!(
        uploaded.node.file.as_ref().expect("file stats").byte_len,
        11
    );

    let downloaded = files.read_file(owner, ws, uploaded.node.node.id).await?;
    assert_eq!(downloaded.bytes, b"binary-data");
    assert_eq!(downloaded.file.content_sha256, uploaded.file.content_sha256);

    let page = files
        .children(
            owner,
            ws,
            projects_id,
            notegate_service::files::ChildrenRequest {
                limit: Some(100),
                cursor: None,
            },
        )
        .await?;
    let listed_file = page
        .items
        .iter()
        .find(|item| item.node.name == "diagram.bin")
        .expect("file appears in ls");
    assert_eq!(
        listed_file
            .file
            .as_ref()
            .expect("listed file stats")
            .media_type,
        "application/octet-stream"
    );

    // --- patch: exact-match replacement ---
    let patched = files
        .patch_text(
            owner,
            ws,
            PatchText {
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
    let (_, doc_now) = repo_find_text(&repo, ws, note_id).await;
    assert!(
        doc_now
            .content
            .as_deref()
            .unwrap()
            .contains("alpha delta gamma")
    );
    assert_eq!(
        doc_now.updated_by_account_id, owner,
        "patch sets doc updated_by"
    );

    // --- find by NAME: q='note' matches the text by name ---
    let by_name = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "note".to_owned(),
                path: None,
                kind: None,
                match_mode: FindMatchMode::Contains,
                limit: Some(50),
                cursor: None,
            },
        )
        .await?;
    assert!(
        by_name.items.iter().any(|item| item.node.id == note_id),
        "find by name must match the text"
    );

    // --- find remains name-only: q='projects' matches the folder, not the doc via path ---
    let by_path = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "projects".to_owned(),
                path: None,
                kind: None,
                match_mode: FindMatchMode::Contains,
                limit: Some(50),
                cursor: None,
            },
        )
        .await?;
    assert!(
        by_path.items.iter().all(|item| item.node.id != note_id),
        "find must not match the text by path substring"
    );

    // --- grep: content candidate by body substring, with derived path ---
    let candidates = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "alpha delta".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: Some(20),
                cursor: None,
            },
        )
        .await?;
    let hit = candidates
        .items
        .iter()
        .find(|item| item.node.node.id == note_id)
        .expect("grep candidate present");
    assert_eq!(
        hit.node.path, "/projects/note.md",
        "grep returns derived path"
    );
    assert_eq!(
        hit.node.text.as_ref().expect("text stats").byte_len,
        doc_now.byte_len
    );

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
    assert_eq!(
        moved.node.updated_by_account_id, owner,
        "move sets updated_by"
    );

    // find is name-only; the moved text keeps its name, and its derived
    // path (for display) reflects the move even though the match is on name.
    let by_name = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "note".to_owned(),
                path: None,
                kind: None,
                match_mode: FindMatchMode::Contains,
                limit: Some(50),
                cursor: None,
            },
        )
        .await?;
    let hit = by_name.items.iter().find(|item| item.node.id == note_id);
    assert!(hit.is_some(), "find by name must hit the moved text");
    assert_eq!(
        hit.map(|item| item.path.as_str()),
        Some("/archive/note.md"),
        "derived path reflects the move",
    );

    // --- rm: hide the moved text and mark it purge-eligible later ---
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
            "SELECT deleted_by_account_id, purge_after FROM nodes WHERE space_id = $1 AND id = $2",
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

/// `append` branch coverage the lifecycle test does not exercise: create-on-append,
/// the create + `expected_sha256` conflict, the empty-text `ensure_newline` guard,
/// and the encrypted-text rejection.
#[tokio::test]
async fn append_text_branches() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let ws_repo = SpaceRepo::new(db.pool.clone());
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));

    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "appends").await;

    // --- create-on-append: a missing text is created with the appended content ---
    let created = files
        .append_text(
            owner,
            ws,
            AppendText {
                target: WriteTarget::Create {
                    parent_node_id: root,
                    name: "log.md".to_owned(),
                },
                content: "first".to_owned(),
                expected_sha256: None,
                ensure_newline: true,
            },
        )
        .await?;
    assert_eq!(created.node.path, "/log.md");
    assert_eq!(
        created.text.line_count, 1,
        "single line, no leading newline"
    );
    let log_id = created.node.node.id;

    // --- create + expected_sha256 is a conflict: nothing exists to guard against ---
    let create_guard_err = files
        .append_text(
            owner,
            ws,
            AppendText {
                target: WriteTarget::Create {
                    parent_node_id: root,
                    name: "other.md".to_owned(),
                },
                content: "x".to_owned(),
                expected_sha256: Some("deadbeef".to_owned()),
                ensure_newline: false,
            },
        )
        .await
        .expect_err("create-on-append must reject expected_sha256");
    assert!(create_guard_err.to_string().contains("expected_sha256"));

    // --- ensure_newline guard: a non-empty body without a trailing newline gets one ---
    let joined = files
        .append_text(
            owner,
            ws,
            AppendText {
                target: WriteTarget::Existing { node_id: log_id },
                content: "second".to_owned(),
                expected_sha256: Some(created.text.content_sha256.clone()),
                ensure_newline: true,
            },
        )
        .await?;
    assert_eq!(joined.text.line_count, 2, "ensure_newline split the lines");
    let read = files
        .read_text(
            owner,
            ws,
            ReadText {
                node_id: log_id,
                start_line: None,
                max_lines: None,
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await?;
    match read.body {
        ReadTextBody::Content(content) => assert_eq!(content.content, "first\nsecond"),
        other => panic!("expected content body, got {other:?}"),
    }

    // --- encrypted text cannot be appended as plaintext ---
    let encrypted = files
        .write_text(
            owner,
            ws,
            WriteText {
                target: WriteTarget::Create {
                    parent_node_id: root,
                    name: "secret.md".to_owned(),
                },
                body: WriteTextBody::Encrypted(json!({
                    "version": 1,
                    "alg": "AES-256-GCM",
                    "ciphertext_b64": "abc"
                })),
                expected_sha256: None,
            },
        )
        .await?;
    let encrypted_err = files
        .append_text(
            owner,
            ws,
            AppendText {
                target: WriteTarget::Existing {
                    node_id: encrypted.node.node.id,
                },
                content: "plain".to_owned(),
                expected_sha256: None,
                ensure_newline: false,
            },
        )
        .await
        .expect_err("append to encrypted text must fail");
    assert!(encrypted_err.to_string().contains("plaintext"));

    db.cleanup().await;
    Ok(())
}

/// Repo-level root delete must return a clean conflict instead of relying on the
/// root CHECK constraint to fail the UPDATE as an internal DB error.
#[tokio::test]
async fn repo_soft_delete_root_is_conflict() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let ws_repo = SpaceRepo::new(db.pool.clone());
    let repo = FilesRepo::new(db.pool.clone());

    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "rootguard").await;

    let err = repo
        .soft_delete_node(ws, root, owner)
        .await
        .expect_err("root delete must be rejected cleanly");
    assert!(
        matches!(err, Error::Conflict(ref message) if message == "cannot delete the root node"),
        "root delete should be a conflict, got {err:?}"
    );

    let root_deleted_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM nodes WHERE space_id = $1 AND id = $2")
            .bind(ws)
            .bind(root)
            .fetch_one(&db.pool)
            .await?;
    assert!(root_deleted_at.is_none(), "root remains live");

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
    let ws_repo = SpaceRepo::new(db.pool.clone());
    let repo = FilesRepo::new(db.pool.clone());

    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "wide").await;

    // 250 texts directly under root (the folder fanout cap is 200, so insert
    // via raw SQL to exercise paging beyond a single page without the cap).
    for index in 0..250 {
        sqlx::query(
            "INSERT INTO nodes (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id) \
             VALUES ($1, $2, $3, 'text', $4, $4)",
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
        let (rows, has_more) = repo.paged_children(ws, root, 100, cursor.as_ref()).await?;
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

/// Scoped find: a folder scope restricts results to that subtree.
#[tokio::test]
async fn find_scope_restricts_to_subtree() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let ws_repo = SpaceRepo::new(db.pool.clone());
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let search = SearchService::new(FilesRepo::new(db.pool.clone()));

    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "scoped").await;

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
        .create_text(
            owner,
            ws,
            CreateText {
                parent_node_id: inside,
                name: "target.md".to_owned(),
            },
        )
        .await?
        .node
        .node
        .id;
    let outside_doc = files
        .create_text(
            owner,
            ws,
            CreateText {
                parent_node_id: outside,
                name: "target.md".to_owned(),
            },
        )
        .await?
        .node
        .node
        .id;

    // Unscoped find matches both target.md texts.
    let all = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "target".to_owned(),
                path: None,
                kind: None,
                match_mode: FindMatchMode::Contains,
                limit: Some(50),
                cursor: None,
            },
        )
        .await?;
    assert!(all.items.iter().any(|item| item.node.id == inside_doc));
    assert!(all.items.iter().any(|item| item.node.id == outside_doc));

    // Scoped to /inside matches only the inside text.
    let scoped = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "target".to_owned(),
                path: Some("/inside".to_owned()),
                kind: None,
                match_mode: FindMatchMode::Contains,
                limit: Some(50),
                cursor: None,
            },
        )
        .await?;
    assert!(scoped.items.iter().any(|item| item.node.id == inside_doc));
    assert!(
        !scoped.items.iter().any(|item| item.node.id == outside_doc),
        "scope must exclude nodes outside the subtree"
    );

    db.cleanup().await;
    Ok(())
}

/// Load a text through the repo, panicking if missing (test helper).
async fn repo_find_text(
    repo: &FilesRepo,
    space_id: Uuid,
    node_id: Uuid,
) -> (notegate_model::Node, notegate_model::TextObject) {
    repo.find_text(space_id, node_id)
        .await
        .expect("find_text query")
        .expect("text present")
}
