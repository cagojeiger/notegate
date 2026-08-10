//! Shared Postgres test harness for integration-style tests.
//!
//! Enabled only for crate tests and the `test-util` feature.

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};
use uuid::Uuid;

/// A throwaway schema-isolated database for one test.
pub struct TestDb {
    database_url: String,
    schema: String,
    pub pool: PgPool,
}

impl TestDb {
    /// Set up an isolated schema, or return `None` when the env var is unset.
    pub async fn setup() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        Self::setup_before_migration(None).await
    }

    /// Set up an isolated schema immediately before one migration.
    pub async fn setup_before(
        migration_version: i64,
    ) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        Self::setup_before_migration(Some(migration_version)).await
    }

    async fn setup_before_migration(
        migration_version: Option<i64>,
    ) -> Result<Option<Self>, Box<dyn std::error::Error>> {
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
        sqlx::query("SELECT pg_advisory_unlock(hashtextextended('notegate_test_extensions', 0))")
            .execute(&mut admin)
            .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&mut admin)
            .await?;
        admin.close().await?;

        // Put the unique test schema first so tables are created in it, but keep
        // `public` on the path so pgcrypto's gen_random_uuid resolves.
        let search_path = format!("{schema},public");
        let options = PgConnectOptions::from_str(&database_url)?
            .options([("search_path", search_path.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;

        for migration in crate::MIGRATOR
            .iter()
            .filter(|migration| migration_version.is_none_or(|version| migration.version < version))
        {
            apply_migration_sql(&pool, migration).await?;
        }
        seed_crypto_key_epochs(&pool).await?;
        record_migration_ledger(&pool, migration_version).await?;

        Ok(Some(Self {
            database_url,
            schema,
            pool,
        }))
    }

    /// Apply one embedded migration to a partially migrated test schema.
    pub async fn apply_migration(
        &self,
        migration_version: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let migration = crate::MIGRATOR
            .iter()
            .find(|migration| migration.version == migration_version)
            .ok_or_else(|| {
                std::io::Error::other(format!("migration {migration_version} does not exist"))
            })?;
        apply_migration_sql(&self.pool, migration).await?;
        record_migration(&self.pool, migration).await?;
        Ok(())
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
        if let Err(err) = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            self.schema
        )))
        .execute(&mut admin)
        .await
        {
            eprintln!("failed to drop temporary schema {}: {err}", self.schema);
        }
        let _ = admin.close().await;
    }
}

async fn apply_migration_sql(
    pool: &PgPool,
    migration: &sqlx::migrate::Migration,
) -> Result<(), sqlx::Error> {
    let schema_migration = migration
        .sql
        .as_str()
        .lines()
        .filter(|line| !line.trim_start().starts_with("CREATE EXTENSION"))
        .collect::<Vec<_>>()
        .join("\n");
    if !schema_migration.trim().is_empty() {
        sqlx::raw_sql(sqlx::AssertSqlSafe(schema_migration))
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn seed_crypto_key_epochs(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO crypto_key_epochs \
         (key_id, domain, status, verify_tag, version, activated_at) \
         VALUES \
         ('test-enc', 'enc', 'active', 'test-enc-verify-tag', 1, now()), \
         ('test-lookup', 'lookup', 'active', 'test-lookup-verify-tag', 1, now())",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn record_migration_ledger(
    pool: &PgPool,
    before_version: Option<i64>,
) -> Result<(), sqlx::Error> {
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

    for migration in crate::MIGRATOR
        .iter()
        .filter(|migration| before_version.is_none_or(|version| migration.version < version))
    {
        record_migration(pool, migration).await?;
    }

    Ok(())
}

async fn record_migration(
    pool: &PgPool,
    migration: &sqlx::migrate::Migration,
) -> Result<(), sqlx::Error> {
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
    Ok(())
}
