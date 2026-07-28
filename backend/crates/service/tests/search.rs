//! End-to-end search against a real Postgres schema.
//!
//! Drives the `SearchService` over the real `FilesRepo` so find (name, kind
//! filter, scope) and grep (text-node candidates + DFS cursor) run through the
//! same service/repository path as the REST/MCP surfaces.
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

use common::{TestDb, insert_user_account, setup_space};
use notegate_core::{SearchBodyCacheConfig, limits};
use notegate_db::{FilesRepo, SpaceRepo};
use notegate_model::{AccountKind, UpdateSpace};
use notegate_service::ServiceError;
use notegate_service::files::{
    CreateFolder, CreateText, FilesService, UpdateNodeSearchPolicy, WriteTarget, WriteText,
    WriteTextBody,
};
use notegate_service::search::{
    FindMatchMode, FindRequest, GrepLineMode, GrepMatchMode, GrepRequest, SearchService,
    TreeRequest,
};
use uuid::Uuid;

/// Build the trio of services/repos used by every test.
fn services(db: &TestDb) -> (SpaceRepo, FilesService, SearchService) {
    let ws_repo = SpaceRepo::new(db.pool.clone());
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let search = SearchService::new(FilesRepo::new(db.pool.clone()));
    (ws_repo, files, search)
}

async fn enable_default_text_encryption(
    db: &TestDb,
    ws_repo: &SpaceRepo,
    owner: Uuid,
    ws: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::query("UPDATE users SET tier = 'system_max' WHERE id = $1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    ws_repo
        .update_space_with_features(
            owner,
            &UpdateSpace {
                space_id: ws,
                name: None,
                sort_order: None,
                navigation_pinned: None,
                user_mcp_enabled: None,
                default_search_enabled: None,
                default_text_encryption_enabled: Some(true),
            },
        )
        .await?;
    Ok(())
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
                body: WriteTextBody::Plain(content.to_owned()),
                expected_sha256: None,
            },
        )
        .await
        .expect("write");
    node
}

#[tokio::test]
async fn search_policy_excludes_only_the_selected_node() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner =
        insert_user_account(&db.pool, "search-policy", "search-policy@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "search-policy").await;

    let hidden_folder = mkdir(&files, owner, ws, root, "hidden-folder").await;
    let visible_text = write_doc(
        &files,
        owner,
        ws,
        hidden_folder,
        "visible.md",
        "visible marker",
    )
    .await;
    let hidden_text = write_doc(&files, owner, ws, root, "hidden.md", "hidden marker").await;

    for node_id in [hidden_folder, hidden_text] {
        files
            .update_node_search_policy(
                AccountKind::User,
                owner,
                ws,
                UpdateNodeSearchPolicy {
                    node_id,
                    enabled: false,
                },
            )
            .await?;
    }

    let folder_find = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "hidden".to_owned(),
                path: None,
                kind: None,
                match_mode: FindMatchMode::Contains,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert!(folder_find.items.is_empty());

    let child_find = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "visible".to_owned(),
                path: None,
                kind: None,
                match_mode: FindMatchMode::Contains,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(child_find.items[0].node.id, visible_text);

    let hidden_grep = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "hidden marker".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert!(hidden_grep.items.is_empty());

    let visible_grep = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "visible marker".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(visible_grep.items[0].node.node.id, visible_text);

    db.cleanup().await;
    Ok(())
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
                match_mode: FindMatchMode::Contains,
                include: Vec::new(),
                exclude: Vec::new(),
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

    let explicit_root = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "note".to_owned(),
                path: Some("/".to_owned()),
                kind: None,
                match_mode: FindMatchMode::Contains,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    let result_paths = |page: &notegate_service::search::FindPage| {
        page.items
            .iter()
            .map(|view| (view.node.id, view.path.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        result_paths(&explicit_root),
        result_paths(&by_name),
        "path='/' is identical to the default root search"
    );

    let projects_only = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "note".to_owned(),
                path: None,
                kind: None,
                match_mode: FindMatchMode::Contains,
                include: vec!["/projects/*".to_owned()],
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    let projects_only_ids: Vec<Uuid> = projects_only.items.iter().map(|v| v.node.id).collect();
    assert_eq!(projects_only_ids, vec![proj_note]);

    let excluded_projects = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "note".to_owned(),
                path: None,
                kind: None,
                match_mode: FindMatchMode::Contains,
                include: Vec::new(),
                exclude: vec!["/projects/*".to_owned()],
                limit: None,
                cursor: None,
            },
        )
        .await?;
    let excluded_projects_ids: Vec<Uuid> =
        excluded_projects.items.iter().map(|v| v.node.id).collect();
    assert_eq!(excluded_projects_ids, vec![other_note]);

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
                match_mode: FindMatchMode::Contains,
                include: Vec::new(),
                exclude: Vec::new(),
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
                match_mode: FindMatchMode::Contains,
                include: Vec::new(),
                exclude: Vec::new(),
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
                match_mode: FindMatchMode::Contains,
                include: Vec::new(),
                exclude: Vec::new(),
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
                match_mode: FindMatchMode::Contains,
                include: Vec::new(),
                exclude: Vec::new(),
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

/// Search scope paths are folder scopes; text scopes and unresolved scopes are rejected.
#[tokio::test]
async fn search_scope_requires_folder_and_rejects_missing_path()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "personal").await;

    let _note = write_doc(
        &files,
        owner,
        ws,
        root,
        "note.md",
        "alpha
needle
",
    )
    .await;
    let _other = write_doc(
        &files, owner, ws, root, "other.md", "needle
",
    )
    .await;

    let text_scope = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "needle".to_owned(),
                path: Some("/note.md".to_owned()),
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        text_scope,
        ServiceError::InvalidInput("search scope must be a folder".to_owned())
    );

    let missing = search
        .find(
            owner,
            ws,
            FindRequest {
                q: "note".to_owned(),
                path: Some("/missing".to_owned()),
                kind: None,
                match_mode: FindMatchMode::Contains,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        missing,
        ServiceError::NotFound("scope path not found".to_owned())
    );

    db.cleanup().await;
    Ok(())
}

/// grep returns matching text node candidates with path and text stats.
#[tokio::test]
async fn grep_returns_text_node_candidates() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "personal").await;

    let content = "l1
l2
l3
needle here
l5
needle again
l7
";
    let node = write_doc(&files, owner, ws, root, "note.md", content).await;

    let page = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "  needle  ".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(page.items.len(), 1, "exactly one matching text node");
    let item = &page.items[0];
    assert_eq!(item.node.node.id, node);
    assert_eq!(item.node.path, "/note.md", "grep returns the derived path");
    assert!(
        item.match_lines.is_empty(),
        "default grep returns file candidates only"
    );
    let stats = item
        .node
        .text
        .as_ref()
        .expect("grep text candidate has text stats");
    assert_eq!(stats.line_count, 7);
    assert_eq!(stats.byte_len, content.len() as i64);

    let first_line = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "needle".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::First,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(first_line.items[0].match_lines, vec![4]);

    let all_lines = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "needle".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::All,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(all_lines.items[0].match_lines, vec![4, 6]);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn grep_does_not_decrypt_server_encrypted_rows_rejected_by_path_filters()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(
        &db.pool,
        "grep-filter-encrypted",
        "grep-filter-encrypted@example.test",
    )
    .await?;
    let (ws, root) = setup_space(&ws_repo, owner, "grep-filter-encrypted").await;
    enable_default_text_encryption(&db, &ws_repo, owner, ws).await?;

    let allowed = write_doc(
        &files,
        owner,
        ws,
        root,
        "allowed.md",
        "needle in allowed text",
    )
    .await;
    let excluded = write_doc(
        &files,
        owner,
        ws,
        root,
        "excluded.md",
        "needle in excluded text",
    )
    .await;
    sqlx::query(
        "UPDATE text_objects SET content_ciphertext = decode('00', 'hex') \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(excluded)
    .execute(&db.pool)
    .await?;

    let page = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "needle".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: vec!["/allowed.md".to_owned()],
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].node.node.id, allowed);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn grep_reuses_cached_encrypted_body_until_content_sha_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner =
        insert_user_account(&db.pool, "grep-body-cache", "grep-body-cache@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "grep-body-cache").await;
    enable_default_text_encryption(&db, &ws_repo, owner, ws).await?;
    let node_id = write_doc(&files, owner, ws, root, "cached.md", "original marker").await;

    let request = |q: &str| GrepRequest {
        q: q.to_owned(),
        path: None,
        match_mode: GrepMatchMode::Literal,
        line_mode: GrepLineMode::None,
        include: Vec::new(),
        exclude: Vec::new(),
        limit: None,
        cursor: None,
    };

    let first = search.grep(owner, ws, request("original marker")).await?;
    assert_eq!(first.items[0].node.node.id, node_id);

    let original_ciphertext: Vec<u8> = sqlx::query_scalar(
        "SELECT content_ciphertext FROM text_objects WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(node_id)
    .fetch_one(&db.pool)
    .await?;
    sqlx::query(
        "UPDATE text_objects SET content_ciphertext = decode('00', 'hex') \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(node_id)
    .execute(&db.pool)
    .await?;

    let cloned_search = search.clone();
    let cached = cloned_search
        .grep(owner, ws, request("original marker"))
        .await?;
    assert_eq!(cached.items[0].node.node.id, node_id);

    sqlx::query(
        "UPDATE text_objects SET content_ciphertext = $3 \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(node_id)
    .bind(original_ciphertext)
    .execute(&db.pool)
    .await?;
    files
        .write_text(
            owner,
            ws,
            WriteText {
                target: WriteTarget::Existing { node_id },
                body: WriteTextBody::Plain("replacement marker".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;
    sqlx::query(
        "UPDATE text_objects SET content_ciphertext = decode('00', 'hex') \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(node_id)
    .execute(&db.pool)
    .await?;

    let error = cloned_search
        .grep(owner, ws, request("replacement marker"))
        .await
        .expect_err("changed content SHA must miss the old cached body");
    assert!(matches!(error, ServiceError::Internal(_)));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn grep_body_cache_can_be_disabled() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, _search) = services(&db);
    let search = SearchService::with_body_cache_config(
        FilesRepo::new(db.pool.clone()),
        SearchBodyCacheConfig {
            max_capacity_bytes: 0,
            ..SearchBodyCacheConfig::default()
        },
    );
    let owner = insert_user_account(
        &db.pool,
        "grep-body-cache-disabled",
        "grep-body-cache-disabled@example.test",
    )
    .await?;
    let (ws, root) = setup_space(&ws_repo, owner, "grep-body-cache-disabled").await;
    enable_default_text_encryption(&db, &ws_repo, owner, ws).await?;
    let node_id = write_doc(&files, owner, ws, root, "uncached.md", "uncached marker").await;
    let request = || GrepRequest {
        q: "uncached marker".to_owned(),
        path: None,
        match_mode: GrepMatchMode::Literal,
        line_mode: GrepLineMode::None,
        include: Vec::new(),
        exclude: Vec::new(),
        limit: None,
        cursor: None,
    };

    let first = search.grep(owner, ws, request()).await?;
    assert_eq!(first.items[0].node.node.id, node_id);
    sqlx::query(
        "UPDATE text_objects SET content_ciphertext = decode('00', 'hex') \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(node_id)
    .execute(&db.pool)
    .await?;

    let error = search
        .grep(owner, ws, request())
        .await
        .expect_err("disabled body cache must reload and decrypt the body");
    assert!(matches!(error, ServiceError::Internal(_)));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn grep_does_not_decrypt_server_encrypted_rows_beyond_byte_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(
        &db.pool,
        "grep-budget-encrypted",
        "grep-budget-encrypted@example.test",
    )
    .await?;
    let (ws, root) = setup_space(&ws_repo, owner, "grep-budget-encrypted").await;
    enable_default_text_encryption(&db, &ws_repo, owner, ws).await?;

    let mut content = "needle".to_owned();
    content.push_str(&"x".repeat(limits::TEXT_MAX_BYTES - content.len()));
    let documents_within_budget = limits::GREP_SCAN_MAX_BYTES / limits::TEXT_MAX_BYTES;
    let mut node_ids = Vec::new();
    for index in 0..=documents_within_budget {
        node_ids.push(
            write_doc(
                &files,
                owner,
                ws,
                root,
                &format!("budget-{index:02}.md"),
                &content,
            )
            .await,
        );
    }

    let beyond_budget = node_ids[documents_within_budget];
    let original_ciphertext: Vec<u8> = sqlx::query_scalar(
        "SELECT content_ciphertext FROM text_objects WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(beyond_budget)
    .fetch_one(&db.pool)
    .await?;
    sqlx::query(
        "UPDATE text_objects SET content_ciphertext = decode('00', 'hex') \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(beyond_budget)
    .execute(&db.pool)
    .await?;

    let first = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "needle".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;

    assert_eq!(first.items.len(), documents_within_budget);
    assert!(
        first
            .items
            .iter()
            .map(|item| item.node.node.id)
            .eq(node_ids[..documents_within_budget].iter().copied())
    );
    assert!(first.has_more);
    assert!(first.next_cursor.is_some());

    sqlx::query(
        "UPDATE text_objects SET content_ciphertext = $3 \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(ws)
    .bind(beyond_budget)
    .bind(original_ciphertext)
    .execute(&db.pool)
    .await?;

    let second = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "needle".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: first.next_cursor,
            },
        )
        .await?;

    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].node.node.id, beyond_budget);
    assert!(!second.has_more);
    assert!(second.next_cursor.is_none());

    db.cleanup().await;
    Ok(())
}

/// A nested scope keeps exact paths and subtree boundaries; find pagination
/// resumes in the same DFS order after descending into a folder.
#[tokio::test]
async fn find_cursor_descends_before_later_siblings() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "personal").await;

    let a = mkdir(&files, owner, ws, root, "a").await;
    let b = mkdir(&files, owner, ws, a, "b").await;
    let c = mkdir(&files, owner, ws, b, "c").await;
    let _outside = write_doc(&files, owner, ws, b, "outside-match.md", "needle outside\n").await;
    let _first = write_doc(&files, owner, ws, c, "a-match.md", "needle first\n").await;
    let nested = mkdir(&files, owner, ws, c, "m-match").await;
    let _nested = write_doc(&files, owner, ws, nested, "b-match.md", "needle nested\n").await;
    let _last = write_doc(&files, owner, ws, c, "z-match.md", "needle last\n").await;

    let find_request = |path, limit, cursor| FindRequest {
        q: "match".to_owned(),
        path,
        kind: None,
        match_mode: FindMatchMode::Contains,
        include: Vec::new(),
        exclude: Vec::new(),
        limit,
        cursor,
    };
    let expected_find_paths = vec![
        "/a/b/c/a-match.md",
        "/a/b/c/m-match",
        "/a/b/c/m-match/b-match.md",
        "/a/b/c/z-match.md",
    ];

    let scoped = search
        .find(
            owner,
            ws,
            find_request(Some("/a/b/c/   ".to_owned()), None, None),
        )
        .await?;
    assert_eq!(
        scoped
            .items
            .iter()
            .map(|view| view.path.as_str())
            .collect::<Vec<_>>(),
        expected_find_paths,
        "deep scope returns exact canonical paths and excludes sibling subtrees"
    );

    let scoped_grep = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "needle".to_owned(),
                path: Some("/a/b/c".to_owned()),
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(
        scoped_grep
            .items
            .iter()
            .map(|hit| hit.node.path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "/a/b/c/a-match.md",
            "/a/b/c/m-match/b-match.md",
            "/a/b/c/z-match.md",
        ],
        "grep returns only text nodes inside the deep scope"
    );

    let first = search
        .find(
            owner,
            ws,
            find_request(Some("/a/b/c".to_owned()), Some(2), None),
        )
        .await?;
    assert!(first.has_more);

    let second = search
        .find(
            owner,
            ws,
            find_request(Some("/a/b/c".to_owned()), Some(2), first.next_cursor),
        )
        .await?;
    assert!(!second.has_more);
    assert!(second.next_cursor.is_none());
    let paged_paths = first
        .items
        .iter()
        .chain(&second.items)
        .map(|view| view.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paged_paths, expected_find_paths,
        "two scoped pages preserve DFS order without duplicates or omissions"
    );

    db.cleanup().await;
    Ok(())
}

/// tree returns bounded-depth DFS node summaries without exposing ids at the MCP layer.
#[tokio::test]
async fn tree_returns_depth_limited_subtree() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "personal").await;

    let a = mkdir(&files, owner, ws, root, "a").await;
    let b = mkdir(&files, owner, ws, a, "b").await;
    let _c = write_doc(&files, owner, ws, b, "c.md", "nested\n").await;
    let _z = write_doc(&files, owner, ws, root, "z.md", "sibling\n").await;

    let depth_one = search
        .tree(
            owner,
            ws,
            TreeRequest {
                path: None,
                depth: Some(1),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    let depth_one_paths: Vec<&str> = depth_one
        .items
        .iter()
        .map(|view| view.path.as_str())
        .collect();
    assert_eq!(depth_one_paths, vec!["/a", "/z.md"]);
    assert!(!depth_one.has_more);

    let depth_two = search
        .tree(
            owner,
            ws,
            TreeRequest {
                path: None,
                depth: Some(2),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    let depth_two_paths: Vec<&str> = depth_two
        .items
        .iter()
        .map(|view| view.path.as_str())
        .collect();
    assert_eq!(depth_two_paths, vec!["/a", "/a/b", "/z.md"]);

    let scoped = search
        .tree(
            owner,
            ws,
            TreeRequest {
                path: Some("/a".to_owned()),
                depth: Some(2),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    let scoped_paths: Vec<&str> = scoped.items.iter().map(|view| view.path.as_str()).collect();
    assert_eq!(scoped_paths, vec!["/a/b", "/a/b/c.md"]);

    db.cleanup().await;
    Ok(())
}

/// grep uses the same DFS cursor semantics as find while returning text-node
/// candidates only.
#[tokio::test]
async fn grep_cursor_descends_before_later_siblings() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "personal").await;

    let a = mkdir(&files, owner, ws, root, "a").await;
    let nested = write_doc(&files, owner, ws, a, "b.md", "needle nested\n").await;
    let sibling = write_doc(&files, owner, ws, root, "z.md", "needle sibling\n").await;

    let first = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "needle".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: Some(1),
                cursor: None,
            },
        )
        .await?;
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].node.node.id, nested);
    assert!(first.has_more);

    let second = search
        .grep(
            owner,
            ws,
            GrepRequest {
                q: "needle".to_owned(),
                path: None,
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: Some(1),
                cursor: first.next_cursor,
            },
        )
        .await?;
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].node.node.id, sibling);

    db.cleanup().await;
    Ok(())
}

/// find DFS cursor round-trips across page boundaries with no dup/loss when the
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
                    match_mode: FindMatchMode::Contains,
                    include: Vec::new(),
                    exclude: Vec::new(),
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
                    "names strictly increase across the traversal page"
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

/// grep DFS cursor pages across matching text-node candidates with no dup/loss.
#[tokio::test]
async fn grep_cursor_pages_across_text_nodes() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (ws_repo, files, search) = services(&db);
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let (ws, root) = setup_space(&ws_repo, owner, "personal").await;

    let total = 5usize;
    let mut expected_ids = Vec::new();
    for index in 0..total {
        let node = write_doc(
            &files,
            owner,
            ws,
            root,
            &format!("many-{index}.md"),
            &format!(
                "pad
hit-{index}
"
            ),
        )
        .await;
        expected_ids.push(node);
    }
    let encrypted = files
        .write_text(
            owner,
            ws,
            WriteText {
                target: WriteTarget::Create {
                    parent_node_id: root,
                    name: "encrypted.md".to_owned(),
                },
                body: WriteTextBody::Encrypted(serde_json::json!({
                    "version": 1,
                    "alg": "AES-256-GCM",
                    "ciphertext_b64": "hit-client-encrypted"
                })),
                expected_sha256: None,
            },
        )
        .await?
        .node
        .node
        .id;

    let limit = 2i64;
    let mut seen: Vec<Uuid> = Vec::new();
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
                    match_mode: GrepMatchMode::Literal,
                    line_mode: GrepLineMode::None,
                    include: Vec::new(),
                    exclude: Vec::new(),
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
        for item in &page.items {
            seen.push(item.node.node.id);
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

    seen.sort();
    expected_ids.sort();
    assert_eq!(
        seen, expected_ids,
        "all matching texts returned exactly once"
    );
    assert!(
        !seen.contains(&encrypted),
        "client-encrypted text is excluded even when its opaque payload matches"
    );

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
                match_mode: FindMatchMode::Contains,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: Some("!!!not-a-cursor!!!".to_owned()),
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            find_err,
            ServiceError::InvalidInput(ref message) if message == "invalid cursor"
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
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::None,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: Some("!!!not-a-cursor!!!".to_owned()),
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            grep_err,
            ServiceError::InvalidInput(ref message) if message == "invalid cursor"
        ),
        "grep rejects a garbage cursor as invalid input, got {grep_err:?}"
    );

    db.cleanup().await;
    Ok(())
}
