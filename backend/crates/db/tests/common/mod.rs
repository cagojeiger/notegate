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

use notegate_db::AccountRepo;
use notegate_model::ResolveAttrs;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};
use uuid::Uuid;

/// The single fresh schema migration, embedded at compile time.
const MIGRATION: &str = include_str!("../../migrations/0001_init.sql");

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
        // Extensions are database-global and not schema-isolated. Install them
        // once in `public` before running the per-test schema migration; applying
        // CREATE EXTENSION concurrently inside throwaway schemas races on a fresh DB.
        sqlx::query("SELECT pg_advisory_lock(hashtextextended('notegate_test_extensions', 0))")
            .execute(&mut admin)
            .await?;
        sqlx::query("CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public")
            .execute(&mut admin)
            .await?;
        sqlx::query("CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA public")
            .execute(&mut admin)
            .await?;
        sqlx::query("SELECT pg_advisory_unlock(hashtextextended('notegate_test_extensions', 0))")
            .execute(&mut admin)
            .await?;
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

        let schema_migration = MIGRATION
            .lines()
            .filter(|line| !line.trim_start().starts_with("CREATE EXTENSION"))
            .collect::<Vec<_>>()
            .join("\n");
        sqlx::raw_sql(&schema_migration).execute(&pool).await?;
        record_migration_ledger(&pool).await?;

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

async fn record_migration_ledger(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE _sqlx_migrations (
            version BIGINT PRIMARY KEY,
            description TEXT NOT NULL,
            installed_on TIMESTAMPTZ NOT NULL DEFAULT now(),
            success BOOLEAN NOT NULL,
            checksum BYTEA NOT NULL,
            execution_time BIGINT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    for migration in notegate_db::MIGRATOR.iter() {
        sqlx::query(
            "INSERT INTO _sqlx_migrations \
             (version, description, success, checksum, execution_time) \
             VALUES ($1, $2, true, $3, 0)",
        )
        .bind(migration.version)
        .bind(migration.description.to_string())
        .bind(migration.checksum.as_ref())
        .execute(pool)
        .await?;
    }

    Ok(())
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
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let (account, _) = AccountRepo::new(pool.clone())
        .upsert_user_by_sub(&ResolveAttrs {
            sub: sub.to_owned(),
            email: email.to_owned(),
            name: format!("user-{sub}"),
        })
        .await?;
    Ok(account.id)
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
