use super::*;
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

    let read_before_root = repo
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
    assert!(matches!(read_before_root, Err(FilesRepoError::NotFound(_))));

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

    let root = repo.root(user_id).await.map_err(debug_error)?;
    assert_eq!(root.path, "/");
    assert_eq!(root.kind, NodeKind::Folder);

    let projects = repo
        .create_folder(user_id, root.id, "projects")
        .await
        .map_err(debug_error)?;
    assert_eq!(projects.path, "/projects");

    let document = repo
        .create_document(user_id, projects.id, "notegate.md")
        .await
        .map_err(debug_error)?;
    assert_eq!(document.node.path, "/projects/notegate.md");
    assert_eq!(document.document.content_md, "");

    let saved = repo
        .save_document(
            user_id,
            document.node.id,
            "# notegate\nfile tree markdown memo\n",
        )
        .await
        .map_err(debug_error)?;
    assert_eq!(
        saved.document.content_md,
        "# notegate\nfile tree markdown memo\n"
    );

    let grep_before = repo
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

    let archive = repo
        .create_folder(user_id, root.id, "archive")
        .await
        .map_err(debug_error)?;
    let duplicate_name = repo
        .create_document(user_id, archive.id, "notegate.md")
        .await;
    assert!(matches!(duplicate_name, Err(FilesRepoError::Conflict(_))));

    let moved = repo
        .move_node(user_id, document.node.id, archive.id, Some("notegate.md"))
        .await
        .map_err(debug_error)?;
    assert_eq!(moved.path, "/archive/notegate.md");

    let old_path = repo.resolve(user_id, "/projects/notegate.md").await;
    assert!(matches!(old_path, Err(FilesRepoError::NotFound(_))));

    let opened = repo
        .document(user_id, document.node.id)
        .await
        .map_err(debug_error)?;
    assert_eq!(opened.node.path, "/archive/notegate.md");
    assert_eq!(opened.document.content_md, saved.document.content_md);

    let grep_after = repo
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

    let find_results = repo
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

    repo.delete_node(user_id, archive.id)
        .await
        .map_err(debug_error)?;
    let deleted_doc = repo.document(user_id, document.node.id).await;
    assert!(matches!(deleted_doc, Err(FilesRepoError::NotFound(_))));

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

fn debug_error(error: FilesRepoError) -> String {
    format!("{error:?}")
}
