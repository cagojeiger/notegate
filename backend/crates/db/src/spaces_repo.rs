//! Space lifecycle persistence.

use std::collections::HashMap;

use crate::audit_events::{self, AuditContext};
use crate::{map_sqlx_error, object_storage_repo, space_permission, tier_lookup};
use chrono::{DateTime, Utc};
use notegate_core::tier::{TierFeatures, UserTier};
use notegate_core::{Error, Result, limits};
use notegate_model::{
    CreateSpace, Permission, Space, SpaceCursor, SpaceOrderUpdate, SpaceView, UpdateSpace,
};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SpaceRepo {
    pool: PgPool,
}

impl SpaceRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Debug, FromRow)]
struct SpaceRow {
    id: Uuid,
    name: String,
    sort_order: i32,
    navigation_pinned_at: Option<DateTime<Utc>>,
    user_mcp_enabled_at: Option<DateTime<Utc>>,
    default_search_enabled: bool,
    default_text_encryption_enabled: bool,
    owner_user_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    deleted_by_user_id: Option<Uuid>,
    purge_after: Option<DateTime<Utc>>,
}

impl From<SpaceRow> for Space {
    fn from(row: SpaceRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            sort_order: row.sort_order,
            navigation_pinned_at: row.navigation_pinned_at,
            user_mcp_enabled_at: row.user_mcp_enabled_at,
            default_search_enabled: row.default_search_enabled,
            default_text_encryption_enabled: row.default_text_encryption_enabled,
            owner_user_id: row.owner_user_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deleted_at: row.deleted_at,
            deleted_by_user_id: row.deleted_by_user_id,
            purge_after: row.purge_after,
        }
    }
}

#[derive(Debug, FromRow)]
struct SpaceViewRow {
    id: Uuid,
    name: String,
    sort_order: i32,
    navigation_pinned_at: Option<DateTime<Utc>>,
    user_mcp_enabled_at: Option<DateTime<Utc>>,
    default_search_enabled: bool,
    default_text_encryption_enabled: bool,
    owner_user_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    deleted_by_user_id: Option<Uuid>,
    purge_after: Option<DateTime<Utc>>,
    permission: String,
    root_node_id: Uuid,
    owner_tier: String,
}

impl SpaceViewRow {
    fn into_view(self) -> Result<SpaceView> {
        let permission = Permission::parse(&self.permission).ok_or_else(|| {
            Error::internal(format!("unknown space permission: {}", self.permission))
        })?;
        let owner_tier = UserTier::parse_db(&self.owner_tier)?;
        Ok(SpaceView {
            space: Space {
                id: self.id,
                name: self.name,
                sort_order: self.sort_order,
                navigation_pinned_at: self.navigation_pinned_at,
                user_mcp_enabled_at: self.user_mcp_enabled_at,
                default_search_enabled: self.default_search_enabled,
                default_text_encryption_enabled: self.default_text_encryption_enabled,
                owner_user_id: self.owner_user_id,
                created_at: self.created_at,
                updated_at: self.updated_at,
                deleted_at: self.deleted_at,
                deleted_by_user_id: self.deleted_by_user_id,
                purge_after: self.purge_after,
            },
            permission,
            root_node_id: self.root_node_id,
            features: owner_tier.features(),
        })
    }
}

const SPACE_COLUMNS: &str = "id, name, sort_order, navigation_pinned_at, user_mcp_enabled_at, default_search_enabled, default_text_encryption_enabled, owner_user_id, created_at, updated_at, deleted_at, deleted_by_user_id, purge_after";
const SPACE_VIEW_BASE_COLUMNS: &str = "s.id, s.name, s.sort_order, s.navigation_pinned_at, s.user_mcp_enabled_at, s.default_search_enabled, s.default_text_encryption_enabled, s.owner_user_id, s.created_at, s.updated_at, \
                                       s.deleted_at, s.deleted_by_user_id, s.purge_after";
const USER_SPACE_VIEW_COLUMNS: &str = "s.id, s.name, s.sort_order, s.navigation_pinned_at, s.user_mcp_enabled_at, s.default_search_enabled, s.default_text_encryption_enabled, s.owner_user_id, s.created_at, s.updated_at, \
     s.deleted_at, s.deleted_by_user_id, s.purge_after, \
     'write'::text AS permission, root.id AS root_node_id, owner.tier AS owner_tier";
const AGENT_SPACE_VIEW_COLUMNS: &str = "s.id, s.name, s.sort_order, s.navigation_pinned_at, s.user_mcp_enabled_at, s.default_search_enabled, s.default_text_encryption_enabled, s.owner_user_id, s.created_at, s.updated_at, \
     s.deleted_at, s.deleted_by_user_id, s.purge_after, \
     c.permission AS permission, root.id AS root_node_id, owner.tier AS owner_tier";

impl SpaceRepo {
    pub async fn permission_for(
        &self,
        space_id: Uuid,
        account_id: Uuid,
    ) -> Result<Option<Permission>> {
        space_permission::permission_for(&self.pool, space_id, account_id).await
    }

    pub async fn create_space(&self, owner_user_id: Uuid, command: &CreateSpace) -> Result<Space> {
        self.create_space_with_features(owner_user_id, command)
            .await
            .map(|(space, _)| space)
    }

    pub async fn create_space_with_features(
        &self,
        owner_user_id: Uuid,
        command: &CreateSpace,
    ) -> Result<(Space, TierFeatures)> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let owner_tier = tier_lookup::lock_active_user_tier(
            &mut tx,
            owner_user_id,
            "space owner user account not found",
        )
        .await?;
        let quota = owner_tier.quota();

        let owned: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM spaces WHERE owner_user_id = $1 AND deleted_at IS NULL",
        )
        .bind(owner_user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let owned = crate::to_usize(owned, "space")?;
        if owned >= quota.spaces_per_user {
            return Err(Error::conflict(format!(
                "owner already has the maximum of {} spaces for tier {}",
                quota.spaces_per_user,
                owner_tier.as_str()
            )));
        }

        let sort_order: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sort_order), 0) + 1000 \
             FROM spaces WHERE owner_user_id = $1 AND deleted_at IS NULL",
        )
        .bind(owner_user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let row = sqlx::query_as::<_, SpaceRow>(&format!(
            "INSERT INTO spaces (name, owner_user_id, sort_order, navigation_pinned_at) \
             VALUES ($1, $2, $3, now()) RETURNING {SPACE_COLUMNS}"
        ))
        .bind(&command.name)
        .bind(owner_user_id)
        .bind(sort_order)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        let audit_ctx = AuditContext::rest(owner_user_id);
        audit_events::space_created(&mut tx, audit_ctx, owner_user_id, row.id).await?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok((Space::from(row), owner_tier.features()))
    }

    pub async fn find_space(&self, space_id: Uuid) -> Result<Option<Space>> {
        let row = sqlx::query_as::<_, SpaceRow>(&format!(
            "SELECT {SPACE_COLUMNS} FROM spaces WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(space_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(Space::from))
    }

    async fn find_space_view(
        &self,
        account_id: Uuid,
        space_id: Uuid,
        user_mcp_only: bool,
    ) -> Result<Option<SpaceView>> {
        let user_mcp_predicate = if user_mcp_only {
            "AND s.user_mcp_enabled_at IS NOT NULL"
        } else {
            ""
        };
        let row = sqlx::query_as::<_, SpaceViewRow>(&format!(
            "SELECT {SPACE_VIEW_BASE_COLUMNS}, \
                    CASE WHEN acc.kind = 'user' THEN 'write'::text ELSE c.permission END AS permission, \
                    root.id AS root_node_id, owner.tier AS owner_tier \
             FROM accounts acc \
             JOIN spaces s ON s.id = $2 AND s.deleted_at IS NULL \
             JOIN users owner ON owner.id = s.owner_user_id \
             JOIN nodes root ON root.space_id = s.id AND root.parent_id IS NULL AND root.deleted_at IS NULL \
             LEFT JOIN space_agent_connections c \
               ON c.space_id = s.id AND c.agent_id = acc.id AND c.disconnected_at IS NULL \
             WHERE acc.id = $1 AND acc.is_active = true AND acc.deleted_at IS NULL \
               AND ((acc.kind = 'user' AND s.owner_user_id = acc.id {user_mcp_predicate}) \
                    OR (acc.kind = 'agent' AND c.agent_id IS NOT NULL))"
        ))
        .bind(account_id)
        .bind(space_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.map(SpaceViewRow::into_view).transpose()
    }

    pub async fn find_space_view_for(
        &self,
        account_id: Uuid,
        space_id: Uuid,
    ) -> Result<Option<SpaceView>> {
        self.find_space_view(account_id, space_id, false).await
    }

    pub async fn find_mcp_space_view_for(
        &self,
        account_id: Uuid,
        space_id: Uuid,
    ) -> Result<Option<SpaceView>> {
        self.find_space_view(account_id, space_id, true).await
    }

    async fn list_space_views_by_name(
        &self,
        account_id: Uuid,
        name: &str,
        limit: i64,
        case_insensitive: bool,
        user_mcp_only: bool,
    ) -> Result<Vec<SpaceView>> {
        let name_predicate = if case_insensitive {
            "lower(s.name) = lower($2)"
        } else {
            "s.name = $2"
        };
        let user_mcp_predicate = if user_mcp_only {
            "AND s.user_mcp_enabled_at IS NOT NULL"
        } else {
            ""
        };
        let rows = sqlx::query_as::<_, SpaceViewRow>(&format!(
            "SELECT * FROM ( \
                 SELECT {USER_SPACE_VIEW_COLUMNS} \
                 FROM accounts acc \
                 JOIN spaces s ON s.owner_user_id = acc.id AND s.deleted_at IS NULL \
                 JOIN users owner ON owner.id = s.owner_user_id \
                 JOIN nodes root ON root.space_id = s.id AND root.parent_id IS NULL AND root.deleted_at IS NULL \
                 WHERE acc.id = $1 AND acc.kind = 'user' AND acc.is_active = true AND acc.deleted_at IS NULL \
                   {user_mcp_predicate} \
                   AND {name_predicate} \
                 UNION ALL \
                 SELECT {AGENT_SPACE_VIEW_COLUMNS} \
                 FROM accounts acc \
                 JOIN space_agent_connections c ON c.agent_id = acc.id AND c.disconnected_at IS NULL \
                 JOIN spaces s ON s.id = c.space_id AND s.deleted_at IS NULL \
                 JOIN users owner ON owner.id = s.owner_user_id \
                 JOIN nodes root ON root.space_id = s.id AND root.parent_id IS NULL AND root.deleted_at IS NULL \
                 WHERE acc.id = $1 AND acc.kind = 'agent' AND acc.is_active = true AND acc.deleted_at IS NULL \
                   AND {name_predicate} \
             ) visible_spaces \
             ORDER BY sort_order, name, id LIMIT $3"
        ))
        .bind(account_id)
        .bind(name)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.into_iter().map(SpaceViewRow::into_view).collect()
    }

    pub async fn list_space_views_by_name_for(
        &self,
        account_id: Uuid,
        name: &str,
        limit: i64,
    ) -> Result<Vec<SpaceView>> {
        self.list_space_views_by_name(account_id, name, limit, false, false)
            .await
    }

    pub async fn list_space_views_by_name_case_insensitive_for(
        &self,
        account_id: Uuid,
        name: &str,
        limit: i64,
    ) -> Result<Vec<SpaceView>> {
        self.list_space_views_by_name(account_id, name, limit, true, false)
            .await
    }

    pub async fn list_mcp_space_views_by_name_for(
        &self,
        account_id: Uuid,
        name: &str,
        limit: i64,
    ) -> Result<Vec<SpaceView>> {
        self.list_space_views_by_name(account_id, name, limit, false, true)
            .await
    }

    pub async fn list_mcp_space_views_by_name_case_insensitive_for(
        &self,
        account_id: Uuid,
        name: &str,
        limit: i64,
    ) -> Result<Vec<SpaceView>> {
        self.list_space_views_by_name(account_id, name, limit, true, true)
            .await
    }

    async fn list_space_views(
        &self,
        account_id: Uuid,
        limit: i64,
        cursor: Option<&SpaceCursor>,
        user_mcp_only: bool,
    ) -> Result<Vec<SpaceView>> {
        let cursor_clause = if cursor.is_some() {
            "WHERE (sort_order, name, id) > ($2, $3, $4)"
        } else {
            ""
        };
        let user_mcp_predicate = if user_mcp_only {
            "AND s.user_mcp_enabled_at IS NOT NULL"
        } else {
            ""
        };
        let sql = format!(
            "SELECT * FROM ( \
                 SELECT {USER_SPACE_VIEW_COLUMNS} \
                 FROM accounts acc \
                 JOIN spaces s ON s.owner_user_id = acc.id AND s.deleted_at IS NULL \
                 JOIN users owner ON owner.id = s.owner_user_id \
                 JOIN nodes root ON root.space_id = s.id AND root.parent_id IS NULL AND root.deleted_at IS NULL \
                 WHERE acc.id = $1 AND acc.kind = 'user' AND acc.is_active = true AND acc.deleted_at IS NULL \
                   {user_mcp_predicate} \
                 UNION ALL \
                 SELECT {AGENT_SPACE_VIEW_COLUMNS} \
                 FROM accounts acc \
                 JOIN space_agent_connections c ON c.agent_id = acc.id AND c.disconnected_at IS NULL \
                 JOIN spaces s ON s.id = c.space_id AND s.deleted_at IS NULL \
                 JOIN users owner ON owner.id = s.owner_user_id \
                 JOIN nodes root ON root.space_id = s.id AND root.parent_id IS NULL AND root.deleted_at IS NULL \
                 WHERE acc.id = $1 AND acc.kind = 'agent' AND acc.is_active = true AND acc.deleted_at IS NULL \
             ) visible_spaces \
             {cursor_clause} \
             ORDER BY sort_order, name, id LIMIT {}",
            if cursor.is_some() { "$5" } else { "$2" }
        );
        let rows = match cursor {
            Some(cursor) => {
                sqlx::query_as::<_, SpaceViewRow>(&sql)
                    .bind(account_id)
                    .bind(cursor.sort_order)
                    .bind(&cursor.name)
                    .bind(cursor.id)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await
            }
            None => {
                sqlx::query_as::<_, SpaceViewRow>(&sql)
                    .bind(account_id)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await
            }
        }
        .map_err(map_sqlx_error)?;
        rows.into_iter().map(SpaceViewRow::into_view).collect()
    }

    pub async fn list_space_views_for(
        &self,
        account_id: Uuid,
        limit: i64,
        cursor: Option<&SpaceCursor>,
    ) -> Result<Vec<SpaceView>> {
        self.list_space_views(account_id, limit, cursor, false)
            .await
    }

    pub async fn list_mcp_space_views_for(
        &self,
        account_id: Uuid,
        limit: i64,
        cursor: Option<&SpaceCursor>,
    ) -> Result<Vec<SpaceView>> {
        self.list_space_views(account_id, limit, cursor, true).await
    }

    pub async fn update_space(
        &self,
        space_id: Uuid,
        owner_user_id: Uuid,
        name: Option<&str>,
        sort_order: Option<i32>,
        user_mcp_enabled: Option<bool>,
    ) -> Result<Space> {
        self.update_space_with_features(
            owner_user_id,
            &UpdateSpace {
                space_id,
                name: name.map(str::to_owned),
                sort_order,
                navigation_pinned: None,
                user_mcp_enabled,
                default_search_enabled: None,
                default_text_encryption_enabled: None,
            },
        )
        .await
        .map(|(space, _)| space)
    }

    pub async fn update_space_with_features(
        &self,
        owner_user_id: Uuid,
        command: &UpdateSpace,
    ) -> Result<(Space, TierFeatures)> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let owner_tier = tier_lookup::lock_active_user_tier(
            &mut tx,
            owner_user_id,
            "space owner user account not found",
        )
        .await?;
        let current = sqlx::query_as::<_, SpaceRow>(&format!(
            "SELECT {SPACE_COLUMNS} FROM spaces \
             WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL \
             FOR UPDATE"
        ))
        .bind(command.space_id)
        .bind(owner_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .ok_or_else(|| Error::not_found("space not found"))?;

        if command.default_text_encryption_enabled == Some(true)
            && !owner_tier.features().text_encryption
        {
            return Err(Error::conflict(
                "text encryption is not available for the space owner's tier",
            ));
        }

        let name_changed = command
            .name
            .as_deref()
            .is_some_and(|value| value != current.name);
        let sort_order_changed = command
            .sort_order
            .is_some_and(|value| value != current.sort_order);
        let navigation_pinned_changed = command
            .navigation_pinned
            .is_some_and(|value| value != current.navigation_pinned_at.is_some());
        let user_mcp_changed = command
            .user_mcp_enabled
            .is_some_and(|value| value != current.user_mcp_enabled_at.is_some());
        let default_search_changed = command
            .default_search_enabled
            .is_some_and(|value| value != current.default_search_enabled);
        let default_encryption_changed = command
            .default_text_encryption_enabled
            .is_some_and(|value| value != current.default_text_encryption_enabled);
        if !name_changed
            && !sort_order_changed
            && !navigation_pinned_changed
            && !user_mcp_changed
            && !default_search_changed
            && !default_encryption_changed
        {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok((Space::from(current), owner_tier.features()));
        }

        let row = sqlx::query_as::<_, SpaceRow>(&format!(
            "UPDATE spaces \
             SET name = COALESCE($3, name), sort_order = COALESCE($4, sort_order), \
                 navigation_pinned_at = CASE \
                     WHEN $5::boolean IS NULL THEN navigation_pinned_at \
                     WHEN $5 THEN COALESCE(navigation_pinned_at, now()) \
                     ELSE NULL \
                 END, \
                 user_mcp_enabled_at = CASE \
                     WHEN $6::boolean IS NULL THEN user_mcp_enabled_at \
                     WHEN $6 THEN COALESCE(user_mcp_enabled_at, now()) \
                     ELSE NULL \
                 END, \
                 default_search_enabled = COALESCE($7, default_search_enabled), \
                 default_text_encryption_enabled = COALESCE($8, default_text_encryption_enabled), \
                 updated_at = now() \
             WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL RETURNING {SPACE_COLUMNS}"
        ))
        .bind(command.space_id)
        .bind(owner_user_id)
        .bind(command.name.as_deref())
        .bind(command.sort_order)
        .bind(command.navigation_pinned)
        .bind(command.user_mcp_enabled)
        .bind(command.default_search_enabled)
        .bind(command.default_text_encryption_enabled)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_constraint_error)?
        .ok_or_else(|| Error::not_found("space not found"))?;

        let mut changed_fields = Vec::new();
        if name_changed {
            changed_fields.push("name");
        }
        if sort_order_changed {
            changed_fields.push("sort_order");
        }
        if navigation_pinned_changed {
            changed_fields.push("navigation_pinned");
        }
        if user_mcp_changed {
            changed_fields.push("user_mcp_enabled");
        }
        if default_search_changed {
            changed_fields.push("default_search_enabled");
        }
        if default_encryption_changed {
            changed_fields.push("default_text_encryption_enabled");
        }
        let audit_ctx = AuditContext::rest(owner_user_id);
        audit_events::space_updated(
            &mut tx,
            audit_ctx,
            owner_user_id,
            command.space_id,
            &changed_fields,
        )
        .await?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok((Space::from(row), owner_tier.features()))
    }

    pub async fn reorder_spaces(
        &self,
        owner_user_id: Uuid,
        updates: &[SpaceOrderUpdate],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let space_ids: Vec<_> = updates.iter().map(|update| update.space_id).collect();
        let current_orders: HashMap<_, _> = sqlx::query_as::<_, (Uuid, i32)>(
            "SELECT id, sort_order FROM spaces \
             WHERE owner_user_id = $1 AND deleted_at IS NULL AND id = ANY($2) \
             ORDER BY id FOR UPDATE",
        )
        .bind(owner_user_id)
        .bind(&space_ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .into_iter()
        .collect();

        if current_orders.len() != updates.len() {
            return Err(Error::not_found("space not found"));
        }

        let audit_ctx = AuditContext::rest(owner_user_id);
        for update in updates {
            if current_orders.get(&update.space_id) == Some(&update.sort_order) {
                continue;
            }
            sqlx::query(
                "UPDATE spaces SET sort_order = $3, updated_at = now() \
                 WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL",
            )
            .bind(update.space_id)
            .bind(owner_user_id)
            .bind(update.sort_order)
            .execute(&mut *tx)
            .await
            .map_err(map_constraint_error)?;
            audit_events::space_updated(
                &mut tx,
                audit_ctx,
                owner_user_id,
                update.space_id,
                &["sort_order"],
            )
            .await?;
        }

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn delete_space(
        &self,
        space_id: Uuid,
        owner_user_id: Uuid,
        deleted_by_user_id: Uuid,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let result = sqlx::query(
            "UPDATE spaces \
             SET deleted_at = now(), deleted_by_user_id = $3, \
                 purge_after = now() + make_interval(days => $4::int), updated_at = now() \
             WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL",
        )
        .bind(space_id)
        .bind(owner_user_id)
        .bind(deleted_by_user_id)
        .bind(i32::try_from(limits::DELETED_SPACE_RETENTION_DAYS).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("space not found"));
        }

        object_storage_repo::queue_space_object_deletions(&mut tx, space_id).await?;

        let audit_ctx = AuditContext::rest(deleted_by_user_id);
        audit_events::space_deleted(&mut tx, audit_ctx, owner_user_id, space_id).await?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn root_node_id(&self, space_id: Uuid) -> Result<Option<Uuid>> {
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT root.id FROM nodes root \
             JOIN spaces s ON s.id = root.space_id \
             WHERE root.space_id = $1 AND root.parent_id IS NULL \
               AND root.deleted_at IS NULL AND s.deleted_at IS NULL",
        )
        .bind(space_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(id)
    }
}

fn map_constraint_error(error: sqlx::Error) -> Error {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.is_unique_violation() {
            return Error::conflict("a space with this name already exists");
        }
        if db_error.is_check_violation() {
            return Error::validation("space name is invalid");
        }
    }
    map_sqlx_error(error)
}
