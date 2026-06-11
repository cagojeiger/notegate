//! Mutating commands for the file tree.
pub mod checks {
    //! In-transaction invariant re-enforcement shared by the mutating commands.
    //!
    //! The service pre-checks these for precise errors; the DB re-checks them inside
    //! the mutation's transaction so a concurrent writer cannot slip past a count or
    //! depth bound between the pre-check and the write.

    use notegate_core::limits::Limits;
    use notegate_core::{Error, Result};
    use sqlx::PgConnection;
    use uuid::Uuid;

    use super::super::error::map_sqlx_error;
    use crate::to_usize;

    /// Serialize file-tree mutations in a space. This closes races where two
    /// transactions both observe state below a cap, or one mutation updates a node
    /// while another concurrently moves/deletes it.
    pub async fn lock_space(tx: &mut PgConnection, space_id: Uuid) -> Result<()> {
        let found: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM spaces WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(space_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if found.is_none() {
            return Err(Error::not_found("space not found"));
        }
        Ok(())
    }

    /// A live node's kind + deleted flag, fetched inside a transaction. `None` when
    /// the node does not exist in the space.
    pub struct LiveNode {
        pub kind: String,
        pub parent_id: Option<Uuid>,
    }

    /// Load a live node's kind/parent inside the transaction, or `None`.
    pub async fn live_node(
        tx: &mut PgConnection,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<LiveNode>> {
        let row: Option<(String, Option<Uuid>)> = sqlx::query_as(
            "SELECT kind, parent_id FROM nodes \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(|(kind, parent_id)| LiveNode { kind, parent_id }))
    }

    /// Assert the parent is a live folder. Returns its kind error otherwise.
    pub async fn require_live_folder(
        tx: &mut PgConnection,
        space_id: Uuid,
        parent_id: Uuid,
    ) -> Result<()> {
        match live_node(tx, space_id, parent_id).await? {
            None => Err(Error::not_found("parent node not found")),
            Some(node) if node.kind != "folder" => {
                Err(Error::validation("parent must be a folder"))
            }
            Some(_) => Ok(()),
        }
    }

    /// Depth of a node below the root (root = 0), computed in-transaction by walking
    /// the parent chain upward.
    pub async fn node_depth(tx: &mut PgConnection, space_id: Uuid, node_id: Uuid) -> Result<usize> {
        let depth: i64 = sqlx::query_scalar(
            "WITH RECURSIVE chain AS ( \
            SELECT id, parent_id, 0 AS depth \
            FROM nodes WHERE space_id = $1 AND id = $2 \
            UNION ALL \
            SELECT n.id, n.parent_id, c.depth + 1 \
            FROM nodes n JOIN chain c ON n.id = c.parent_id \
            WHERE n.space_id = $1 \
         ) \
         SELECT COALESCE(max(depth), 0)::bigint FROM chain",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        to_usize(depth, "depth")
    }

    /// Maximum depth of any live descendant relative to `node_id` (0 if none).
    pub async fn subtree_relative_depth(
        tx: &mut PgConnection,
        space_id: Uuid,
        node_id: Uuid,
    ) -> Result<usize> {
        let depth: i64 = sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
            SELECT id, 0 AS depth \
            FROM nodes WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id, s.depth + 1 \
            FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT COALESCE(max(depth), 0)::bigint FROM subtree",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        to_usize(depth, "depth")
    }

    /// Enforce the parent fanout cap (`< FOLDER_MAX_CHILDREN` live children).
    pub async fn require_fanout(
        tx: &mut PgConnection,
        space_id: Uuid,
        parent_id: Uuid,
        caps: Limits,
    ) -> Result<()> {
        let children: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM nodes \
         WHERE space_id = $1 AND parent_id = $2 AND deleted_at IS NULL",
        )
        .bind(space_id)
        .bind(parent_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if to_usize(children, "child")? >= caps.folder_max_children {
            return Err(Error::conflict(format!(
                "folder already has the maximum of {} children",
                caps.folder_max_children
            )));
        }
        Ok(())
    }

    /// Enforce the space live-node cap.
    pub async fn require_node_budget(
        tx: &mut PgConnection,
        space_id: Uuid,
        caps: Limits,
    ) -> Result<()> {
        let nodes: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM nodes WHERE space_id = $1 AND deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if to_usize(nodes, "node")? >= caps.space_max_nodes {
            return Err(Error::conflict(format!(
                "space already has the maximum of {} nodes",
                caps.space_max_nodes
            )));
        }
        Ok(())
    }

    /// Enforce the space live-text cap.
    pub async fn require_text_budget(
        tx: &mut PgConnection,
        space_id: Uuid,
        caps: Limits,
    ) -> Result<()> {
        let docs: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM text_objects d \
         JOIN nodes n ON n.id = d.node_id AND n.space_id = d.space_id \
         WHERE d.space_id = $1 AND n.deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if to_usize(docs, "text")? >= caps.space_max_texts {
            return Err(Error::conflict(format!(
                "space already has the maximum of {} texts",
                caps.space_max_texts
            )));
        }
        Ok(())
    }

    /// Enforce the space live-file cap.
    pub async fn require_file_budget(
        tx: &mut PgConnection,
        space_id: Uuid,
        caps: Limits,
    ) -> Result<()> {
        let files: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM file_objects f \
         JOIN nodes n ON n.id = f.node_id AND n.space_id = f.space_id \
         WHERE f.space_id = $1 AND n.deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if to_usize(files, "file")? >= caps.space_max_files {
            return Err(Error::conflict(format!(
                "space already has the maximum of {} files",
                caps.space_max_files
            )));
        }
        Ok(())
    }

    /// Enforce the space total live text-byte budget for a write that
    /// replaces `previous_bytes` with `new_bytes` (use `previous_bytes = 0` on
    /// create). Errors when the resulting total would exceed the cap.
    pub async fn require_byte_budget(
        tx: &mut PgConnection,
        space_id: Uuid,
        previous_bytes: i64,
        new_bytes: i64,
        caps: Limits,
    ) -> Result<()> {
        let total: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(d.byte_len), 0)::bigint FROM text_objects d \
         JOIN nodes n ON n.id = d.node_id AND n.space_id = d.space_id \
         WHERE d.space_id = $1 AND n.deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let projected = total - previous_bytes + new_bytes;
        if projected > caps.space_max_text_bytes as i64 {
            return Err(Error::conflict(format!(
                "write would exceed the space text byte budget of {}",
                caps.space_max_text_bytes
            )));
        }
        Ok(())
    }

    /// Enforce sibling-name uniqueness among live children of `parent_id`, ignoring
    /// `ignore_id` (the node being moved, for in-place operations).
    pub async fn require_sibling_unique(
        tx: &mut PgConnection,
        space_id: Uuid,
        parent_id: Uuid,
        name: &str,
        ignore_id: Option<Uuid>,
    ) -> Result<()> {
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM nodes \
         WHERE space_id = $1 AND parent_id = $2 AND name = $3 AND deleted_at IS NULL",
        )
        .bind(space_id)
        .bind(parent_id)
        .bind(name)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        match existing {
            Some(id) if Some(id) != ignore_id => Err(Error::conflict(format!(
                "a node named '{name}' already exists in this folder"
            ))),
            _ => Ok(()),
        }
    }
}

fn stored_text_parts(
    content: &notegate_model::files::StoredContent,
) -> (&'static str, Option<&str>, Option<&serde_json::Value>) {
    match &content.body {
        notegate_model::files::WriteTextBody::Plain(content) => {
            ("plain", Some(content.as_str()), None)
        }
        notegate_model::files::WriteTextBody::Encrypted(payload) => {
            ("encrypted", None, Some(payload))
        }
    }
}

pub mod create {
    //! Create commands: `mkdir` (folder) and `touch`/`write-create` (text).
    //!
    //! Both run in one transaction that re-checks every create invariant — parent is
    //! a live folder, resulting depth ≤ 5, parent fanout < 200, space node count
    //! < 10000, sibling-name unique (texts also: text count < 5000, byte
    //! budget) — then inserts the node (and the `text_objects` row for a text) with
    //! attribution = the caller.

    use notegate_core::limits::{self, Limits};
    use notegate_core::{Error, Result};
    use notegate_model::files::{StoredContent, StoredFile};
    use notegate_model::{FileObject, Node, TextObject};
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::{map_constraint_error, map_sqlx_error};
    use super::super::rows::{FILE_COLUMNS, FileRow, NODE_COLUMNS, NodeRow, TEXT_COLUMNS, TextRow};
    use super::{checks, stored_text_parts};

    /// Insert a folder under `parent_id`, attributing it to `created_by`.
    pub async fn insert_folder(
        pool: &PgPool,
        space_id: Uuid,
        parent_id: Uuid,
        name: &str,
        created_by: Uuid,
        caps: Limits,
    ) -> Result<Node> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_space(&mut tx, space_id).await?;
        prepare_create(&mut tx, space_id, parent_id, name, caps).await?;

        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, 'folder', $4, $4) RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(parent_id)
        .bind(name)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        row.into_node()
    }

    /// Insert a text node + its `text_objects` row, attributing both to
    /// `created_by`. `content` carries the pre-computed metrics from the service.
    pub async fn insert_text(
        pool: &PgPool,
        space_id: Uuid,
        parent_id: Uuid,
        name: &str,
        content: &StoredContent,
        created_by: Uuid,
        caps: Limits,
    ) -> Result<(Node, TextObject)> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_space(&mut tx, space_id).await?;
        prepare_create(&mut tx, space_id, parent_id, name, caps).await?;
        checks::require_text_budget(&mut tx, space_id, caps).await?;
        checks::require_byte_budget(&mut tx, space_id, 0, content.byte_len, caps).await?;

        let node_row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, 'text', $4, $4) RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(parent_id)
        .bind(name)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        let (storage_format, content_text, encrypted_payload) = stored_text_parts(content);
        let doc_row = sqlx::query_as::<_, TextRow>(&format!(
            "INSERT INTO text_objects \
            (node_id, space_id, storage_format, content_text, encrypted_payload, content_sha256, byte_len, line_count, \
             created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9) RETURNING {TEXT_COLUMNS}"
        ))
        .bind(node_row.id)
        .bind(space_id)
        .bind(storage_format)
        .bind(content_text)
        .bind(encrypted_payload)
        .bind(&content.content_sha256)
        .bind(content.byte_len)
        .bind(content.line_count)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok((node_row.into_node()?, doc_row.into_text()?))
    }

    /// Insert a file node + metadata and inline bytes, attributing it to `created_by`.
    pub async fn insert_file(
        pool: &PgPool,
        space_id: Uuid,
        parent_id: Uuid,
        name: &str,
        file: &StoredFile,
        created_by: Uuid,
        caps: Limits,
    ) -> Result<(Node, FileObject)> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_space(&mut tx, space_id).await?;
        prepare_create(&mut tx, space_id, parent_id, name, caps).await?;
        checks::require_file_budget(&mut tx, space_id, caps).await?;

        let node_row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, 'file', $4, $4) RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(parent_id)
        .bind(name)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        let file_row = sqlx::query_as::<_, FileRow>(&format!(
            "INSERT INTO file_objects \
            (node_id, space_id, storage_kind, media_type, byte_len, content_sha256, original_filename, encryption_mode, encryption_metadata) \
         VALUES ($1, $2, 'inline_pg', $3, $4, $5, $6, $7, $8) RETURNING {FILE_COLUMNS}"
        ))
        .bind(node_row.id)
        .bind(space_id)
        .bind(&file.media_type)
        .bind(file.byte_len)
        .bind(&file.content_sha256)
        .bind(&file.original_filename)
        .bind(file.encryption_mode.as_str())
        .bind(&file.encryption_metadata)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        sqlx::query(
            "INSERT INTO file_inline_contents (node_id, space_id, bytes) VALUES ($1, $2, $3)",
        )
        .bind(node_row.id)
        .bind(space_id)
        .bind(&file.bytes)
        .execute(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok((node_row.into_node()?, file_row.into_file()?))
    }

    /// Shared in-tx create pre-checks: parent live folder, depth, sibling-unique,
    /// fanout, and space node budget.
    async fn prepare_create(
        tx: &mut sqlx::PgConnection,
        space_id: Uuid,
        parent_id: Uuid,
        name: &str,
        caps: Limits,
    ) -> Result<()> {
        checks::require_live_folder(tx, space_id, parent_id).await?;

        let parent_depth = checks::node_depth(tx, space_id, parent_id).await?;
        if parent_depth + 1 > limits::MAX_PATH_DEPTH {
            return Err(Error::validation(format!(
                "path depth would exceed the maximum of {}",
                limits::MAX_PATH_DEPTH
            )));
        }

        checks::require_sibling_unique(tx, space_id, parent_id, name, None).await?;
        checks::require_fanout(tx, space_id, parent_id, caps).await?;
        checks::require_node_budget(tx, space_id, caps).await?;
        Ok(())
    }
}

pub mod copy_node {
    //! Server-side copy command (`cp`).
    //!
    //! Copies a live node inside one space-serialized transaction. The new root
    //! gets a fresh id/location/attribution, while node metadata and text/file
    //! content payloads are preserved.

    use std::collections::HashMap;

    use notegate_core::limits::{self, Limits};
    use notegate_core::{Error, Result};
    use notegate_model::files::CopyCounts;
    use notegate_model::{FileStorageKind, Node};
    use serde_json::Value;
    use sqlx::{FromRow, PgConnection, PgPool};
    use uuid::Uuid;

    use super::super::error::{map_constraint_error, map_sqlx_error};
    use super::super::rows::{FILE_COLUMNS, FileRow, NODE_COLUMNS, NodeRow, TEXT_COLUMNS, TextRow};
    use super::checks;
    use crate::to_usize;

    pub struct CopyNodeArgs<'a> {
        pub pool: &'a PgPool,
        pub space_id: Uuid,
        pub source_node_id: Uuid,
        pub new_parent_id: Uuid,
        pub new_name: &'a str,
        pub recursive: bool,
        pub created_by: Uuid,
        pub caps: Limits,
    }

    pub async fn copy_node(args: CopyNodeArgs<'_>) -> Result<(Node, CopyCounts)> {
        let CopyNodeArgs {
            pool,
            space_id,
            source_node_id,
            new_parent_id,
            new_name,
            recursive,
            created_by,
            caps,
        } = args;
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;
        checks::lock_space(&mut tx, space_id).await?;

        let source = checks::live_node(&mut tx, space_id, source_node_id)
            .await?
            .ok_or_else(|| Error::not_found("source node not found"))?;
        if source.parent_id.is_none() {
            return Err(Error::conflict("cannot copy the root node"));
        }
        if source.kind == "folder" && !recursive {
            return Err(Error::conflict("folder copy requires recursive=true"));
        }

        checks::require_live_folder(&mut tx, space_id, new_parent_id).await?;
        checks::require_sibling_unique(&mut tx, space_id, new_parent_id, new_name, None).await?;
        checks::require_fanout(&mut tx, space_id, new_parent_id, caps).await?;

        let snapshot = load_subtree(&mut tx, space_id, source_node_id).await?;
        if snapshot.len() > limits::SUBTREE_DELETE_MAX_NODES {
            return Err(Error::conflict(format!(
                "subtree of {} nodes exceeds the synchronous copy limit of {}",
                snapshot.len(),
                limits::SUBTREE_DELETE_MAX_NODES
            )));
        }

        let source_depth = snapshot.iter().map(|node| node.depth).max().unwrap_or(0) as usize;
        let dest_depth = checks::node_depth(&mut tx, space_id, new_parent_id).await?;
        if dest_depth + 1 + source_depth > limits::MAX_PATH_DEPTH {
            return Err(Error::conflict(format!(
                "copy would exceed the maximum path depth of {}",
                limits::MAX_PATH_DEPTH
            )));
        }

        let counts = CopyCounts {
            nodes: snapshot.len(),
            texts: snapshot.iter().filter(|node| node.kind == "text").count(),
            files: snapshot.iter().filter(|node| node.kind == "file").count(),
        };
        let text_bytes = copied_text_bytes(&mut tx, space_id, source_node_id).await?;
        require_copy_budgets(&mut tx, space_id, counts, text_bytes, caps).await?;

        let mut id_map = HashMap::with_capacity(snapshot.len());
        let mut copied_root = None;
        for source in snapshot {
            let new_parent = if source.id == source_node_id {
                new_parent_id
            } else {
                let Some(parent_id) = source.parent_id.and_then(|id| id_map.get(&id).copied())
                else {
                    return Err(Error::internal(
                        "copy traversal produced child before parent",
                    ));
                };
                parent_id
            };
            let name = if source.id == source_node_id {
                new_name
            } else {
                &source.name
            };
            let node = insert_copied_node(&mut tx, space_id, new_parent, name, &source, created_by)
                .await?;
            id_map.insert(source.id, node.id);
            copy_content(&mut tx, space_id, source.id, &node, created_by).await?;
            if source.id == source_node_id {
                copied_root = Some(node);
            }
        }

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok((
            copied_root.ok_or_else(|| Error::internal("copy produced no root node"))?,
            counts,
        ))
    }

    #[derive(Debug, FromRow)]
    struct CopyNodeRow {
        id: Uuid,
        parent_id: Option<Uuid>,
        name: String,
        kind: String,
        sort_order: i32,
        metadata: Value,
        depth: i32,
    }

    async fn load_subtree(
        tx: &mut PgConnection,
        space_id: Uuid,
        source_node_id: Uuid,
    ) -> Result<Vec<CopyNodeRow>> {
        sqlx::query_as(
            "WITH RECURSIVE subtree AS ( \
                SELECT id, parent_id, name, kind, sort_order, metadata, 0 AS depth \
                FROM nodes WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
                UNION ALL \
                SELECT n.id, n.parent_id, n.name, n.kind, n.sort_order, n.metadata, s.depth + 1 \
                FROM nodes n JOIN subtree s ON n.parent_id = s.id \
                WHERE n.space_id = $1 AND n.deleted_at IS NULL \
             ) \
             SELECT id, parent_id, name, kind, sort_order, metadata, depth \
             FROM subtree ORDER BY depth, sort_order, name, id",
        )
        .bind(space_id)
        .bind(source_node_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx_error)
    }

    async fn copied_text_bytes(
        tx: &mut PgConnection,
        space_id: Uuid,
        source_node_id: Uuid,
    ) -> Result<i64> {
        sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
                SELECT id FROM nodes WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
                UNION ALL \
                SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
                WHERE n.space_id = $1 AND n.deleted_at IS NULL \
             ) \
             SELECT COALESCE(sum(t.byte_len), 0)::bigint \
             FROM text_objects t JOIN subtree s ON s.id = t.node_id \
             WHERE t.space_id = $1",
        )
        .bind(space_id)
        .bind(source_node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)
    }

    async fn require_copy_budgets(
        tx: &mut PgConnection,
        space_id: Uuid,
        copied: CopyCounts,
        copied_text_bytes: i64,
        caps: Limits,
    ) -> Result<()> {
        let live_nodes: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM nodes WHERE space_id = $1 AND deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if to_usize(live_nodes, "node")?.saturating_add(copied.nodes) > caps.space_max_nodes {
            return Err(Error::conflict(format!(
                "copy would exceed the space node limit of {}",
                caps.space_max_nodes
            )));
        }

        let live_texts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM text_objects t \
             JOIN nodes n ON n.id = t.node_id AND n.space_id = t.space_id \
             WHERE t.space_id = $1 AND n.deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if to_usize(live_texts, "text")?.saturating_add(copied.texts) > caps.space_max_texts {
            return Err(Error::conflict(format!(
                "copy would exceed the space text limit of {}",
                caps.space_max_texts
            )));
        }

        let live_files: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM file_objects f \
             JOIN nodes n ON n.id = f.node_id AND n.space_id = f.space_id \
             WHERE f.space_id = $1 AND n.deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if to_usize(live_files, "file")?.saturating_add(copied.files) > caps.space_max_files {
            return Err(Error::conflict(format!(
                "copy would exceed the space file limit of {}",
                caps.space_max_files
            )));
        }

        let live_text_bytes: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(t.byte_len), 0)::bigint FROM text_objects t \
             JOIN nodes n ON n.id = t.node_id AND n.space_id = t.space_id \
             WHERE t.space_id = $1 AND n.deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if live_text_bytes + copied_text_bytes > caps.space_max_text_bytes as i64 {
            return Err(Error::conflict(format!(
                "copy would exceed the space text byte budget of {}",
                caps.space_max_text_bytes
            )));
        }
        Ok(())
    }

    async fn insert_copied_node(
        tx: &mut PgConnection,
        space_id: Uuid,
        parent_id: Uuid,
        name: &str,
        source: &CopyNodeRow,
        created_by: Uuid,
    ) -> Result<Node> {
        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes \
             (space_id, parent_id, name, kind, sort_order, metadata, created_by_account_id, updated_by_account_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $7) RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(parent_id)
        .bind(name)
        .bind(&source.kind)
        .bind(source.sort_order)
        .bind(&source.metadata)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;
        row.into_node()
    }

    async fn copy_content(
        tx: &mut PgConnection,
        space_id: Uuid,
        source_node_id: Uuid,
        new_node: &Node,
        created_by: Uuid,
    ) -> Result<()> {
        match new_node.kind.as_str() {
            "folder" => Ok(()),
            "text" => copy_text(tx, space_id, source_node_id, new_node.id, created_by).await,
            "file" => copy_file(tx, space_id, source_node_id, new_node.id).await,
            _ => Err(Error::internal("unknown node kind during copy")),
        }
    }

    async fn copy_text(
        tx: &mut PgConnection,
        space_id: Uuid,
        source_node_id: Uuid,
        new_node_id: Uuid,
        created_by: Uuid,
    ) -> Result<()> {
        let source = sqlx::query_as::<_, TextRow>(&format!(
            "SELECT {TEXT_COLUMNS} FROM text_objects WHERE space_id = $1 AND node_id = $2"
        ))
        .bind(space_id)
        .bind(source_node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(&format!(
            "INSERT INTO text_objects \
             (node_id, space_id, storage_format, content_text, encrypted_payload, content_sha256, byte_len, line_count, \
              media_type, encoding, created_by_account_id, updated_by_account_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11) RETURNING {TEXT_COLUMNS}"
        ))
        .bind(new_node_id)
        .bind(space_id)
        .bind(source.storage_format)
        .bind(source.content)
        .bind(source.encrypted_payload)
        .bind(source.content_sha256)
        .bind(source.byte_len)
        .bind(source.line_count)
        .bind(source.media_type)
        .bind(source.encoding)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;
        Ok(())
    }

    async fn copy_file(
        tx: &mut PgConnection,
        space_id: Uuid,
        source_node_id: Uuid,
        new_node_id: Uuid,
    ) -> Result<()> {
        let source = sqlx::query_as::<_, FileRow>(&format!(
            "SELECT {FILE_COLUMNS} FROM file_objects WHERE space_id = $1 AND node_id = $2"
        ))
        .bind(space_id)
        .bind(source_node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let file = source.into_file()?;
        if file.storage_kind != FileStorageKind::InlinePg {
            return Err(Error::conflict(
                "copy does not support object-storage files yet",
            ));
        }
        let bytes: Vec<u8> = sqlx::query_scalar(
            "SELECT bytes FROM file_inline_contents WHERE space_id = $1 AND node_id = $2",
        )
        .bind(space_id)
        .bind(source_node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(&format!(
            "INSERT INTO file_objects \
             (node_id, space_id, storage_kind, media_type, byte_len, content_sha256, original_filename, encryption_mode, encryption_metadata) \
             VALUES ($1, $2, 'inline_pg', $3, $4, $5, $6, $7, $8) RETURNING {FILE_COLUMNS}"
        ))
        .bind(new_node_id)
        .bind(space_id)
        .bind(file.media_type)
        .bind(file.byte_len)
        .bind(file.content_sha256)
        .bind(file.original_filename)
        .bind(file.encryption_mode.as_str())
        .bind(file.encryption_metadata)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        sqlx::query(
            "INSERT INTO file_inline_contents (node_id, space_id, bytes) VALUES ($1, $2, $3)",
        )
        .bind(new_node_id)
        .bind(space_id)
        .bind(bytes)
        .execute(&mut *tx)
        .await
        .map_err(map_constraint_error)?;
        Ok(())
    }
}

pub mod delete {
    //! Soft-delete command (`rm`).
    //!
    //! Soft-deletes the node and its entire live subtree (folders are recursive) in
    //! one space-serialized transaction, setting `deleted_at`/`deleted_by`. The
    //! root is rejected before the update. The subtree size is
    //! re-checked in-tx against `subtree_delete_max_nodes`; a larger subtree is
    //! rejected so a synchronous delete never touches an unbounded number of rows.

    use chrono::{DateTime, Utc};
    use notegate_core::{Error, Result, limits};
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::map_sqlx_error;
    use super::checks;

    /// Soft-delete `node_id` and its live subtree, attributing it to `deleted_by`.
    pub async fn soft_delete_node(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
        deleted_by: Uuid,
    ) -> Result<DateTime<Utc>> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_space(&mut tx, space_id).await?;

        let node = checks::live_node(&mut tx, space_id, node_id)
            .await?
            .ok_or_else(|| Error::not_found("node not found"))?;
        if node.parent_id.is_none() {
            return Err(Error::conflict("cannot delete the root node"));
        }

        // Bound the synchronous delete by the live subtree size.
        let subtree: i64 = sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
            SELECT id FROM nodes \
            WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT count(*) FROM subtree",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let subtree =
            usize::try_from(subtree).map_err(|_error| Error::internal("negative subtree count"))?;
        if subtree > limits::SUBTREE_DELETE_MAX_NODES {
            return Err(Error::conflict(format!(
                "subtree of {subtree} nodes exceeds the synchronous delete limit of {}",
                limits::SUBTREE_DELETE_MAX_NODES
            )));
        }

        let purge_after: DateTime<Utc> =
            sqlx::query_scalar("SELECT now() + ($1::bigint * interval '1 day')")
                .bind(limits::DELETED_NODE_RETENTION_DAYS)
                .fetch_one(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;

        // Soft-delete the whole live subtree in one statement.
        sqlx::query(
            "WITH RECURSIVE subtree AS ( \
            SELECT id FROM nodes \
            WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         UPDATE nodes SET deleted_at = now(), deleted_by_account_id = $3, purge_after = $4 \
         WHERE space_id = $1 AND id IN (SELECT id FROM subtree)",
        )
        .bind(space_id)
        .bind(node_id)
        .bind(deleted_by)
        .bind(purge_after)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(purge_after)
    }
}

pub mod move_node {
    //! Move/rename command (`mv`).
    //!
    //! O(1): this UPDATEs ONLY the moved node's `parent_id`/`name` (plus
    //! attribution). Descendants are never rewritten — their paths are derived,
    //! so they follow the moved node automatically. The transaction re-checks the
    //! move invariants: destination is a live folder, the move is not into the node
    //! itself or its own subtree, sibling-name is unique at the destination, the
    //! resulting subtree depth ≤ 5, and the destination fanout < 200.

    use notegate_core::limits::{self, Limits};
    use notegate_core::{Error, Result};
    use notegate_model::Node;
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::{map_constraint_error, map_sqlx_error};
    use super::super::rows::{NODE_COLUMNS, NodeRow};
    use super::checks;

    pub struct MoveNodeArgs<'a> {
        pub pool: &'a PgPool,
        pub space_id: Uuid,
        pub node_id: Uuid,
        pub new_parent_id: Uuid,
        pub new_name: Option<&'a str>,
        pub expected_parent_id: Option<Uuid>,
        pub updated_by: Uuid,
        pub caps: Limits,
    }

    /// Move/rename `node_id` to `new_parent_id` with optional `new_name`, attributing
    /// the update to `updated_by`. Updates only the moved node's row.
    pub async fn move_node(args: MoveNodeArgs<'_>) -> Result<Node> {
        let MoveNodeArgs {
            pool,
            space_id,
            node_id,
            new_parent_id,
            new_name,
            expected_parent_id,
            updated_by,
            caps,
        } = args;
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_space(&mut tx, space_id).await?;

        // The moved node must exist and be live; the root cannot be moved.
        let moved = checks::live_node(&mut tx, space_id, node_id)
            .await?
            .ok_or_else(|| Error::not_found("node not found"))?;
        if moved.parent_id.is_none() {
            return Err(Error::conflict("cannot move the root node"));
        }
        if let Some(expected_parent_id) = expected_parent_id
            && moved.parent_id != Some(expected_parent_id)
        {
            return Err(Error::conflict(
                "expected_parent_id does not match the node's current parent; refresh and retry",
            ));
        }
        let current_name: String =
            sqlx::query_scalar("SELECT name FROM nodes WHERE space_id = $1 AND id = $2")
                .bind(space_id)
                .bind(node_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        let final_name = new_name.unwrap_or(&current_name);

        // Destination must be a live folder.
        checks::require_live_folder(&mut tx, space_id, new_parent_id).await?;

        // Cannot move into self or own descendant (recursive subtree membership).
        let into_subtree: bool = sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
            SELECT id FROM nodes \
            WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.space_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT EXISTS (SELECT 1 FROM subtree WHERE id = $3)",
        )
        .bind(space_id)
        .bind(node_id)
        .bind(new_parent_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if into_subtree {
            return Err(Error::conflict(
                "cannot move a node into itself or its descendant",
            ));
        }

        // Sibling-name unique at destination (ignoring the node itself).
        checks::require_sibling_unique(&mut tx, space_id, new_parent_id, final_name, Some(node_id))
            .await?;

        // Resulting subtree depth: dest depth + 1 (the moved node) + its subtree depth.
        let dest_depth = checks::node_depth(&mut tx, space_id, new_parent_id).await?;
        let subtree_depth = checks::subtree_relative_depth(&mut tx, space_id, node_id).await?;
        if dest_depth + 1 + subtree_depth > limits::MAX_PATH_DEPTH {
            return Err(Error::conflict(format!(
                "move would exceed the maximum path depth of {}",
                limits::MAX_PATH_DEPTH
            )));
        }

        // Destination fanout, only when actually changing parent.
        if moved.parent_id != Some(new_parent_id) {
            checks::require_fanout(&mut tx, space_id, new_parent_id, caps).await?;
        }

        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "UPDATE nodes SET parent_id = $3, name = $4, updated_by_account_id = $5, updated_at = now() \
         WHERE space_id = $1 AND id = $2 RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(node_id)
        .bind(new_parent_id)
        .bind(final_name)
        .bind(updated_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        row.into_node()
    }
}

pub mod save {
    //! Save command: replace a text's content + metrics (`write`/`patch`).
    //!
    //! Runs in one transaction: re-reads the text's current byte length, enforces
    //! the space byte budget for the replacement, updates `text_objects` content +
    //! metrics + attribution, and bumps the node's `updated_by`/`updated_at`.

    use notegate_core::limits::Limits;
    use notegate_core::{Error, Result};
    use notegate_model::files::StoredContent;
    use notegate_model::{Node, TextObject};
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::{map_constraint_error, map_sqlx_error};
    use super::super::rows::{NODE_COLUMNS, NodeRow, TEXT_COLUMNS, TextRow};
    use super::{checks, stored_text_parts};

    /// Replace a live text's content + metrics, attributing the update to
    /// `updated_by` on both the text and its node.
    pub async fn save_text_content(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
        content: &StoredContent,
        expected_sha256: Option<&str>,
        updated_by: Uuid,
        caps: Limits,
    ) -> Result<(Node, TextObject)> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_space(&mut tx, space_id).await?;

        // Current byte length/hash (for budget delta + optimistic guard); the
        // text row is locked so `expected_sha256` is compared atomically with
        // the following update.
        let previous: Option<(i64, String)> = sqlx::query_as(
            "SELECT d.byte_len::bigint, d.content_sha256 FROM text_objects d \
         JOIN nodes n ON n.id = d.node_id AND n.space_id = d.space_id \
         WHERE d.space_id = $1 AND d.node_id = $2 AND n.deleted_at IS NULL \
         FOR UPDATE OF d",
        )
        .bind(space_id)
        .bind(node_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let (previous_bytes, previous_sha256) =
            previous.ok_or_else(|| Error::not_found("text not found"))?;
        if let Some(expected) = expected_sha256
            && expected != previous_sha256
        {
            return Err(Error::conflict(
                "expected_sha256 does not match the current text; read it again",
            ));
        }

        checks::require_byte_budget(&mut tx, space_id, previous_bytes, content.byte_len, caps)
            .await?;

        let (storage_format, content_text, encrypted_payload) = stored_text_parts(content);
        let doc_row = sqlx::query_as::<_, TextRow>(&format!(
            "UPDATE text_objects \
         SET storage_format = $3, content_text = $4, encrypted_payload = $5, \
             content_sha256 = $6, byte_len = $7, line_count = $8, \
             updated_by_account_id = $9, updated_at = now() \
         WHERE space_id = $1 AND node_id = $2 RETURNING {TEXT_COLUMNS}"
        ))
        .bind(space_id)
        .bind(node_id)
        .bind(storage_format)
        .bind(content_text)
        .bind(encrypted_payload)
        .bind(&content.content_sha256)
        .bind(content.byte_len)
        .bind(content.line_count)
        .bind(updated_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        let node_row = sqlx::query_as::<_, NodeRow>(&format!(
            "UPDATE nodes SET updated_by_account_id = $3, updated_at = now() \
         WHERE space_id = $1 AND id = $2 RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(node_id)
        .bind(updated_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok((node_row.into_node()?, doc_row.into_text()?))
    }
}

pub mod update {
    //! Update-metadata command (`PATCH /nodes/{id}`): rename and/or reorder a node
    //! in place, without changing its parent.
    //!
    //! Runs in one transaction serialized by the space row: the node must exist
    //! and be live; the root cannot be renamed; a rename re-checks sibling-name
    //! uniqueness at the current parent. Only
    //! the supplied fields change (`NULL` leaves a column unchanged via `COALESCE`),
    //! plus attribution.

    use notegate_core::{Error, Result};
    use notegate_model::Node;
    use serde_json::Value;
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::{map_constraint_error, map_sqlx_error};
    use super::super::rows::{NODE_COLUMNS, NodeRow};
    use super::checks;

    /// Update `node_id`'s `name` and/or `sort_order` in place, attributing the change
    /// to `updated_by`. `None` fields are left unchanged.
    pub async fn update_node_metadata(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
        new_name: Option<&str>,
        new_sort_order: Option<i32>,
        updated_by: Uuid,
    ) -> Result<Node> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_space(&mut tx, space_id).await?;

        let node = checks::live_node(&mut tx, space_id, node_id)
            .await?
            .ok_or_else(|| Error::not_found("node not found"))?;

        if let Some(name) = new_name {
            // The root node (no parent) cannot be renamed.
            let Some(parent_id) = node.parent_id else {
                return Err(Error::conflict("cannot rename the root node"));
            };
            checks::require_sibling_unique(&mut tx, space_id, parent_id, name, Some(node_id))
                .await?;
        }

        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "UPDATE nodes \
         SET name = COALESCE($3, name), \
             sort_order = COALESCE($4, sort_order), \
             updated_by_account_id = $5, updated_at = now() \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(node_id)
        .bind(new_name)
        .bind(new_sort_order)
        .bind(updated_by)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_constraint_error)?
        .ok_or_else(|| Error::not_found("node not found"))?;

        tx.commit().await.map_err(map_sqlx_error)?;
        row.into_node()
    }

    /// Replace `node_id`'s metadata object in place.
    pub async fn replace_node_metadata(
        pool: &PgPool,
        space_id: Uuid,
        node_id: Uuid,
        metadata: &Value,
        updated_by: Uuid,
    ) -> Result<Node> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_space(&mut tx, space_id).await?;
        checks::live_node(&mut tx, space_id, node_id)
            .await?
            .ok_or_else(|| Error::not_found("node not found"))?;

        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "UPDATE nodes \
         SET metadata = $3, updated_by_account_id = $4, updated_at = now() \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(node_id)
        .bind(metadata)
        .bind(updated_by)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_constraint_error)?
        .ok_or_else(|| Error::not_found("node not found"))?;

        tx.commit().await.map_err(map_sqlx_error)?;
        row.into_node()
    }
}
