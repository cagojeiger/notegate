use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct VaultRepo {
    pool: PgPool,
}

impl VaultRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn root(&self, user_id: Uuid) -> VaultResult<Node> {
        let workspace_id = self.ensure_default_workspace(user_id).await?;
        self.root_for_workspace(workspace_id).await
    }

    pub async fn resolve(&self, user_id: Uuid, path: &str) -> VaultResult<Node> {
        let workspace_id = self.ensure_default_workspace(user_id).await?;
        let path = normalize_path(path)?;
        self.node_by_path(workspace_id, &path).await
    }

    pub async fn children(&self, user_id: Uuid, node_id: Uuid) -> VaultResult<Children> {
        let workspace_id = self.ensure_default_workspace(user_id).await?;
        let parent = self.node_by_id(workspace_id, node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(VaultRepoError::InvalidInput("node is not a folder".into()));
        }

        let children = sqlx::query_as::<_, NodeRow>(
            r#"
            SELECT
                n.id,
                n.parent_id,
                n.name,
                n.kind,
                n.path_cache,
                n.sort_order,
                EXISTS (
                    SELECT 1
                    FROM nodes c
                    WHERE c.workspace_id = n.workspace_id
                      AND c.parent_id = n.id
                      AND c.deleted_at IS NULL
                ) AS has_children,
                n.created_at,
                n.updated_at
            FROM nodes n
            WHERE n.workspace_id = $1
              AND n.parent_id = $2
              AND n.deleted_at IS NULL
            ORDER BY n.sort_order, n.name
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .into_iter()
        .map(NodeRow::into_node)
        .collect();

        Ok(Children { parent, children })
    }

    pub async fn create_folder(
        &self,
        user_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
    ) -> VaultResult<Node> {
        validate_folder_name(name)?;
        let workspace_id = self.ensure_default_workspace(user_id).await?;
        let parent = self.node_by_id(workspace_id, parent_node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(VaultRepoError::InvalidInput(
                "parent is not a folder".into(),
            ));
        }

        let path = child_path(&parent.path, name);
        let row = sqlx::query_as::<_, NodeRow>(
            r#"
            INSERT INTO nodes (workspace_id, parent_id, name, kind, path_cache)
            VALUES ($1, $2, $3, 'folder', $4)
            RETURNING
                id,
                parent_id,
                name,
                kind,
                path_cache,
                sort_order,
                false AS has_children,
                created_at,
                updated_at
            "#,
        )
        .bind(workspace_id)
        .bind(parent_node_id)
        .bind(name)
        .bind(path)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(row.into_node())
    }

    pub async fn create_document(
        &self,
        user_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
    ) -> VaultResult<DocumentBundle> {
        validate_document_name(name)?;
        let workspace_id = self.ensure_default_workspace(user_id).await?;
        let parent = self.node_by_id(workspace_id, parent_node_id).await?;
        if parent.kind != NodeKind::Folder {
            return Err(VaultRepoError::InvalidInput(
                "parent is not a folder".into(),
            ));
        }

        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let path = child_path(&parent.path, name);
        let node_row = sqlx::query_as::<_, NodeRow>(
            r#"
            INSERT INTO nodes (workspace_id, parent_id, name, kind, path_cache)
            VALUES ($1, $2, $3, 'document', $4)
            RETURNING
                id,
                parent_id,
                name,
                kind,
                path_cache,
                sort_order,
                false AS has_children,
                created_at,
                updated_at
            "#,
        )
        .bind(workspace_id)
        .bind(parent_node_id)
        .bind(name)
        .bind(path)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let document_row = sqlx::query_as::<_, DocumentRow>(
            r#"
            INSERT INTO documents (node_id, workspace_id, content_md, search_text)
            VALUES ($1, $2, '', '')
            RETURNING node_id, workspace_id, content_md, search_text, created_at, updated_at
            "#,
        )
        .bind(node_row.id)
        .bind(workspace_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;

        Ok(DocumentBundle {
            node: node_row.into_node(),
            document: document_row.into_document(),
        })
    }

    pub async fn document(&self, user_id: Uuid, node_id: Uuid) -> VaultResult<DocumentBundle> {
        let workspace_id = self.ensure_default_workspace(user_id).await?;
        let row = sqlx::query_as::<_, DocumentBundleRow>(
            r#"
            SELECT
                n.id,
                n.parent_id,
                n.name,
                n.kind,
                n.path_cache,
                n.sort_order,
                false AS has_children,
                n.created_at AS node_created_at,
                n.updated_at AS node_updated_at,
                d.node_id,
                d.workspace_id,
                d.content_md,
                d.search_text,
                d.created_at AS document_created_at,
                d.updated_at AS document_updated_at
            FROM nodes n
            JOIN documents d
              ON d.node_id = n.id
             AND d.workspace_id = n.workspace_id
            WHERE n.workspace_id = $1
              AND n.id = $2
              AND n.kind = 'document'
              AND n.deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(DocumentBundleRow::into_bundle)
            .ok_or_else(|| VaultRepoError::NotFound("document not found".into()))
    }

    pub async fn save_document(
        &self,
        user_id: Uuid,
        node_id: Uuid,
        content_md: &str,
    ) -> VaultResult<DocumentBundle> {
        let workspace_id = self.ensure_default_workspace(user_id).await?;
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let exists = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM nodes
                WHERE workspace_id = $1
                  AND id = $2
                  AND kind = 'document'
                  AND deleted_at IS NULL
            )
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        if !exists {
            return Err(VaultRepoError::NotFound("document not found".into()));
        }

        sqlx::query(
            r#"
            UPDATE documents
            SET content_md = $3,
                search_text = $3,
                updated_at = now()
            WHERE workspace_id = $1
              AND node_id = $2
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .bind(content_md)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            r#"
            UPDATE nodes
            SET updated_at = now()
            WHERE workspace_id = $1
              AND id = $2
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;

        self.document(user_id, node_id).await
    }

    pub async fn move_node(
        &self,
        user_id: Uuid,
        node_id: Uuid,
        new_parent_node_id: Uuid,
        new_name: Option<&str>,
    ) -> VaultResult<Node> {
        let workspace_id = self.ensure_default_workspace(user_id).await?;
        let node = self.node_by_id(workspace_id, node_id).await?;
        if node.parent_id.is_none() {
            return Err(VaultRepoError::Conflict("root cannot be moved".into()));
        }

        let new_parent = self.node_by_id(workspace_id, new_parent_node_id).await?;
        if new_parent.kind != NodeKind::Folder {
            return Err(VaultRepoError::InvalidInput(
                "new parent is not a folder".into(),
            ));
        }

        let final_name = new_name.unwrap_or(&node.name);
        match node.kind {
            NodeKind::Folder => validate_folder_name(final_name)?,
            NodeKind::Document => validate_document_name(final_name)?,
        }

        if node.id == new_parent.id
            || new_parent.path == node.path
            || new_parent
                .path
                .starts_with(&format!("{}/", node.path.trim_end_matches('/')))
        {
            return Err(VaultRepoError::Conflict(
                "node cannot move into itself or its descendant".into(),
            ));
        }

        let old_path = node.path.clone();
        let new_path = child_path(&new_parent.path, final_name);
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        sqlx::query(
            r#"
            UPDATE nodes
            SET parent_id = $3,
                name = $4,
                updated_at = now()
            WHERE workspace_id = $1
              AND id = $2
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .bind(new_parent_node_id)
        .bind(final_name)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        update_subtree_paths(&mut tx, workspace_id, node_id, &old_path, &new_path).await?;
        tx.commit().await.map_err(map_sqlx_error)?;

        self.node_by_id(workspace_id, node_id).await
    }

    pub async fn delete_node(&self, user_id: Uuid, node_id: Uuid) -> VaultResult<()> {
        let workspace_id = self.ensure_default_workspace(user_id).await?;
        let node = self.node_by_id(workspace_id, node_id).await?;
        if node.parent_id.is_none() {
            return Err(VaultRepoError::Conflict("root cannot be deleted".into()));
        }

        sqlx::query(
            r#"
            WITH RECURSIVE descendants AS (
                SELECT id
                FROM nodes
                WHERE workspace_id = $1
                  AND id = $2

                UNION ALL

                SELECT n.id
                FROM nodes n
                JOIN descendants d
                  ON n.parent_id = d.id
                WHERE n.workspace_id = $1
                  AND n.deleted_at IS NULL
            )
            UPDATE nodes
            SET deleted_at = now(),
                updated_at = now()
            WHERE workspace_id = $1
              AND id IN (SELECT id FROM descendants)
            "#,
        )
        .bind(workspace_id)
        .bind(node_id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(())
    }

    pub async fn find(&self, user_id: Uuid, request: FindRequest) -> VaultResult<Vec<Node>> {
        let q = request.q.trim();
        if q.is_empty() {
            return Err(VaultRepoError::InvalidInput("query cannot be empty".into()));
        }
        if let Some(kind) = request.kind.as_deref() {
            if kind != "folder" && kind != "document" {
                return Err(VaultRepoError::InvalidInput("invalid node kind".into()));
            }
        }

        let workspace_id = self.ensure_default_workspace(user_id).await?;
        let limit = clamp_limit(request.limit);
        let path = request.path.as_deref().map(normalize_path).transpose()?;
        let like_q = format!("%{q}%");
        let subtree_like = path
            .as_ref()
            .map(|p| format!("{}/%", p.trim_end_matches('/')));

        let rows = sqlx::query_as::<_, NodeRow>(
            r#"
            SELECT
                n.id,
                n.parent_id,
                n.name,
                n.kind,
                n.path_cache,
                n.sort_order,
                EXISTS (
                    SELECT 1
                    FROM nodes c
                    WHERE c.workspace_id = n.workspace_id
                      AND c.parent_id = n.id
                      AND c.deleted_at IS NULL
                ) AS has_children,
                n.created_at,
                n.updated_at
            FROM nodes n
            WHERE n.workspace_id = $1
              AND n.deleted_at IS NULL
              AND n.path_cache ILIKE $2
              AND ($3::TEXT IS NULL OR n.kind = $3)
              AND (
                  $4::TEXT IS NULL
                  OR n.path_cache = $4
                  OR n.path_cache LIKE $5
              )
            ORDER BY n.path_cache
            LIMIT $6
            "#,
        )
        .bind(workspace_id)
        .bind(like_q)
        .bind(request.kind)
        .bind(path)
        .bind(subtree_like)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(rows.into_iter().map(NodeRow::into_node).collect())
    }

    pub async fn grep(&self, user_id: Uuid, request: GrepRequest) -> VaultResult<Vec<GrepMatch>> {
        let q = request.q.trim();
        if q.is_empty() {
            return Err(VaultRepoError::InvalidInput("query cannot be empty".into()));
        }

        let workspace_id = self.ensure_default_workspace(user_id).await?;
        let limit = clamp_limit(request.limit) as usize;
        let context = request.context.unwrap_or(0).clamp(0, 5) as usize;
        let path = request.path.as_deref().map(normalize_path).transpose()?;
        let subtree_like = path
            .as_ref()
            .map(|p| format!("{}/%", p.trim_end_matches('/')));
        let like_q = format!("%{q}%");

        let candidates = sqlx::query_as::<_, GrepCandidateRow>(
            r#"
            SELECT n.id AS node_id, n.path_cache, d.content_md
            FROM documents d
            JOIN nodes n
              ON n.id = d.node_id
             AND n.workspace_id = d.workspace_id
            WHERE d.workspace_id = $1
              AND n.deleted_at IS NULL
              AND d.search_text ILIKE $2
              AND (
                  $3::TEXT IS NULL
                  OR n.path_cache = $3
                  OR n.path_cache LIKE $4
              )
            ORDER BY d.updated_at DESC
            LIMIT $5
            "#,
        )
        .bind(workspace_id)
        .bind(like_q)
        .bind(path)
        .bind(subtree_like)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let needle = q.to_lowercase();
        let mut matches = Vec::new();
        for candidate in candidates {
            let lines: Vec<&str> = candidate.content_md.split('\n').collect();
            for (idx, line) in lines.iter().enumerate() {
                if !line.to_lowercase().contains(&needle) {
                    continue;
                }

                let before_start = idx.saturating_sub(context);
                let before = lines[before_start..idx]
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect();
                let after_end = (idx + 1 + context).min(lines.len());
                let after = lines[idx + 1..after_end]
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect();

                matches.push(GrepMatch {
                    node_id: candidate.node_id,
                    path: candidate.path_cache.clone(),
                    line_no: idx as i64 + 1,
                    line: (*line).to_owned(),
                    before,
                    after,
                });

                if matches.len() >= limit {
                    return Ok(matches);
                }
            }
        }

        Ok(matches)
    }

    async fn ensure_default_workspace(&self, user_id: Uuid) -> VaultResult<Uuid> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let workspace_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO workspaces (owner_user_id, name)
            VALUES ($1, 'default')
            ON CONFLICT (owner_user_id, name) DO UPDATE
                SET name = EXCLUDED.name
            RETURNING id
            "#,
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            r#"
            INSERT INTO nodes (workspace_id, parent_id, name, kind, path_cache)
            SELECT $1, NULL, '/', 'folder', '/'
            WHERE NOT EXISTS (
                SELECT 1
                FROM nodes
                WHERE workspace_id = $1
                  AND parent_id IS NULL
            )
            "#,
        )
        .bind(workspace_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(workspace_id)
    }

    async fn root_for_workspace(&self, workspace_id: Uuid) -> VaultResult<Node> {
        let row = sqlx::query_as::<_, NodeRow>(
            r#"
            SELECT
                n.id,
                n.parent_id,
                n.name,
                n.kind,
                n.path_cache,
                n.sort_order,
                EXISTS (
                    SELECT 1
                    FROM nodes c
                    WHERE c.workspace_id = n.workspace_id
                      AND c.parent_id = n.id
                      AND c.deleted_at IS NULL
                ) AS has_children,
                n.created_at,
                n.updated_at
            FROM nodes n
            WHERE n.workspace_id = $1
              AND n.parent_id IS NULL
              AND n.deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(NodeRow::into_node)
            .ok_or_else(|| VaultRepoError::NotFound("root node not found".into()))
    }

    async fn node_by_id(&self, workspace_id: Uuid, node_id: Uuid) -> VaultResult<Node> {
        let row = sqlx::query_as::<_, NodeRow>(NODE_SELECT_BY_ID)
            .bind(workspace_id)
            .bind(node_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

        row.map(NodeRow::into_node)
            .ok_or_else(|| VaultRepoError::NotFound("node not found".into()))
    }

    async fn node_by_path(&self, workspace_id: Uuid, path: &str) -> VaultResult<Node> {
        let row = sqlx::query_as::<_, NodeRow>(
            r#"
            SELECT
                n.id,
                n.parent_id,
                n.name,
                n.kind,
                n.path_cache,
                n.sort_order,
                EXISTS (
                    SELECT 1
                    FROM nodes c
                    WHERE c.workspace_id = n.workspace_id
                      AND c.parent_id = n.id
                      AND c.deleted_at IS NULL
                ) AS has_children,
                n.created_at,
                n.updated_at
            FROM nodes n
            WHERE n.workspace_id = $1
              AND n.path_cache = $2
              AND n.deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(path)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(NodeRow::into_node)
            .ok_or_else(|| VaultRepoError::NotFound("node not found".into()))
    }
}

const NODE_SELECT_BY_ID: &str = r#"
    SELECT
        n.id,
        n.parent_id,
        n.name,
        n.kind,
        n.path_cache,
        n.sort_order,
        EXISTS (
            SELECT 1
            FROM nodes c
            WHERE c.workspace_id = n.workspace_id
              AND c.parent_id = n.id
              AND c.deleted_at IS NULL
        ) AS has_children,
        n.created_at,
        n.updated_at
    FROM nodes n
    WHERE n.workspace_id = $1
      AND n.id = $2
      AND n.deleted_at IS NULL
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Folder,
    Document,
}

impl NodeKind {
    fn from_db(value: String) -> Self {
        match value.as_str() {
            "document" => Self::Document,
            _ => Self::Folder,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Document => "document",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: NodeKind,
    pub path: String,
    pub sort_order: i32,
    pub has_children: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Children {
    pub parent: Node,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub node_id: Uuid,
    pub workspace_id: Uuid,
    pub content_md: String,
    pub search_text: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DocumentBundle {
    pub node: Node,
    pub document: Document,
}

#[derive(Debug, Clone)]
pub struct FindRequest {
    pub q: String,
    pub path: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct GrepRequest {
    pub q: String,
    pub path: Option<String>,
    pub context: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct GrepMatch {
    pub node_id: Uuid,
    pub path: String,
    pub line_no: i64,
    pub line: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug)]
pub enum VaultRepoError {
    NotFound(String),
    InvalidInput(String),
    Conflict(String),
    Internal(String),
}

pub type VaultResult<T> = Result<T, VaultRepoError>;

#[derive(sqlx::FromRow)]
struct NodeRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    kind: String,
    path_cache: String,
    sort_order: i32,
    has_children: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl NodeRow {
    fn into_node(self) -> Node {
        Node {
            id: self.id,
            parent_id: self.parent_id,
            name: self.name,
            kind: NodeKind::from_db(self.kind),
            path: self.path_cache,
            sort_order: self.sort_order,
            has_children: self.has_children,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct DocumentRow {
    node_id: Uuid,
    workspace_id: Uuid,
    content_md: String,
    search_text: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl DocumentRow {
    fn into_document(self) -> Document {
        Document {
            node_id: self.node_id,
            workspace_id: self.workspace_id,
            content_md: self.content_md,
            search_text: self.search_text,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct DocumentBundleRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    kind: String,
    path_cache: String,
    sort_order: i32,
    has_children: bool,
    node_created_at: DateTime<Utc>,
    node_updated_at: DateTime<Utc>,
    node_id: Uuid,
    workspace_id: Uuid,
    content_md: String,
    search_text: String,
    document_created_at: DateTime<Utc>,
    document_updated_at: DateTime<Utc>,
}

impl DocumentBundleRow {
    fn into_bundle(self) -> DocumentBundle {
        DocumentBundle {
            node: Node {
                id: self.id,
                parent_id: self.parent_id,
                name: self.name,
                kind: NodeKind::from_db(self.kind),
                path: self.path_cache,
                sort_order: self.sort_order,
                has_children: self.has_children,
                created_at: self.node_created_at,
                updated_at: self.node_updated_at,
            },
            document: Document {
                node_id: self.node_id,
                workspace_id: self.workspace_id,
                content_md: self.content_md,
                search_text: self.search_text,
                created_at: self.document_created_at,
                updated_at: self.document_updated_at,
            },
        }
    }
}

#[derive(sqlx::FromRow)]
struct GrepCandidateRow {
    node_id: Uuid,
    path_cache: String,
    content_md: String,
}

async fn update_subtree_paths(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    moving_node_id: Uuid,
    old_prefix: &str,
    new_prefix: &str,
) -> VaultResult<()> {
    sqlx::query(
        r#"
        UPDATE nodes
        SET path_cache = $4 || substring(path_cache from length($3) + 1),
            updated_at = now()
        WHERE workspace_id = $1
          AND deleted_at IS NULL
          AND (
            id = $2
            OR path_cache LIKE $3 || '/%'
          )
        "#,
    )
    .bind(workspace_id)
    .bind(moving_node_id)
    .bind(old_prefix)
    .bind(new_prefix)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;

    Ok(())
}

fn validate_folder_name(name: &str) -> VaultResult<()> {
    validate_base_name(name)?;
    if name.ends_with(".md") {
        return Err(VaultRepoError::InvalidInput(
            "folder name cannot end with .md".into(),
        ));
    }
    Ok(())
}

fn validate_document_name(name: &str) -> VaultResult<()> {
    validate_base_name(name)?;
    if !name.ends_with(".md") {
        return Err(VaultRepoError::InvalidInput(
            "document name must end with .md".into(),
        ));
    }
    Ok(())
}

fn validate_base_name(name: &str) -> VaultResult<()> {
    if name.is_empty() {
        return Err(VaultRepoError::InvalidInput("name cannot be empty".into()));
    }
    if name == "." || name == ".." {
        return Err(VaultRepoError::InvalidInput("invalid name".into()));
    }
    if name.contains('/') {
        return Err(VaultRepoError::InvalidInput("name cannot contain /".into()));
    }
    Ok(())
}

fn normalize_path(path: &str) -> VaultResult<String> {
    if !path.starts_with('/') {
        return Err(VaultRepoError::InvalidInput(
            "path must start with /".into(),
        ));
    }

    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        if segment == "." || segment == ".." {
            return Err(VaultRepoError::InvalidInput(
                "path cannot contain . or ..".into(),
            ));
        }
        segments.push(segment);
    }

    if segments.is_empty() {
        Ok("/".into())
    } else {
        Ok(format!("/{}", segments.join("/")))
    }
}

fn child_path(parent_path: &str, name: &str) -> String {
    if parent_path == "/" {
        format!("/{name}")
    } else {
        format!("{parent_path}/{name}")
    }
}

fn clamp_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(50).clamp(1, 100)
}

fn map_sqlx_error(error: sqlx::Error) -> VaultRepoError {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().as_deref() == Some("23505") {
            return VaultRepoError::Conflict("name or path already exists".into());
        }
        if db_error.code().as_deref() == Some("23514") {
            return VaultRepoError::InvalidInput("invalid vault data".into());
        }
    }

    VaultRepoError::Internal(format!("vault repository query failed: {error}"))
}
