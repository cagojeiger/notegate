use super::*;
use notegate_domain::files::{
    CreateDocument, CreateFolder, FilesError, FilesService, FindRequest, GrepRequest, MoveNode,
    SaveDocument,
};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

#[tokio::test]
async fn files_flow_uses_nodes_for_paths_and_documents_for_content() -> Result<(), String> {
    let Some(pool) = test_pool().await? else {
        return Ok(());
    };
    crate::MIGRATOR
        .run(&pool)
        .await
        .map_err(|error| error.to_string())?;
    assert_index_exists(&pool, "nodes_path_trgm_idx").await?;

    let user_id = create_test_user(&pool).await?;
    let repo = FilesRepo::new(pool.clone());
    let service = FilesService::new(repo);

    let read_before_root = service
        .find(
            user_id,
            FindRequest {
                q: "notegate".into(),
                path: None,
                kind: None,
                limit: Some(50),
            },
        )
        .await;
    assert!(matches!(read_before_root, Err(FilesError::NotFound(_))));

    let workspace_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM workspaces
        WHERE owner_user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(workspace_count, 0);

    let root = service.root(user_id).await.map_err(debug_error)?;
    assert_eq!(root.path, "/");
    assert_eq!(root.kind, NodeKind::Folder);

    let projects = service
        .create_folder(
            user_id,
            CreateFolder {
                parent_node_id: root.id,
                name: "projects".into(),
            },
        )
        .await
        .map_err(debug_error)?;
    assert_eq!(projects.path, "/projects");

    let document = service
        .create_document(
            user_id,
            CreateDocument {
                parent_node_id: projects.id,
                name: "notegate.md".into(),
            },
        )
        .await
        .map_err(debug_error)?;
    assert_eq!(document.node.path, "/projects/notegate.md");
    assert_eq!(document.document.content_md, "");

    let create_under_document = service
        .create_folder(
            user_id,
            CreateFolder {
                parent_node_id: document.node.id,
                name: "invalid-child".into(),
            },
        )
        .await;
    assert!(matches!(
        create_under_document,
        Err(FilesError::InvalidInput(_))
    ));

    let nested = service
        .create_folder(
            user_id,
            CreateFolder {
                parent_node_id: projects.id,
                name: "nested".into(),
            },
        )
        .await
        .map_err(debug_error)?;
    let child = service
        .create_document(
            user_id,
            CreateDocument {
                parent_node_id: nested.id,
                name: "child.md".into(),
            },
        )
        .await
        .map_err(debug_error)?;
    assert_eq!(child.node.path, "/projects/nested/child.md");

    let saved = service
        .save_document(
            user_id,
            SaveDocument {
                node_id: document.node.id,
                content_md: "# notegate\nfile tree markdown memo\n".into(),
            },
        )
        .await
        .map_err(debug_error)?;
    assert_eq!(
        saved.document.content_md,
        "# notegate\nfile tree markdown memo\n"
    );

    let grep_before = service
        .grep(
            user_id,
            GrepRequest {
                q: "file tree".into(),
                path: Some("/projects".into()),
                context: Some(1),
                limit: Some(50),
            },
        )
        .await
        .map_err(debug_error)?;
    assert_eq!(grep_before.len(), 1);
    let first_grep_before = grep_before.first().ok_or("missing grep_before result")?;
    assert_eq!(first_grep_before.path, "/projects/notegate.md");
    assert_eq!(first_grep_before.line_no, 2);

    let archive = service
        .create_folder(
            user_id,
            CreateFolder {
                parent_node_id: root.id,
                name: "archive".into(),
            },
        )
        .await
        .map_err(debug_error)?;
    let duplicate_name = service
        .create_document(
            user_id,
            CreateDocument {
                parent_node_id: archive.id,
                name: "notegate.md".into(),
            },
        )
        .await;
    assert!(matches!(duplicate_name, Err(FilesError::Conflict(_))));

    let moved = service
        .move_node(
            user_id,
            MoveNode {
                node_id: projects.id,
                new_parent_node_id: archive.id,
                new_name: Some("projects".into()),
            },
        )
        .await
        .map_err(debug_error)?;
    assert_eq!(moved.path, "/archive/projects");

    let old_path = service.resolve(user_id, "/projects/notegate.md").await;
    assert!(matches!(old_path, Err(FilesError::NotFound(_))));

    let opened = service
        .document(user_id, document.node.id)
        .await
        .map_err(debug_error)?;
    assert_eq!(opened.node.path, "/archive/projects/notegate.md");
    assert_eq!(opened.document.content_md, saved.document.content_md);

    let opened_child = service
        .document(user_id, child.node.id)
        .await
        .map_err(debug_error)?;
    assert_eq!(opened_child.node.path, "/archive/projects/nested/child.md");
    assert_paths_match_parent(&pool, user_id).await?;

    let move_into_descendant = service
        .move_node(
            user_id,
            MoveNode {
                node_id: archive.id,
                new_parent_node_id: nested.id,
                new_name: None,
            },
        )
        .await;
    assert!(matches!(move_into_descendant, Err(FilesError::Conflict(_))));

    let grep_after = service
        .grep(
            user_id,
            GrepRequest {
                q: "file tree".into(),
                path: Some("/archive/projects".into()),
                context: Some(0),
                limit: Some(50),
            },
        )
        .await
        .map_err(debug_error)?;
    assert_eq!(grep_after.len(), 1);
    let first_grep_after = grep_after.first().ok_or("missing grep_after result")?;
    assert_eq!(first_grep_after.path, "/archive/projects/notegate.md");

    let find_results = service
        .find(
            user_id,
            FindRequest {
                q: "notegate".into(),
                path: None,
                kind: Some("document".into()),
                limit: Some(50),
            },
        )
        .await
        .map_err(debug_error)?;
    assert_eq!(find_results.len(), 1);
    let first_find = find_results.first().ok_or("missing find result")?;
    assert_eq!(first_find.path, "/archive/projects/notegate.md");

    service
        .delete_node(user_id, archive.id)
        .await
        .map_err(debug_error)?;
    let deleted_doc = service.document(user_id, document.node.id).await;
    assert!(matches!(deleted_doc, Err(FilesError::NotFound(_))));
    let save_deleted = service
        .save_document(
            user_id,
            SaveDocument {
                node_id: document.node.id,
                content_md: "should not save".into(),
            },
        )
        .await;
    assert!(matches!(save_deleted, Err(FilesError::NotFound(_))));

    let grep_deleted = service
        .grep(
            user_id,
            GrepRequest {
                q: "file tree".into(),
                path: None,
                context: Some(0),
                limit: Some(50),
            },
        )
        .await
        .map_err(debug_error)?;
    assert!(grep_deleted.is_empty());

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tokio::test]
async fn concurrent_moves_keep_descendant_paths_consistent() -> Result<(), String> {
    let Some(pool) = test_pool().await? else {
        return Ok(());
    };
    crate::MIGRATOR
        .run(&pool)
        .await
        .map_err(|error| error.to_string())?;

    let user_id = create_test_user(&pool).await?;
    let service = FilesService::new(FilesRepo::new(pool.clone()));
    let root = service.root(user_id).await.map_err(debug_error)?;
    let a = service
        .create_folder(
            user_id,
            CreateFolder {
                parent_node_id: root.id,
                name: format!("a-{user_id}"),
            },
        )
        .await
        .map_err(debug_error)?;
    let b = service
        .create_folder(
            user_id,
            CreateFolder {
                parent_node_id: root.id,
                name: format!("b-{user_id}"),
            },
        )
        .await
        .map_err(debug_error)?;
    let c = service
        .create_folder(
            user_id,
            CreateFolder {
                parent_node_id: root.id,
                name: format!("c-{user_id}"),
            },
        )
        .await
        .map_err(debug_error)?;
    let doc = service
        .create_document(
            user_id,
            CreateDocument {
                parent_node_id: a.id,
                name: format!("doc-{user_id}.md"),
            },
        )
        .await
        .map_err(debug_error)?;

    let service_a = service.clone();
    let service_b = service.clone();
    let (move_to_b, move_to_c) = tokio::join!(
        service_a.move_node(
            user_id,
            MoveNode {
                node_id: a.id,
                new_parent_node_id: b.id,
                new_name: None,
            },
        ),
        service_b.move_node(
            user_id,
            MoveNode {
                node_id: a.id,
                new_parent_node_id: c.id,
                new_name: None,
            },
        )
    );
    move_to_b.map_err(debug_error)?;
    move_to_c.map_err(debug_error)?;

    let opened = service
        .document(user_id, doc.node.id)
        .await
        .map_err(debug_error)?;
    assert!(
        opened
            .node
            .path
            .starts_with(&format!("/b-{user_id}/a-{user_id}/"))
            || opened
                .node
                .path
                .starts_with(&format!("/c-{user_id}/a-{user_id}/"))
    );
    assert_paths_match_parent(&pool, user_id).await?;

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

async fn test_pool() -> Result<Option<PgPool>, String> {
    let url = std::env::var("NOTEGATE_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok();
    let Some(url) = url else {
        eprintln!("skipping files db test: NOTEGATE_TEST_DATABASE_URL is not set");
        return Ok(None);
    };

    PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .map(Some)
        .map_err(|error| error.to_string())
}

async fn create_test_user(pool: &PgPool) -> Result<Uuid, String> {
    let id = Uuid::new_v4();
    let sub = format!("files-test-{id}");
    let email = format!("files-test-{id}@example.test");
    sqlx::query_scalar::<_, Uuid>(
        r#"
            INSERT INTO users (id, sub, email, display_name)
            VALUES ($1, $2, $3, 'Files Test')
            RETURNING id
            "#,
    )
    .bind(id)
    .bind(sub)
    .bind(email)
    .fetch_one(pool)
    .await
    .map_err(|error| error.to_string())
}

fn debug_error(error: FilesError) -> String {
    format!("{error:?}")
}

async fn assert_index_exists(pool: &PgPool, name: &str) -> Result<(), String> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT to_regclass($1) IS NOT NULL
        "#,
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(|error| error.to_string())?;
    assert!(exists, "missing index {name}");
    Ok(())
}

async fn assert_paths_match_parent(pool: &PgPool, user_id: Uuid) -> Result<(), String> {
    let broken_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM nodes child
        JOIN nodes parent
          ON parent.id = child.parent_id
         AND parent.workspace_id = child.workspace_id
        JOIN workspaces w
          ON w.id = child.workspace_id
        WHERE w.owner_user_id = $1
          AND child.deleted_at IS NULL
          AND parent.deleted_at IS NULL
          AND child.path_cache <> CASE
              WHEN parent.path_cache = '/' THEN '/' || child.name
              ELSE parent.path_cache || '/' || child.name
          END
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(broken_count, 0);
    Ok(())
}
