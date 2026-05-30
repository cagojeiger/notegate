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
    assert_eq!(grep_before[0].path, "/projects/notegate.md");
    assert_eq!(grep_before[0].line_no, 2);

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
                node_id: document.node.id,
                new_parent_node_id: archive.id,
                new_name: Some("notegate.md".into()),
            },
        )
        .await
        .map_err(debug_error)?;
    assert_eq!(moved.path, "/archive/notegate.md");

    let old_path = service.resolve(user_id, "/projects/notegate.md").await;
    assert!(matches!(old_path, Err(FilesError::NotFound(_))));

    let opened = service
        .document(user_id, document.node.id)
        .await
        .map_err(debug_error)?;
    assert_eq!(opened.node.path, "/archive/notegate.md");
    assert_eq!(opened.document.content_md, saved.document.content_md);

    let grep_after = service
        .grep(
            user_id,
            GrepRequest {
                q: "file tree".into(),
                path: Some("/archive".into()),
                context: Some(0),
                limit: Some(50),
            },
        )
        .await
        .map_err(debug_error)?;
    assert_eq!(grep_after.len(), 1);
    assert_eq!(grep_after[0].path, "/archive/notegate.md");

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
    assert_eq!(find_results[0].path, "/archive/notegate.md");

    service
        .delete_node(user_id, archive.id)
        .await
        .map_err(debug_error)?;
    let deleted_doc = service.document(user_id, document.node.id).await;
    assert!(matches!(deleted_doc, Err(FilesError::NotFound(_))));

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
        .max_connections(1)
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
