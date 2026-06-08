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

    /// Serialize file-tree mutations in a workspace. This closes races where two
    /// transactions both observe state below a cap, or one mutation updates a node
    /// while another concurrently moves/deletes it.
    pub async fn lock_workspace(tx: &mut PgConnection, workspace_id: Uuid) -> Result<()> {
        let found: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM workspaces WHERE id = $1 FOR UPDATE")
                .bind(workspace_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        if found.is_none() {
            return Err(Error::not_found("workspace not found"));
        }
        Ok(())
    }

    /// A live node's kind + deleted flag, fetched inside a transaction. `None` when
    /// the node does not exist in the workspace.
    pub struct LiveNode {
        pub kind: String,
        pub parent_id: Option<Uuid>,
    }

    /// Load a live node's kind/parent inside the transaction, or `None`.
    pub async fn live_node(
        tx: &mut PgConnection,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> Result<Option<LiveNode>> {
        let row: Option<(String, Option<Uuid>)> = sqlx::query_as(
            "SELECT kind, parent_id FROM nodes \
         WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(node_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(|(kind, parent_id)| LiveNode { kind, parent_id }))
    }

    /// Assert the parent is a live folder. Returns its kind error otherwise.
    pub async fn require_live_folder(
        tx: &mut PgConnection,
        workspace_id: Uuid,
        parent_id: Uuid,
    ) -> Result<()> {
        match live_node(tx, workspace_id, parent_id).await? {
            None => Err(Error::not_found("parent node not found")),
            Some(node) if node.kind != "folder" => {
                Err(Error::validation("parent must be a folder"))
            }
            Some(_) => Ok(()),
        }
    }

    /// Depth of a node below the root (root = 0), computed in-transaction by walking
    /// the parent chain upward.
    pub async fn node_depth(
        tx: &mut PgConnection,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> Result<usize> {
        let depth: i64 = sqlx::query_scalar(
            "WITH RECURSIVE chain AS ( \
            SELECT id, parent_id, 0 AS depth \
            FROM nodes WHERE workspace_id = $1 AND id = $2 \
            UNION ALL \
            SELECT n.id, n.parent_id, c.depth + 1 \
            FROM nodes n JOIN chain c ON n.id = c.parent_id \
            WHERE n.workspace_id = $1 \
         ) \
         SELECT COALESCE(max(depth), 0)::bigint FROM chain",
        )
        .bind(workspace_id)
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        to_usize(depth, "depth")
    }

    /// Maximum depth of any live descendant relative to `node_id` (0 if none).
    pub async fn subtree_relative_depth(
        tx: &mut PgConnection,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> Result<usize> {
        let depth: i64 = sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
            SELECT id, 0 AS depth \
            FROM nodes WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id, s.depth + 1 \
            FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT COALESCE(max(depth), 0)::bigint FROM subtree",
        )
        .bind(workspace_id)
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        to_usize(depth, "depth")
    }

    /// Enforce the parent fanout cap (`< FOLDER_MAX_CHILDREN` live children).
    pub async fn require_fanout(
        tx: &mut PgConnection,
        workspace_id: Uuid,
        parent_id: Uuid,
        caps: Limits,
    ) -> Result<()> {
        let children: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM nodes \
         WHERE workspace_id = $1 AND parent_id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
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

    /// Enforce the workspace live-node cap.
    pub async fn require_node_budget(
        tx: &mut PgConnection,
        workspace_id: Uuid,
        caps: Limits,
    ) -> Result<()> {
        let nodes: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM nodes WHERE workspace_id = $1 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if to_usize(nodes, "node")? >= caps.workspace_max_nodes {
            return Err(Error::conflict(format!(
                "workspace already has the maximum of {} nodes",
                caps.workspace_max_nodes
            )));
        }
        Ok(())
    }

    /// Enforce the workspace live-document cap.
    pub async fn require_document_budget(
        tx: &mut PgConnection,
        workspace_id: Uuid,
        caps: Limits,
    ) -> Result<()> {
        let docs: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM documents d \
         JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
         WHERE d.workspace_id = $1 AND n.deleted_at IS NULL",
        )
        .bind(workspace_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if to_usize(docs, "document")? >= caps.workspace_max_documents {
            return Err(Error::conflict(format!(
                "workspace already has the maximum of {} documents",
                caps.workspace_max_documents
            )));
        }
        Ok(())
    }

    /// Enforce the workspace total live document-byte budget for a write that
    /// replaces `previous_bytes` with `new_bytes` (use `previous_bytes = 0` on
    /// create). Errors when the resulting total would exceed the cap.
    pub async fn require_byte_budget(
        tx: &mut PgConnection,
        workspace_id: Uuid,
        previous_bytes: i64,
        new_bytes: i64,
        caps: Limits,
    ) -> Result<()> {
        let total: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(d.byte_len), 0)::bigint FROM documents d \
         JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
         WHERE d.workspace_id = $1 AND n.deleted_at IS NULL",
        )
        .bind(workspace_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let projected = total - previous_bytes + new_bytes;
        if projected > caps.workspace_max_document_bytes as i64 {
            return Err(Error::conflict(format!(
                "write would exceed the workspace document byte budget of {}",
                caps.workspace_max_document_bytes
            )));
        }
        Ok(())
    }

    /// Enforce sibling-name uniqueness among live children of `parent_id`, ignoring
    /// `ignore_id` (the node being moved, for in-place operations).
    pub async fn require_sibling_unique(
        tx: &mut PgConnection,
        workspace_id: Uuid,
        parent_id: Uuid,
        name: &str,
        ignore_id: Option<Uuid>,
    ) -> Result<()> {
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM nodes \
         WHERE workspace_id = $1 AND parent_id = $2 AND name = $3 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
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

pub mod create {
    //! Create commands: `mkdir` (folder) and `touch`/`write-create` (document).
    //!
    //! Both run in one transaction that re-checks every create invariant — parent is
    //! a live folder, resulting depth ≤ 5, parent fanout < 200, workspace node count
    //! < 10000, sibling-name unique (documents also: document count < 5000, byte
    //! budget) — then inserts the node (and the `documents` row for a document) with
    //! attribution = the caller.

    use notegate_core::limits::{self, Limits};
    use notegate_core::{Error, Result};
    use notegate_model::{Document, Node};
    use notegate_service::files::StoredContent;
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::{map_constraint_error, map_sqlx_error};
    use super::super::rows::{DOCUMENT_COLUMNS, DocumentRow, NODE_COLUMNS, NodeRow};
    use super::checks;

    /// Insert a folder under `parent_id`, attributing it to `created_by`.
    pub async fn insert_folder(
        pool: &PgPool,
        workspace_id: Uuid,
        parent_id: Uuid,
        name: &str,
        created_by: Uuid,
        caps: Limits,
    ) -> Result<Node> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_workspace(&mut tx, workspace_id).await?;
        prepare_create(&mut tx, workspace_id, parent_id, name, caps).await?;

        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes (workspace_id, parent_id, name, kind, created_by, updated_by) \
         VALUES ($1, $2, $3, 'folder', $4, $4) RETURNING {NODE_COLUMNS}"
        ))
        .bind(workspace_id)
        .bind(parent_id)
        .bind(name)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        row.into_node()
    }

    /// Insert a document node + its `documents` row, attributing both to
    /// `created_by`. `content` carries the pre-computed metrics from the service.
    pub async fn insert_document(
        pool: &PgPool,
        workspace_id: Uuid,
        parent_id: Uuid,
        name: &str,
        content: &StoredContent,
        created_by: Uuid,
        caps: Limits,
    ) -> Result<(Node, Document)> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_workspace(&mut tx, workspace_id).await?;
        prepare_create(&mut tx, workspace_id, parent_id, name, caps).await?;
        checks::require_document_budget(&mut tx, workspace_id, caps).await?;
        checks::require_byte_budget(&mut tx, workspace_id, 0, i64::from(content.byte_len), caps)
            .await?;

        let node_row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes (workspace_id, parent_id, name, kind, created_by, updated_by) \
         VALUES ($1, $2, $3, 'document', $4, $4) RETURNING {NODE_COLUMNS}"
        ))
        .bind(workspace_id)
        .bind(parent_id)
        .bind(name)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        let doc_row = sqlx::query_as::<_, DocumentRow>(&format!(
            "INSERT INTO documents \
            (node_id, workspace_id, content_md, content_sha256, byte_len, line_count, \
             created_by, updated_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7) RETURNING {DOCUMENT_COLUMNS}"
        ))
        .bind(node_row.id)
        .bind(workspace_id)
        .bind(&content.content_md)
        .bind(&content.content_sha256)
        .bind(content.byte_len)
        .bind(content.line_count)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok((node_row.into_node()?, Document::from(doc_row)))
    }

    /// Shared in-tx create pre-checks: parent live folder, depth, sibling-unique,
    /// fanout, and workspace node budget.
    async fn prepare_create(
        tx: &mut sqlx::PgConnection,
        workspace_id: Uuid,
        parent_id: Uuid,
        name: &str,
        caps: Limits,
    ) -> Result<()> {
        checks::require_live_folder(tx, workspace_id, parent_id).await?;

        let parent_depth = checks::node_depth(tx, workspace_id, parent_id).await?;
        if parent_depth + 1 > limits::MAX_PATH_DEPTH {
            return Err(Error::validation(format!(
                "path depth would exceed the maximum of {}",
                limits::MAX_PATH_DEPTH
            )));
        }

        checks::require_sibling_unique(tx, workspace_id, parent_id, name, None).await?;
        checks::require_fanout(tx, workspace_id, parent_id, caps).await?;
        checks::require_node_budget(tx, workspace_id, caps).await?;
        Ok(())
    }
}

pub mod delete {
    //! Soft-delete command (`rm`).
    //!
    //! Soft-deletes the node and its entire live subtree (folders are recursive) in
    //! one workspace-serialized transaction, setting `deleted_at`/`deleted_by`. The
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
        workspace_id: Uuid,
        node_id: Uuid,
        deleted_by: Uuid,
    ) -> Result<DateTime<Utc>> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_workspace(&mut tx, workspace_id).await?;

        let node = checks::live_node(&mut tx, workspace_id, node_id)
            .await?
            .ok_or_else(|| Error::not_found("node not found"))?;
        if node.parent_id.is_none() {
            return Err(Error::conflict("cannot delete the root node"));
        }

        // Bound the synchronous delete by the live subtree size.
        let subtree: i64 = sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
            SELECT id FROM nodes \
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT count(*) FROM subtree",
        )
        .bind(workspace_id)
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
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         UPDATE nodes SET deleted_at = now(), deleted_by = $3, purge_after = $4 \
         WHERE workspace_id = $1 AND id IN (SELECT id FROM subtree)",
        )
        .bind(workspace_id)
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

    /// Move/rename `node_id` to `new_parent_id` with optional `new_name`, attributing
    /// the update to `updated_by`. Updates only the moved node's row.
    pub async fn move_node(
        pool: &PgPool,
        workspace_id: Uuid,
        node_id: Uuid,
        new_parent_id: Uuid,
        new_name: Option<&str>,
        expected_parent_id: Option<Uuid>,
        updated_by: Uuid,
        caps: Limits,
    ) -> Result<Node> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_workspace(&mut tx, workspace_id).await?;

        // The moved node must exist and be live; the root cannot be moved.
        let moved = checks::live_node(&mut tx, workspace_id, node_id)
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
            sqlx::query_scalar("SELECT name FROM nodes WHERE workspace_id = $1 AND id = $2")
                .bind(workspace_id)
                .bind(node_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        let final_name = new_name.unwrap_or(&current_name);

        // Destination must be a live folder.
        checks::require_live_folder(&mut tx, workspace_id, new_parent_id).await?;

        // Cannot move into self or own descendant (recursive subtree membership).
        let into_subtree: bool = sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
            SELECT id FROM nodes \
            WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
            WHERE n.workspace_id = $1 AND n.deleted_at IS NULL \
         ) \
         SELECT EXISTS (SELECT 1 FROM subtree WHERE id = $3)",
        )
        .bind(workspace_id)
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
        checks::require_sibling_unique(
            &mut tx,
            workspace_id,
            new_parent_id,
            final_name,
            Some(node_id),
        )
        .await?;

        // Resulting subtree depth: dest depth + 1 (the moved node) + its subtree depth.
        let dest_depth = checks::node_depth(&mut tx, workspace_id, new_parent_id).await?;
        let subtree_depth = checks::subtree_relative_depth(&mut tx, workspace_id, node_id).await?;
        if dest_depth + 1 + subtree_depth > limits::MAX_PATH_DEPTH {
            return Err(Error::conflict(format!(
                "move would exceed the maximum path depth of {}",
                limits::MAX_PATH_DEPTH
            )));
        }

        // Destination fanout, only when actually changing parent.
        if moved.parent_id != Some(new_parent_id) {
            checks::require_fanout(&mut tx, workspace_id, new_parent_id, caps).await?;
        }

        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "UPDATE nodes SET parent_id = $3, name = $4, updated_by = $5, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2 RETURNING {NODE_COLUMNS}"
        ))
        .bind(workspace_id)
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
    //! Save command: replace a document's content + metrics (`write`/`patch`).
    //!
    //! Runs in one transaction: re-reads the document's current byte length, enforces
    //! the workspace byte budget for the replacement, updates `documents` content +
    //! metrics + attribution, and bumps the node's `updated_by`/`updated_at`.

    use notegate_core::limits::Limits;
    use notegate_core::{Error, Result};
    use notegate_model::{Document, Node};
    use notegate_service::files::StoredContent;
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::{map_constraint_error, map_sqlx_error};
    use super::super::rows::{DOCUMENT_COLUMNS, DocumentRow, NODE_COLUMNS, NodeRow};
    use super::checks;

    /// Replace a live document's content + metrics, attributing the update to
    /// `updated_by` on both the document and its node.
    pub async fn save_document_content(
        pool: &PgPool,
        workspace_id: Uuid,
        node_id: Uuid,
        content: &StoredContent,
        expected_sha256: Option<&str>,
        updated_by: Uuid,
        caps: Limits,
    ) -> Result<(Node, Document)> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_workspace(&mut tx, workspace_id).await?;

        // Current byte length/hash (for budget delta + optimistic guard); the
        // document row is locked so `expected_sha256` is compared atomically with
        // the following update.
        let previous: Option<(i64, String)> = sqlx::query_as(
            "SELECT d.byte_len::bigint, d.content_sha256 FROM documents d \
         JOIN nodes n ON n.id = d.node_id AND n.workspace_id = d.workspace_id \
         WHERE d.workspace_id = $1 AND d.node_id = $2 AND n.deleted_at IS NULL \
         FOR UPDATE OF d",
        )
        .bind(workspace_id)
        .bind(node_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let (previous_bytes, previous_sha256) =
            previous.ok_or_else(|| Error::not_found("document not found"))?;
        if let Some(expected) = expected_sha256
            && expected != previous_sha256
        {
            return Err(Error::conflict(
                "expected_sha256 does not match the current document; read it again",
            ));
        }

        checks::require_byte_budget(
            &mut tx,
            workspace_id,
            previous_bytes,
            i64::from(content.byte_len),
            caps,
        )
        .await?;

        let doc_row = sqlx::query_as::<_, DocumentRow>(&format!(
            "UPDATE documents \
         SET content_md = $3, content_sha256 = $4, byte_len = $5, line_count = $6, \
             updated_by = $7, updated_at = now() \
         WHERE workspace_id = $1 AND node_id = $2 RETURNING {DOCUMENT_COLUMNS}"
        ))
        .bind(workspace_id)
        .bind(node_id)
        .bind(&content.content_md)
        .bind(&content.content_sha256)
        .bind(content.byte_len)
        .bind(content.line_count)
        .bind(updated_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        let node_row = sqlx::query_as::<_, NodeRow>(&format!(
            "UPDATE nodes SET updated_by = $3, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2 RETURNING {NODE_COLUMNS}"
        ))
        .bind(workspace_id)
        .bind(node_id)
        .bind(updated_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok((node_row.into_node()?, Document::from(doc_row)))
    }
}

pub mod update {
    //! Update-metadata command (`PATCH /nodes/{id}`): rename and/or reorder a node
    //! in place, without changing its parent.
    //!
    //! Runs in one transaction serialized by the workspace row: the node must exist
    //! and be live; the root cannot be renamed; a rename re-checks sibling-name
    //! uniqueness at the current parent. Only
    //! the supplied fields change (`NULL` leaves a column unchanged via `COALESCE`),
    //! plus attribution.

    use notegate_core::{Error, Result};
    use notegate_model::Node;
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::super::error::{map_constraint_error, map_sqlx_error};
    use super::super::rows::{NODE_COLUMNS, NodeRow};
    use super::checks;

    /// Update `node_id`'s `name` and/or `sort_order` in place, attributing the change
    /// to `updated_by`. `None` fields are left unchanged.
    pub async fn update_node_metadata(
        pool: &PgPool,
        workspace_id: Uuid,
        node_id: Uuid,
        new_name: Option<&str>,
        new_sort_order: Option<i32>,
        updated_by: Uuid,
    ) -> Result<Node> {
        let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

        checks::lock_workspace(&mut tx, workspace_id).await?;

        let node = checks::live_node(&mut tx, workspace_id, node_id)
            .await?
            .ok_or_else(|| Error::not_found("node not found"))?;

        if let Some(name) = new_name {
            // The root node (no parent) cannot be renamed.
            let Some(parent_id) = node.parent_id else {
                return Err(Error::conflict("cannot rename the root node"));
            };
            checks::require_sibling_unique(&mut tx, workspace_id, parent_id, name, Some(node_id))
                .await?;
        }

        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "UPDATE nodes \
         SET name = COALESCE($3, name), \
             sort_order = COALESCE($4, sort_order), \
             updated_by = $5, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING {NODE_COLUMNS}"
        ))
        .bind(workspace_id)
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
}
