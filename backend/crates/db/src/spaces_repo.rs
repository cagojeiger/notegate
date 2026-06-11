//! Space lifecycle persistence.

use crate::{map_sqlx_error, space_permission};
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

const SPACE_COLUMNS: &str =
    "id, name, owner_user_id, created_at, updated_at, deleted_at, deleted_by_user_id, purge_after";
const SPACE_VIEW_COLUMNS: &str = "s.id, s.name, s.owner_user_id, s.created_at, s.updated_at, \
                                  s.deleted_at, s.deleted_by_user_id, s.purge_after, \
                                  CASE \
                                    WHEN acc.kind = 'user' THEN 'write' \
                                    ELSE c.permission \
                                  END AS permission, \
                                  root.id AS root_node_id";

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

        let owner_exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT u.id FROM users u \
             JOIN accounts acc ON acc.id = u.id \
             WHERE u.id = $1 AND acc.kind = 'user' AND acc.is_active = true AND acc.deleted_at IS NULL \
             FOR UPDATE OF acc",
        )
        .bind(owner_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if owner_exists.is_none() {
            return Err(Error::not_found("space owner user account not found"));
        }

        let owned: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM spaces WHERE owner_user_id = $1 AND deleted_at IS NULL",
        )
        .bind(owner_user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let owned = usize::try_from(owned).map_err(|_| Error::internal("negative space count"))?;
        if owned >= limits::OWNED_SPACES_MAX {
            return Err(Error::conflict(format!(
                "owner already has the maximum of {} spaces",
                limits::OWNED_SPACES_MAX
            )));
        }

        let row = sqlx::query_as::<_, SpaceRow>(&format!(
            "INSERT INTO spaces (name, owner_user_id) VALUES ($1, $2) RETURNING {SPACE_COLUMNS}"
        ))
        .bind(&command.name)
        .bind(owner_user_id)
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
            "SELECT {SPACE_VIEW_COLUMNS} \
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
            "SELECT {SPACE_VIEW_COLUMNS} \
             FROM accounts acc \
             JOIN spaces s ON s.name = $2 AND s.deleted_at IS NULL \
             JOIN nodes root ON root.space_id = s.id AND root.parent_id IS NULL AND root.deleted_at IS NULL \
             LEFT JOIN space_agent_connections c \
               ON c.space_id = s.id AND c.agent_id = acc.id AND c.disconnected_at IS NULL \
             WHERE acc.id = $1 AND acc.is_active = true AND acc.deleted_at IS NULL \
               AND ((acc.kind = 'user' AND s.owner_user_id = acc.id) \
                    OR (acc.kind = 'agent' AND c.agent_id IS NOT NULL)) \
             ORDER BY s.created_at, s.id LIMIT $3"
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
            "AND (s.created_at, s.id) > ($2, $3)"
        } else {
            ""
        };
        let sql = format!(
            "SELECT {SPACE_VIEW_COLUMNS} \
             FROM accounts acc \
             JOIN spaces s ON s.deleted_at IS NULL \
             JOIN nodes root ON root.space_id = s.id AND root.parent_id IS NULL AND root.deleted_at IS NULL \
             LEFT JOIN space_agent_connections c \
               ON c.space_id = s.id AND c.agent_id = acc.id AND c.disconnected_at IS NULL \
             WHERE acc.id = $1 AND acc.is_active = true AND acc.deleted_at IS NULL \
               AND ((acc.kind = 'user' AND s.owner_user_id = acc.id) \
                    OR (acc.kind = 'agent' AND c.agent_id IS NOT NULL)) \
               {cursor_clause} \
             ORDER BY s.created_at, s.id LIMIT {}",
            if cursor.is_some() { "$4" } else { "$2" }
        );
        let rows = match cursor {
            Some(cursor) => {
                sqlx::query_as::<_, SpaceViewRow>(&sql)
                    .bind(account_id)
                    .bind(cursor.created_at)
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

    pub async fn rename_space(
        &self,
        space_id: Uuid,
        owner_user_id: Uuid,
        new_name: &str,
    ) -> Result<Space> {
        let row = sqlx::query_as::<_, SpaceRow>(&format!(
            "UPDATE spaces SET name = $3, updated_at = now() \
             WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL RETURNING {SPACE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(owner_user_id)
        .bind(new_name)
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
