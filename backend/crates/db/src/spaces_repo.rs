//! Space lifecycle persistence.

use crate::{map_sqlx_error, space_permission, tier_lookup};
use chrono::{DateTime, Utc};
use notegate_core::{Error, Result, limits};
use notegate_model::{CreateSpace, Permission, Space, SpaceCursor, SpaceView};
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
    owner_user_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    deleted_by_user_id: Option<Uuid>,
    purge_after: Option<DateTime<Utc>>,
    permission: String,
    root_node_id: Uuid,
}

impl SpaceViewRow {
    fn into_view(self) -> Result<SpaceView> {
        let permission = Permission::parse(&self.permission).ok_or_else(|| {
            Error::internal(format!("unknown space permission: {}", self.permission))
        })?;
        Ok(SpaceView {
            space: Space {
                id: self.id,
                name: self.name,
                sort_order: self.sort_order,
                owner_user_id: self.owner_user_id,
                created_at: self.created_at,
                updated_at: self.updated_at,
                deleted_at: self.deleted_at,
                deleted_by_user_id: self.deleted_by_user_id,
                purge_after: self.purge_after,
            },
            permission,
            root_node_id: self.root_node_id,
        })
    }
}

const SPACE_COLUMNS: &str = "id, name, sort_order, owner_user_id, created_at, updated_at, deleted_at, deleted_by_user_id, purge_after";
const SPACE_VIEW_BASE_COLUMNS: &str = "s.id, s.name, s.sort_order, s.owner_user_id, s.created_at, s.updated_at, \
                                       s.deleted_at, s.deleted_by_user_id, s.purge_after";
const USER_SPACE_VIEW_COLUMNS: &str = "s.id, s.name, s.sort_order, s.owner_user_id, s.created_at, s.updated_at, \
     s.deleted_at, s.deleted_by_user_id, s.purge_after, \
     'write'::text AS permission, root.id AS root_node_id";
const AGENT_SPACE_VIEW_COLUMNS: &str = "s.id, s.name, s.sort_order, s.owner_user_id, s.created_at, s.updated_at, \
     s.deleted_at, s.deleted_by_user_id, s.purge_after, \
     c.permission AS permission, root.id AS root_node_id";

impl SpaceRepo {
    pub async fn permission_for(
        &self,
        space_id: Uuid,
        account_id: Uuid,
    ) -> Result<Option<Permission>> {
        space_permission::permission_for(&self.pool, space_id, account_id).await
    }

    pub async fn create_space(&self, owner_user_id: Uuid, command: &CreateSpace) -> Result<Space> {
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
        let owned = usize::try_from(owned).map_err(|_| Error::internal("negative space count"))?;
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
            "INSERT INTO spaces (name, owner_user_id, sort_order) VALUES ($1, $2, $3) RETURNING {SPACE_COLUMNS}"
        ))
        .bind(&command.name)
        .bind(owner_user_id)
        .bind(sort_order)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(Space::from(row))
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

    pub async fn find_space_view_for(
        &self,
        account_id: Uuid,
        space_id: Uuid,
    ) -> Result<Option<SpaceView>> {
        let row = sqlx::query_as::<_, SpaceViewRow>(&format!(
            "SELECT {SPACE_VIEW_BASE_COLUMNS}, \
                    CASE WHEN acc.kind = 'user' THEN 'write'::text ELSE c.permission END AS permission, \
                    root.id AS root_node_id \
             FROM accounts acc \
             JOIN spaces s ON s.id = $2 AND s.deleted_at IS NULL \
             JOIN nodes root ON root.space_id = s.id AND root.parent_id IS NULL AND root.deleted_at IS NULL \
             LEFT JOIN space_agent_connections c \
               ON c.space_id = s.id AND c.agent_id = acc.id AND c.disconnected_at IS NULL \
             WHERE acc.id = $1 AND acc.is_active = true AND acc.deleted_at IS NULL \
               AND ((acc.kind = 'user' AND s.owner_user_id = acc.id) \
                    OR (acc.kind = 'agent' AND c.agent_id IS NOT NULL))"
        ))
        .bind(account_id)
        .bind(space_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.map(SpaceViewRow::into_view).transpose()
    }

    pub async fn list_space_views_by_name_for(
        &self,
        account_id: Uuid,
        name: &str,
        limit: i64,
    ) -> Result<Vec<SpaceView>> {
        let rows = sqlx::query_as::<_, SpaceViewRow>(&format!(
            "SELECT * FROM ( \
                 SELECT {USER_SPACE_VIEW_COLUMNS} \
                 FROM accounts acc \
                 JOIN spaces s ON s.owner_user_id = acc.id AND s.deleted_at IS NULL \
                 JOIN nodes root ON root.space_id = s.id AND root.parent_id IS NULL AND root.deleted_at IS NULL \
                 WHERE acc.id = $1 AND acc.kind = 'user' AND acc.is_active = true AND acc.deleted_at IS NULL \
                   AND s.name = $2 \
                 UNION ALL \
                 SELECT {AGENT_SPACE_VIEW_COLUMNS} \
                 FROM accounts acc \
                 JOIN space_agent_connections c ON c.agent_id = acc.id AND c.disconnected_at IS NULL \
                 JOIN spaces s ON s.id = c.space_id AND s.deleted_at IS NULL \
                 JOIN nodes root ON root.space_id = s.id AND root.parent_id IS NULL AND root.deleted_at IS NULL \
                 WHERE acc.id = $1 AND acc.kind = 'agent' AND acc.is_active = true AND acc.deleted_at IS NULL \
                   AND s.name = $2 \
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

    pub async fn list_space_views_for(
        &self,
        account_id: Uuid,
        limit: i64,
        cursor: Option<&SpaceCursor>,
    ) -> Result<Vec<SpaceView>> {
        let cursor_clause = if cursor.is_some() {
            "WHERE (sort_order, name, id) > ($2, $3, $4)"
        } else {
            ""
        };
        let sql = format!(
            "SELECT * FROM ( \
                 SELECT {USER_SPACE_VIEW_COLUMNS} \
                 FROM accounts acc \
                 JOIN spaces s ON s.owner_user_id = acc.id AND s.deleted_at IS NULL \
                 JOIN nodes root ON root.space_id = s.id AND root.parent_id IS NULL AND root.deleted_at IS NULL \
                 WHERE acc.id = $1 AND acc.kind = 'user' AND acc.is_active = true AND acc.deleted_at IS NULL \
                 UNION ALL \
                 SELECT {AGENT_SPACE_VIEW_COLUMNS} \
                 FROM accounts acc \
                 JOIN space_agent_connections c ON c.agent_id = acc.id AND c.disconnected_at IS NULL \
                 JOIN spaces s ON s.id = c.space_id AND s.deleted_at IS NULL \
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

    pub async fn update_space(
        &self,
        space_id: Uuid,
        owner_user_id: Uuid,
        name: Option<&str>,
        sort_order: Option<i32>,
    ) -> Result<Space> {
        let row = sqlx::query_as::<_, SpaceRow>(&format!(
            "UPDATE spaces \
             SET name = COALESCE($3, name), sort_order = COALESCE($4, sort_order), updated_at = now() \
             WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL RETURNING {SPACE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(owner_user_id)
        .bind(name)
        .bind(sort_order)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_constraint_error)?
        .ok_or_else(|| Error::not_found("space not found"))?;
        Ok(Space::from(row))
    }

    pub async fn delete_space(
        &self,
        space_id: Uuid,
        owner_user_id: Uuid,
        deleted_by_user_id: Uuid,
    ) -> Result<()> {
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
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("space not found"));
        }
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
