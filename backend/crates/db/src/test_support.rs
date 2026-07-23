//! Shared Postgres test harness for integration-style tests.
//!
//! Enabled only for crate tests and the `test-util` feature.

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};
use uuid::Uuid;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_extensions.sql"),
    include_str!("../migrations/0002_identity.sql"),
    include_str!("../migrations/0003_spaces.sql"),
    include_str!("../migrations/0004_nodes_content.sql"),
    include_str!("../migrations/0005_unicode_names.sql"),
    include_str!("../migrations/0006_browser_sessions.sql"),
    include_str!("../migrations/0007_expand_text_line_limit.sql"),
    include_str!("../migrations/0008_recent_nodes_index.sql"),
    include_str!("../migrations/0009_audit_events.sql"),
    include_str!("../migrations/0010_file_change_events.sql"),
    include_str!("../migrations/0011_nodes_name_sort_index.sql"),
    include_str!("../migrations/0012_space_usage.sql"),
    include_str!("../migrations/0013_split_space_usage_bytes.sql"),
    include_str!("../migrations/0014_object_storage.sql"),
    include_str!("../migrations/0015_object_only_files.sql"),
    include_str!("../migrations/0016_default_user_tier.sql"),
    include_str!("../migrations/0017_multipart_object_uploads.sql"),
    include_str!("../migrations/0018_detected_file_media_type.sql"),
];

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
        sqlx::query("SELECT pg_advisory_unlock(hashtextextended('notegate_test_extensions', 0))")
            .execute(&mut admin)
            .await?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
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

        for migration in MIGRATIONS {
            let schema_migration = migration
                .lines()
                .filter(|line| !line.trim_start().starts_with("CREATE EXTENSION"))
                .collect::<Vec<_>>()
                .join("\n");
            if !schema_migration.trim().is_empty() {
                sqlx::raw_sql(&schema_migration).execute(&pool).await?;
            }
        }
        seed_crypto_key_epochs(&pool).await?;
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

    for migration in crate::MIGRATOR.iter() {
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
