//! Trash metadata and owner mutations. Never copy document bodies into history.
use chrono::{DateTime, Utc};
use notegate_core::tier::effective_file_tree_limits;
use notegate_core::{Error, Result};
use notegate_model::trash::{TrashCursor, TrashEntryVersion, TrashItem};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::audit_events::{self, AuditContext};
use crate::files::commands::checks;
use crate::space_usage::{self, UsageDelta};
use crate::{FilesRepo, file_change_events, map_sqlx_error, tier_lookup};

#[derive(FromRow)]
struct TrashRow {
    id: Uuid,
    space_id: Uuid,
    space_name: String,
    kind: String,
    name: String,
    path: String,
    deleted_at: DateTime<Utc>,
    purge_after: DateTime<Utc>,
    deletion_operation_id: Option<Uuid>,
    recoverable: bool,
    deletion_pending: bool,
}

impl From<TrashRow> for TrashItem {
    fn from(row: TrashRow) -> Self {
        Self {
            id: row.id,
            space_id: row.space_id,
            space_name: row.space_name,
            kind: row.kind,
            name: row.name,
            path: row.path,
            deleted_at: row.deleted_at,
            purge_after: row.purge_after,
            deletion_operation_id: row.deletion_operation_id,
            recoverable: row.recoverable,
            deletion_pending: row.deletion_pending,
        }
    }
}

#[derive(FromRow)]
struct DeletedNode {
    deleted_at: DateTime<Utc>,
    name: String,
    kind: String,
    parent_id: Uuid,
    purge_after: DateTime<Utc>,
    deletion_target_node_id: Option<Uuid>,
    deletion_operation_id: Option<Uuid>,
    purge_requested_at: Option<DateTime<Utc>>,
}

impl FilesRepo {
    /// Inject time only in test builds; production uses the database clock.
    #[cfg(any(test, feature = "test-util"))]
    pub fn with_trash_time(mut self, time: DateTime<Utc>) -> Self {
        self.trash_time = Some(time);
        self
    }

    pub async fn list_trash(
        &self,
        owner: Uuid,
        limit: i64,
        cursor: Option<TrashCursor>,
    ) -> Result<Vec<TrashItem>> {
        let rows = sqlx::query_as::<_, TrashRow>(
            "WITH items AS ( \
                SELECT s.id, s.id AS space_id, s.name AS space_name, 'space'::text AS kind, \
                       s.name, s.deleted_at, s.purge_after, s.purge_requested_at, s.deletion_operation_id, \
                       s.trash_recoverable AND s.purge_requested_at IS NULL AS recoverable \
                FROM spaces s JOIN accounts a ON a.id = s.owner_user_id \
                WHERE s.owner_user_id = $1 AND s.deleted_at IS NOT NULL \
                  AND a.is_active AND a.deleted_at IS NULL \
                UNION ALL \
                SELECT n.id, n.space_id, s.name, n.kind, n.name, n.deleted_at, n.purge_after, n.purge_requested_at, n.deletion_operation_id, \
                       n.deletion_target_node_id = n.id AND p.deleted_at IS NULL AND n.purge_requested_at IS NULL AS recoverable \
                FROM nodes n JOIN spaces s ON s.id = n.space_id \
                JOIN accounts a ON a.id = s.owner_user_id \
                JOIN nodes p ON p.id = n.parent_id \
                WHERE s.owner_user_id = $1 AND s.deleted_at IS NULL \
                  AND a.is_active AND a.deleted_at IS NULL AND n.deleted_at IS NOT NULL \
                  AND (n.deletion_target_node_id = n.id OR (n.deletion_target_node_id IS NULL \
                       AND (p.deleted_at IS NULL OR p.deleted_at <> n.deleted_at))) \
             ), page AS ( \
                SELECT * FROM items WHERE $2::timestamptz IS NULL OR (deleted_at, id) < ($2, $3) \
                ORDER BY deleted_at DESC, id DESC LIMIT $4 \
             ) \
             SELECT page.id, page.space_id, page.space_name, page.kind, page.name, \
                    page.deleted_at, page.purge_after, page.deletion_operation_id, \
                    COALESCE(page.recoverable, false) \
                        AND page.purge_after > COALESCE($5, now()) AS recoverable, \
                    (page.purge_requested_at IS NOT NULL OR page.purge_after <= COALESCE($5, now()) \
                        OR COALESCE(ancestry.deletion_pending, false)) AS deletion_pending, \
                    CASE WHEN page.kind = 'space' THEN '/' ELSE ancestry.path END AS path \
             FROM page LEFT JOIN LATERAL ( \
                WITH RECURSIVE chain AS ( \
                    SELECT id, parent_id, '/' || name AS path, deleted_at, purge_after, purge_requested_at \
                    FROM nodes WHERE space_id = page.space_id AND id = page.id AND page.kind <> 'space' \
                    UNION ALL SELECT p.id, p.parent_id, \
                        CASE WHEN p.parent_id IS NULL THEN c.path ELSE '/' || p.name || c.path END, \
                        p.deleted_at, p.purge_after, p.purge_requested_at \
                    FROM nodes p JOIN chain c ON p.id = c.parent_id WHERE p.space_id = page.space_id \
                ) SELECT max(path) FILTER (WHERE parent_id IS NULL) AS path, \
                    bool_or(deleted_at IS NOT NULL AND (purge_requested_at IS NOT NULL \
                        OR purge_after <= COALESCE($5, now()))) AS deletion_pending FROM chain \
             ) ancestry ON true ORDER BY page.deleted_at DESC, page.id DESC",
        )
        .bind(owner)
        .bind(cursor.as_ref().map(|c| c.deleted_at))
        .bind(cursor.as_ref().map(|c| c.id))
        .bind(limit)
        .bind(self.trash_time)
        .fetch_all(&self.pool).await.map_err(map_sqlx_error)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn restore_trashed_node(
        &self,
        owner: Uuid,
        space_id: Uuid,
        node_id: Uuid,
        expected: TrashEntryVersion,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let gate = space_usage::acquire_mutation_gate(&mut tx, space_id).await?;
        let tier =
            tier_lookup::lock_active_user_tier(&mut tx, owner, "trash item not found").await?;
        lock_owned_space(&mut tx, owner, space_id, false).await?;
        let now = trash_now(&mut tx, self.trash_time).await?;
        let node = deleted_node(&mut tx, space_id, node_id).await?;
        require_entry_version(node.deleted_at, node.deletion_operation_id, expected)?;
        if node.deletion_target_node_id != Some(node_id)
            || node.purge_requested_at.is_some()
            || node.purge_after <= now
        {
            return Err(Error::conflict("item is no longer recoverable"));
        }
        let caps = effective_file_tree_limits(tier, self.limits);
        let parent = checks::require_child_write(&mut tx, space_id, node.parent_id).await?;
        checks::require_fanout(&mut tx, space_id, node.parent_id, caps).await?;
        checks::require_sibling_unique(&mut tx, space_id, node.parent_id, &node.name, None).await?;
        let (count, text_bytes, file_bytes, depth, bytes): (i64, i64, i64, i64, i64) = sqlx::query_as(
            "WITH RECURSIVE restored AS ( \
                SELECT id, 0::bigint AS depth, 0::bigint AS bytes FROM nodes \
                WHERE space_id = $1 AND id = $2 AND deletion_target_node_id = $2 \
                UNION ALL SELECT n.id, r.depth + 1, r.bytes + 1 + octet_length(n.name) \
                FROM nodes n JOIN restored r ON n.parent_id = r.id \
                WHERE n.space_id = $1 AND n.deletion_target_node_id = $2 \
             ) SELECT count(*), \
                COALESCE((SELECT sum(t.byte_len) FROM text_objects t JOIN restored r ON r.id = t.node_id), 0)::bigint, \
                COALESCE((SELECT sum(f.byte_len) FROM file_objects f JOIN restored r ON r.id = f.node_id), 0)::bigint, \
                max(depth), max(bytes) FROM restored",
        ).bind(space_id).bind(node_id).fetch_one(&mut *tx).await.map_err(map_sqlx_error)?;
        checks::require_path_limits(checks::destination_bounds(
            parent,
            &node.name,
            checks::PathBounds {
                depth: crate::to_usize(depth, "depth")?,
                bytes: crate::to_usize(bytes, "path length")?,
            },
        )?)?;
        require_restore_fanout(&mut tx, space_id, Some(node_id), caps.folder_max_children).await?;
        require_attached_objects(&mut tx, space_id, Some(node_id)).await?;
        space_usage::apply_quota_delta(
            &mut tx,
            &gate,
            UsageDelta::subtree(count, text_bytes, file_bytes),
            caps,
        )
        .await?;
        sqlx::query(
            "UPDATE nodes SET deleted_at = NULL, deleted_by_account_id = NULL, purge_after = NULL, \
                 deletion_target_node_id = NULL, deletion_operation_id = NULL, updated_at = now(), updated_by_account_id = $3 \
             WHERE space_id = $1 AND deletion_target_node_id = $2",
        )
        .bind(space_id)
        .bind(node_id)
        .bind(owner)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        file_change_events::node_restored(
            &mut tx,
            file_change_events::context(owner, space_id, self.change_capture())
                .with_operation_id(Uuid::new_v4()),
            node_id,
            &node.kind,
            &node.name,
            node.parent_id,
            count,
            node.deletion_operation_id,
        )
        .await?;
        tx.commit().await.map_err(map_sqlx_error)
    }

    pub async fn restore_trashed_space(
        &self,
        owner: Uuid,
        space_id: Uuid,
        expected: TrashEntryVersion,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let _gate = space_usage::acquire_mutation_gate(&mut tx, space_id).await?;
        let tier =
            tier_lookup::lock_active_user_tier(&mut tx, owner, "trash item not found").await?;
        lock_owned_space(&mut tx, owner, space_id, true).await?;
        let now = trash_now(&mut tx, self.trash_time).await?;
        let (recoverable, deleted_at, deletion_operation_id): (bool, DateTime<Utc>, Option<Uuid>) =
            sqlx::query_as(
                "SELECT trash_recoverable AND purge_requested_at IS NULL AND purge_after > $2, \
                deleted_at, deletion_operation_id FROM spaces WHERE id = $1",
            )
            .bind(space_id)
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        require_entry_version(deleted_at, deletion_operation_id, expected)?;
        if !recoverable {
            return Err(Error::conflict("space is no longer recoverable"));
        }
        let owned: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM spaces WHERE owner_user_id = $1 AND deleted_at IS NULL",
        )
        .bind(owner)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if crate::to_usize(owned, "space")? >= tier.quota().spaces_per_user {
            return Err(Error::conflict("space limit would be exceeded"));
        }
        let collision: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM spaces live JOIN spaces trash ON live.name = trash.name \
             WHERE trash.id = $1 AND live.owner_user_id = $2 AND live.deleted_at IS NULL)",
        )
        .bind(space_id)
        .bind(owner)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if collision {
            return Err(Error::conflict("a space with this name already exists"));
        }
        let caps = effective_file_tree_limits(tier, self.limits);
        let (nodes, text, files): (i64, i64, i64) = sqlx::query_as(
            "SELECT live_node_count, live_text_bytes, live_file_bytes FROM space_usage WHERE space_id = $1 FOR UPDATE",
        ).bind(space_id).fetch_one(&mut *tx).await.map_err(map_sqlx_error)?;
        if crate::to_usize(nodes, "node")? > caps.space_max_nodes
            || crate::to_usize(text, "text bytes")? > caps.space_max_text_bytes
            || crate::to_usize(files, "file bytes")? > caps.space_max_file_bytes
        {
            return Err(Error::conflict(
                "restored space would exceed its current tier limits",
            ));
        }
        require_restore_fanout(&mut tx, space_id, None, caps.folder_max_children).await?;
        require_attached_objects(&mut tx, space_id, None).await?;
        // Restoration must not silently reopen external agent access.
        sqlx::query(
            "UPDATE space_agent_connections SET disconnected_at = now(), disconnected_by_user_id = $2 \
             WHERE space_id = $1 AND disconnected_at IS NULL",
        ).bind(space_id).bind(owner).execute(&mut *tx).await.map_err(map_sqlx_error)?;
        sqlx::query(
            "UPDATE spaces SET deleted_at = NULL, deleted_by_user_id = NULL, purge_after = NULL, \
                 trash_recoverable = false, deletion_operation_id = NULL, updated_at = now() WHERE id = $1",
        )
        .bind(space_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        crate::link_graph_work_repo::schedule_space_rebuild_in(&mut tx, space_id).await?;
        audit_events::space_restored(
            &mut tx,
            AuditContext::rest(owner).with_operation_id(Uuid::new_v4()),
            owner,
            space_id,
            deletion_operation_id,
        )
        .await?;
        tx.commit().await.map_err(map_sqlx_error)
    }

    pub async fn request_trash_purge(
        &self,
        owner: Uuid,
        space_id: Uuid,
        node_id: Option<Uuid>,
        expected: TrashEntryVersion,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let _gate = space_usage::acquire_mutation_gate(&mut tx, space_id).await?;
        tier_lookup::lock_active_user_tier(&mut tx, owner, "trash item not found").await?;
        lock_owned_space(&mut tx, owner, space_id, node_id.is_none()).await?;
        let now = trash_now(&mut tx, self.trash_time).await?;
        let deletion_operation_id = if let Some(node_id) = node_id {
            let node = deleted_node(&mut tx, space_id, node_id).await?;
            require_entry_version(node.deleted_at, node.deletion_operation_id, expected)?;
            if node
                .deletion_target_node_id
                .is_some_and(|target| target != node_id)
            {
                return Err(Error::conflict(
                    "permanent deletion must target the trash entry",
                ));
            }
            // The Reconciler follows due ancestors. Record only the target's
            // irreversible intent; retained descendants are not bounded by live quotas.
            sqlx::query(
                "UPDATE nodes SET purge_after = LEAST(purge_after, $3), purge_requested_at = COALESCE(purge_requested_at, $3) \
                 WHERE space_id = $1 AND id = $2",
            ).bind(space_id).bind(node_id).bind(now).execute(&mut *tx).await.map_err(map_sqlx_error)?;
            node.deletion_operation_id
        } else {
            let (deleted_at, operation_id): (DateTime<Utc>, Option<Uuid>) = sqlx::query_as(
                "SELECT deleted_at, deletion_operation_id FROM spaces WHERE id = $1",
            )
            .bind(space_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
            require_entry_version(deleted_at, operation_id, expected)?;
            sqlx::query_scalar("UPDATE spaces SET purge_after = LEAST(purge_after, $2), purge_requested_at = COALESCE(purge_requested_at, $2) WHERE id = $1 RETURNING deletion_operation_id")
                .bind(space_id)
                .bind(now)
                .fetch_one(&mut *tx)
                .await
                .map_err(map_sqlx_error)?
        };
        audit_events::trash_purge_requested(
            &mut tx,
            AuditContext::rest(owner).with_operation_id(Uuid::new_v4()),
            owner,
            space_id,
            node_id,
            deletion_operation_id,
        )
        .await?;
        tx.commit().await.map_err(map_sqlx_error)
    }
}

fn require_entry_version(
    deleted_at: DateTime<Utc>,
    operation_id: Option<Uuid>,
    expected: TrashEntryVersion,
) -> Result<()> {
    if deleted_at != expected.deleted_at || operation_id != expected.deletion_operation_id {
        return Err(Error::conflict(
            "trash entry has changed; refresh before trying again",
        ));
    }
    Ok(())
}

async fn trash_now(
    tx: &mut Transaction<'_, Postgres>,
    override_time: Option<DateTime<Utc>>,
) -> Result<DateTime<Utc>> {
    sqlx::query_scalar("SELECT COALESCE($1::timestamptz, clock_timestamp())")
        .bind(override_time)
        .fetch_one(&mut **tx)
        .await
        .map_err(map_sqlx_error)
}

async fn lock_owned_space(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    space: Uuid,
    deleted: bool,
) -> Result<()> {
    let found: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM spaces WHERE id = $1 AND owner_user_id = $2 \
         AND (deleted_at IS NOT NULL) = $3 FOR UPDATE",
    )
    .bind(space)
    .bind(owner)
    .bind(deleted)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;
    found.ok_or_else(|| Error::not_found("trash item not found"))?;
    Ok(())
}

async fn deleted_node(
    tx: &mut Transaction<'_, Postgres>,
    space: Uuid,
    node: Uuid,
) -> Result<DeletedNode> {
    sqlx::query_as(
        "SELECT deleted_at, name, kind, parent_id, purge_after, deletion_target_node_id, deletion_operation_id, purge_requested_at FROM nodes \
         WHERE space_id = $1 AND id = $2 AND parent_id IS NOT NULL AND deleted_at IS NOT NULL FOR UPDATE",
    ).bind(space).bind(node).fetch_optional(&mut **tx).await.map_err(map_sqlx_error)?
        .ok_or_else(|| Error::not_found("trash item not found"))
}

async fn require_attached_objects(
    tx: &mut Transaction<'_, Postgres>,
    space: Uuid,
    deletion_target: Option<Uuid>,
) -> Result<()> {
    let unavailable: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM file_objects f JOIN nodes n ON n.id = f.node_id \
         LEFT JOIN object_storage_objects o ON o.object_key = f.object_key \
         WHERE n.space_id = $1 AND (($2::uuid IS NULL AND n.deleted_at IS NULL) OR n.deletion_target_node_id = $2) \
           AND (o.id IS NULL OR o.state <> 'attached'))",
    ).bind(space).bind(deletion_target).fetch_one(&mut **tx).await.map_err(map_sqlx_error)?;
    if unavailable {
        return Err(Error::conflict("file content is no longer recoverable"));
    }
    Ok(())
}

async fn require_restore_fanout(
    tx: &mut Transaction<'_, Postgres>,
    space: Uuid,
    deletion_target: Option<Uuid>,
    max_children: usize,
) -> Result<()> {
    let cap =
        i64::try_from(max_children).map_err(|_| Error::internal("folder limit exceeds bigint"))?;
    let exceeded: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM nodes WHERE space_id = $1 AND parent_id IS NOT NULL \
         AND (($2::uuid IS NULL AND deleted_at IS NULL) OR deletion_target_node_id = $2) \
         GROUP BY parent_id HAVING count(*) > $3)",
    )
    .bind(space)
    .bind(deletion_target)
    .bind(cap)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;
    if exceeded {
        return Err(Error::conflict(
            "restored folder would exceed its current child limit",
        ));
    }
    Ok(())
}
