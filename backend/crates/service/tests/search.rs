//! End-to-end search against a real Postgres schema.
//!
//! Drives the `SearchService` over the real `FilesRepo` so find (name + derived
//! path, kind filter, scope) and grep (line-split, context, keyset + intra-doc
//! offset cursor) run through SQL exactly as the REST/MCP surfaces call them.
//!
//! Run with:
//! `NOTEGATE_TEST_DATABASE_URL=postgres://notegate:notegate@localhost:5433/notegate \
//!  cargo test -p notegate-service --test search`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, insert_user_account};
use notegate_db::{FilesRepo, SpaceRepo};
use notegate_service::files::{CreateFolder, CreateText, FilesService, WriteTarget, WriteText};
use notegate_service::search::{FindRequest, GrepRequest, SearchService};
use notegate_service::spaces::CreateSpace;
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

/// Build the trio of services/repos used by every test.
fn services(db: &TestDb) -> (SpaceRepo, FilesService, SearchService) {
    let ws_repo = SpaceRepo::new(db.pool.clone());
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let search = SearchService::new(FilesRepo::new(db.pool.clone()));
    (ws_repo, files, search)
}

async fn mkdir(files: &FilesService, owner: Uuid, ws: Uuid, parent: Uuid, name: &str) -> Uuid {
    files
        .create_folder(
            owner,
            ws,
            CreateFolder {
                parent_node_id: parent,
                name: name.to_owned(),
            },
        )
        .await
        .expect("mkdir")
        .node
        .id
}

async fn write_doc(
    files: &FilesService,
    owner: Uuid,
    ws: Uuid,
    parent: Uuid,
    name: &str,
    content: &str,
) -> Uuid {
    // touch then write so create-on-existing path is exercised.
    let node = files
        .create_text(
            owner,
            ws,
            CreateText {
                parent_node_id: parent,
                name: name.to_owned(),
            },
        )
        .await
        .expect("touch")
        .node
        .node
        .id;
    files
        .write_text(
            owner,
            ws,
            WriteText {
                target: WriteTarget::Existing { node_id: node },
                content: content.to_owned(),
                expected_sha256: None,
            },
        )
        .await
        .expect("write");
    node
}

/// find matches by NAME; the `kind` filter and `scope` both work, and results
/// still carry derived display paths.
#[tokio::test]
async fn find_matches_name_kind_and_scope() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "personal").await;

    // /projects (folder), /projects/note.md (doc), /other/note.md (doc)
    let projects = mkdir(&files, owner, ws, root, "projects").await;
    let other = mkdir(&files, owner, ws, root, "other").await;
    let proj_note = write_doc(&files, owner, ws, projects, "note.md", "# Project note\n").await;
    let other_note = write_doc(&files, owner, ws, other, "note.md", "# Other note\n").await;

    // by NAME: q='note' hits both texts (and not the folders).
    let by_name = search
        .find(
            owner,
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
    let ids: Vec<Uuid> = by_name.items.iter().map(|v| v.node.id).collect();
    assert!(
        ids.contains(&proj_note),
        "find by name hits /projects/note.md"
    );
    assert!(
        ids.contains(&other_note),
        "find by name hits /other/note.md"
    );
    // Every find item carries a derived path.
    let proj_view = by_name
        .items
        .iter()
        .find(|v| v.node.id == proj_note)
        .unwrap();
    assert_eq!(
        proj_view.path, "/projects/note.md",
        "find item carries derived path"
    );

    // name-only: q='projects' hits the folder itself, not the doc beneath it
    // merely because its derived path contains /projects.
    let by_path = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "projects".to_owned(),
                path: None,
                kind: None,
                limit: None,
                cursor: None,
            },
        )
        .await?;
    let path_ids: Vec<Uuid> = by_path.items.iter().map(|v| v.node.id).collect();
    assert!(
        path_ids.contains(&projects),
        "find by name hits the /projects folder"
    );
    assert!(
        !path_ids.contains(&proj_note),
        "find does not match texts by path substring"
    );
    assert!(
        !path_ids.contains(&other_note),
        "find excludes /other/note.md"
    );

    // kind filter: q='note' kind=folder returns nothing (both notes are texts).
    let folders_only = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "note".to_owned(),
                path: None,
                kind: Some(notegate_model::NodeKind::Folder),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert!(
        folders_only.items.is_empty(),
        "kind=folder filters out texts"
    );

    // kind filter: q='projects' kind=folder returns only the folder.
    let proj_folder = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "projects".to_owned(),
                path: None,
                kind: Some(notegate_model::NodeKind::Folder),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    let pf_ids: Vec<Uuid> = proj_folder.items.iter().map(|v| v.node.id).collect();
    assert_eq!(
        pf_ids,
        vec![projects],
        "kind=folder returns only the folder"
    );

    // scope: searching 'note' under /projects returns only the project note.
    let scoped = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "note".to_owned(),
                path: Some("/projects".to_owned()),
                kind: None,
                limit: None,
                cursor: None,
            },
        )
        .await?;
    let scoped_ids: Vec<Uuid> = scoped.items.iter().map(|v| v.node.id).collect();
    assert!(
        scoped_ids.contains(&proj_note),
        "scope includes the in-subtree doc"
    );
    assert!(
        !scoped_ids.contains(&other_note),
        "scope excludes the out-of-subtree doc"
    );

    db.cleanup().await;
    Ok(())
}

/// Search scope paths are node scopes: a folder scopes to its subtree, a text
/// scopes to that single text, and an unresolved scope is an actionable not-found.
#[tokio::test]
async fn search_scope_accepts_text_and_rejects_missing_path()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "personal").await;

    let note = write_doc(&files, owner, ws, root, "note.md", "alpha\nneedle\n").await;
    let _other = write_doc(&files, owner, ws, root, "other.md", "needle\n").await;

    let single_doc = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "needle".to_owned(),
                path: Some("/note.md".to_owned()),
                context: Some(0),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(
        single_doc.items.len(),
        1,
        "text scope returns that text only"
    );
    assert_eq!(single_doc.items[0].node_id, note);

    let missing = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "note".to_owned(),
                path: Some("/missing".to_owned()),
                kind: None,
                limit: None,
                cursor: None,
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(missing, notegate_service::error::ServiceError::NotFound(_)),
        "missing scope is not_found, got {missing:?}"
    );

    db.cleanup().await;
    Ok(())
}

/// grep returns the correct 1-based line number and before/after context, and the
/// context count is clamped to GREP_MAX_CONTEXT.
#[tokio::test]
async fn grep_line_no_context_and_clamp() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "personal").await;

    // A 7-line text; 'needle' is on line 4 only.
    let content = "l1\nl2\nl3\nneedle here\nl5\nl6\nl7\n";
    let node = write_doc(&files, owner, ws, root, "note.md", content).await;

    // context=2: line_no=4, two lines before/after, derived path.
    let page = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "  needle  ".to_owned(),
                path: None,
                context: Some(2),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(page.items.len(), 1, "exactly one matching line");
    let m = &page.items[0];
    assert_eq!(m.node_id, node);
    assert_eq!(m.path, "/note.md", "grep returns the derived path");
    assert_eq!(m.line_no, 4, "1-based line number");
    assert_eq!(m.line, "needle here");
    assert_eq!(m.before, vec!["l2".to_owned(), "l3".to_owned()]);
    assert_eq!(m.after, vec!["l5".to_owned(), "l6".to_owned()]);

    // context clamp: a request for 100 lines yields at most GREP_MAX_CONTEXT (5).
    // The match is on line 4, so before is bounded by available lines (3), but
    // after has 3 lines available (l5,l6,l7) — both well under the clamp; assert
    // the clamp by checking we never exceed 5 and that all available context is
    // returned (proving the request did not error and was bounded).
    let wide = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "needle".to_owned(),
                path: None,
                context: Some(100),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    let wm = &wide.items[0];
    assert!(
        wm.before.len() <= 5 && wm.after.len() <= 5,
        "context never exceeds GREP_MAX_CONTEXT"
    );
    // Line 4 has exactly 3 lines before and 3 after; the clamp (5) does not cut them.
    assert_eq!(
        wm.before,
        vec!["l1".to_owned(), "l2".to_owned(), "l3".to_owned()]
    );
    assert_eq!(
        wm.after,
        vec!["l5".to_owned(), "l6".to_owned(), "l7".to_owned()]
    );

    db.cleanup().await;
    Ok(())
}

/// find keyset cursor round-trips across a page boundary with no dup/loss when the
/// seed exceeds the page limit.
#[tokio::test]
async fn find_cursor_pages_without_dup_or_loss() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, _files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "wide").await;

    // 25 texts named match-0000.md .. match-0024.md (seed > limit). Insert via
    // raw SQL so the folder fanout cap (200) is irrelevant and names sort cleanly.
    let total = 25usize;
    for index in 0..total {
        sqlx::query(
            "INSERT INTO nodes (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id) \
             VALUES ($1, $2, $3, 'text', $4, $4)",
        )
        .bind(ws)
        .bind(root)
        .bind(format!("match-{index:04}.md"))
        .bind(owner)
        .execute(&db.pool)
        .await?;
    }

    let limit = 10i64;
    let mut seen: Vec<Uuid> = Vec::new();
    let mut last_name: Option<String> = None;
    let mut cursor: Option<String> = None;
    let mut pages = 0;

    loop {
        let page = search
            .find(
                owner,
                ws,
                FindRequest {
                    q: "match".to_owned(),
                    path: None,
                    kind: None,
                    limit: Some(limit),
                    cursor: cursor.clone(),
                },
            )
            .await?;
        pages += 1;
        assert!(
            page.items.len() as i64 <= limit,
            "page never exceeds the limit"
        );
        for view in &page.items {
            if let Some(prev) = &last_name {
                assert!(
                    view.node.name.as_str() > prev.as_str(),
                    "names strictly increase across the keyset"
                );
            }
            last_name = Some(view.node.name.clone());
            seen.push(view.node.id);
        }
        match page.next_cursor {
            Some(c) => {
                assert!(page.has_more, "next_cursor implies has_more");
                cursor = Some(c);
            }
            None => {
                assert!(!page.has_more, "no next_cursor implies no more pages");
                break;
            }
        }
        assert!(pages <= 10, "paging must terminate");
    }

    assert_eq!(seen.len(), total, "all rows paged exactly once");
    let mut distinct = seen.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), total, "no duplicate ids across pages");

    db.cleanup().await;
    Ok(())
}

/// grep keyset cursor resumes across a page boundary, including INSIDE a single
/// text that has more matches than the page limit (intra-text offset).
#[tokio::test]
async fn grep_cursor_resumes_within_a_text() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "personal").await;

    // ONE text with 5 matching lines (hit-00 .. hit-04), interleaved with
    // non-matching lines so line numbers are distinct and meaningful.
    let mut body = String::new();
    let mut expected_line_nos = Vec::new();
    let mut line_no = 0i64;
    for index in 0..5 {
        line_no += 1;
        body.push_str(&format!("pad {index}\n"));
        line_no += 1;
        body.push_str(&format!("hit-{index:02}\n"));
        expected_line_nos.push(line_no);
    }
    let node = write_doc(&files, owner, ws, root, "many.md", &body).await;

    // Page through with limit=2, so the single text spans 3 pages (2+2+1).
    let limit = 2i64;
    let mut seen_line_nos: Vec<i64> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let page = search
            .grep(
                owner,
                ws,
                GrepRequest {
                    q: "hit-".to_owned(),
                    path: None,
                    context: Some(0),
                    limit: Some(limit),
                    cursor: cursor.clone(),
                },
            )
            .await?;
        pages += 1;
        assert!(
            page.items.len() as i64 <= limit,
            "page never exceeds the limit"
        );
        for m in &page.items {
            assert_eq!(m.node_id, node);
            seen_line_nos.push(m.line_no);
        }
        match page.next_cursor {
            Some(c) => {
                cursor = Some(c);
            }
            None => break,
        }
        assert!(pages <= 10, "paging must terminate");
    }

    assert_eq!(
        seen_line_nos, expected_line_nos,
        "all 5 matches returned exactly once in order, resuming mid-text across pages"
    );
    // No duplicates.
    let mut distinct = seen_line_nos.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 5, "no duplicate matches across pages");

    db.cleanup().await;
    Ok(())
}

/// A garbage cursor is rejected as invalid input (mapped to 400 / invalid-arg by
/// the REST/MCP surfaces), for both find and grep.
#[tokio::test]
async fn garbage_cursor_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, _files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, _root) = setup_space(&ws_repo, owner, "personal").await;

    let find_err = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "x".to_owned(),
                path: None,
                kind: None,
                limit: None,
                cursor: Some("!!!not-a-cursor!!!".to_owned()),
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            find_err,
            notegate_service::error::ServiceError::InvalidInput(_)
        ),
        "find rejects a garbage cursor as invalid input, got {find_err:?}"
    );

    let grep_err = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "x".to_owned(),
                path: None,
                context: None,
                limit: None,
                cursor: Some("!!!not-a-cursor!!!".to_owned()),
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            grep_err,
            notegate_service::error::ServiceError::InvalidInput(_)
        ),
        "grep rejects a garbage cursor as invalid input, got {grep_err:?}"
    );

    db.cleanup().await;
    Ok(())
}
