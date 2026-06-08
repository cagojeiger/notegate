//! Env-gated Postgres test harness with per-test schema isolation.
//!
//! Each test creates a unique schema, sets `search_path` to it, applies the
//! migration via `include_str!`, and drops the schema on cleanup. Tests are
//! skipped (returning `Ok(None)`) when `NOTEGATE_TEST_DATABASE_URL` is unset, so
//! the suite is a no-op without a database.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};
use uuid::Uuid;

/// The single fresh schema migration, embedded at compile time.
const MIGRATION: &str = include_str!("../../../db/migrations/0001_init.sql");

/// A throwaway schema-isolated database for one test.
pub struct TestDb {
    database_url: String,
    schema: String,
    pub pool: PgPool,
}

impl TestDb {
    /// Set up an isolated schema, or return `None` when the env var is unset.
    pub async fn setup() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let database_url = match std::env::var("NOTEGATE_TEST_DATABASE_URL") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => {
                eprintln!("skipping Postgres tests; set NOTEGATE_TEST_DATABASE_URL to run them");
                return Ok(None);
            }
        };
        let schema = format!("notegate_test_{}", Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&database_url).await?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&mut admin)
            .await?;
        admin.close().await?;

        // Put the unique test schema first so tables are created in it, but keep
        // `public` on the path so shared extensions (pg_trgm's gin_trgm_ops) and
        // pgcrypto's gen_random_uuid resolve.
        let search_path = format!("{schema},public");
        let options = PgConnectOptions::from_str(&database_url)?
            .options([("search_path", search_path.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;

        sqlx::raw_sql(MIGRATION).execute(&pool).await?;

        Ok(Some(Self {
            database_url,
            schema,
            pool,
        }))
    }

    /// Drop the isolated schema and close the pool.
    pub async fn cleanup(self) {
        self.pool.close().await;
        let mut admin = match PgConnection::connect(&self.database_url).await {
            Ok(conn) => conn,
            Err(err) => {
                eprintln!(
                    "failed to connect for schema cleanup {}: {err}",
                    self.schema
                );
                return;
            }
        };
        if let Err(err) = sqlx::query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema))
            .execute(&mut admin)
            .await
        {
            eprintln!("failed to drop temporary schema {}: {err}", self.schema);
        }
        let _ = admin.close().await;
    }
}

/// Insert a `kind='user'` account + matching `users` row, returning the id.
///
/// Shared by the agent tests; `account_repo` builds its users through the repo,
/// so the lint fires only for that binary.
#[allow(dead_code)]
pub async fn insert_user_account(
    pool: &PgPool,
    sub: &str,
    email: &str,
) -> Result<Uuid, sqlx::Error> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO accounts (kind, display_name) VALUES ('user', $1) RETURNING id",
    )
    .bind(format!("user-{sub}"))
    .fetch_one(pool)
    .await?;
    sqlx::query("INSERT INTO users (id, sub, email) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(sub)
        .bind(email)
        .execute(pool)
        .await?;
    Ok(id)
}

/// Deactivate an account as a soft-delete, matching the production account lifecycle.
#[allow(dead_code)]
pub async fn deactivate_account(
    pool: &PgPool,
    account_id: Uuid,
    deleted_by: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE accounts \
         SET is_active = false, deleted_at = now(), deleted_by = $2, updated_at = now() \
         WHERE id = $1",
    )
    .bind(account_id)
    .bind(deleted_by)
    .execute(pool)
    .await?;
    Ok(())
}
