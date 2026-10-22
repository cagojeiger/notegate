//! Shared Postgres test harness and account fixtures for integration-style tests.
//!
//! Enabled only for crate tests and the `test-util` feature.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::Instant;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};
use uuid::Uuid;

/// Missing integration configuration is an error unless a local partial run
/// explicitly opts out. CI must always execute the integration tests.
pub fn integration_test_env(name: &str) -> Result<Option<String>, std::io::Error> {
    integration_test_env_value(
        name,
        std::env::var(name).ok().as_deref(),
        std::env::var("NOTEGATE_TEST_SKIP_INTEGRATION").as_deref() == Ok("1"),
        std::env::var_os("CI").is_some(),
    )
}

fn integration_test_env_value(
    name: &str,
    value: Option<&str>,
    skip: bool,
    ci: bool,
) -> Result<Option<String>, std::io::Error> {
    if skip {
        if ci {
            return Err(std::io::Error::other(
                "NOTEGATE_TEST_SKIP_INTEGRATION is not allowed in CI",
            ));
        }
        return Ok(None);
    }
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| Some(value.to_owned()))
        .ok_or_else(|| {
            std::io::Error::other(format!(
                "{name} is required; use make test-fast for an explicit local partial run"
            ))
        })
}

/// Insert a `kind='user'` account + matching `users` row, returning the id.
pub async fn insert_user_account(
    pool: &PgPool,
    sub: &str,
    email: &str,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let (account, _) = crate::AccountRepo::new(pool.clone())
        .upsert_user_by_identity(&notegate_model::ResolveAttrs {
            issuer: "https://auth.example.test".to_owned(),
            sub: sub.to_owned(),
            email: email.to_owned(),
            name: format!("user-{sub}"),
        })
        .await?;
    Ok(account.id)
}

/// Deactivate an account as a soft-delete, matching the production account lifecycle.
pub async fn deactivate_account(
    pool: &PgPool,
    account_id: Uuid,
    deleted_by: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE accounts \
         SET is_active = false, deleted_at = now(), deleted_by_account_id = $2, updated_at = now() \
         WHERE id = $1",
    )
    .bind(account_id)
    .bind(deleted_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// A throwaway schema-isolated database for one test.
pub struct TestDb {
    database_url: String,
    schema: String,
    timing: Option<TestDbTiming>,
    pub pool: PgPool,
}

struct TestDbTiming {
    path: PathBuf,
    setup_finished: Instant,
}

impl TestDbTiming {
    fn record(&self, schema: &str, record: serde_json::Value) {
        static WRITE_LOCK: Mutex<()> = Mutex::new(());
        let write = || -> std::io::Result<()> {
            if !self.path.is_absolute() {
                return Err(std::io::Error::other(
                    "NOTEGATE_TEST_TIMING_FILE must be an absolute path",
                ));
            }
            let mut line = serde_json::to_vec(&record)?;
            line.push(b'\n');
            let _guard = WRITE_LOCK
                .lock()
                .map_err(|_| std::io::Error::other("test timing write lock is poisoned"))?;
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?
                .write_all(&line)
        };
        if let Err(err) = write() {
            let _ = writeln!(
                std::io::stderr().lock(),
                "failed to write test timing for {schema}: {err}"
            );
        }
    }
}

impl TestDb {
    /// Set up an isolated schema, or return `None` for an explicit local partial run.
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
        let Some(database_url) = integration_test_env("NOTEGATE_TEST_DATABASE_URL")? else {
            return Ok(None);
        };
        let timing_path = std::env::var_os("NOTEGATE_TEST_TIMING_FILE").map(PathBuf::from);
        let setup_started = timing_path.as_ref().map(|_| Instant::now());
        let run_id = std::env::var("NOTEGATE_TEST_RUN_ID").ok();
        let schema = schema_name(run_id.as_deref(), Uuid::new_v4())?;
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

        let migration_started = setup_started.map(|_| Instant::now());
        for migration in crate::MIGRATOR
            .iter()
            .filter(|migration| migration_version.is_none_or(|version| migration.version < version))
        {
            apply_migration_sql(&pool, migration).await?;
        }
        let migration_ms = migration_started.map(|start| start.elapsed().as_secs_f64() * 1_000.0);
        let fixture_started = setup_started.map(|_| Instant::now());
        seed_crypto_key_epochs(&pool).await?;
        record_migration_ledger(&pool, migration_version).await?;
        let timing = timing_path.zip(setup_started).map(|(path, started)| {
            let setup_finished = Instant::now();
            let timing = TestDbTiming {
                path,
                setup_finished,
            };
            timing.record(
                &schema,
                serde_json::json!({
                    "schema": schema.as_str(),
                    "phase": "setup",
                    "setup_total_ms": setup_finished.duration_since(started).as_secs_f64() * 1_000.0,
                    "migration_ms": migration_ms,
                    "fixture_ms": fixture_started.map(|start| setup_finished.duration_since(start).as_secs_f64() * 1_000.0),
                }),
            );
            timing
        });

        Ok(Some(Self {
            database_url,
            schema,
            timing,
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
        let cleanup_started = self.timing.as_ref().map(|_| Instant::now());
        self.pool.close().await;
        let mut admin = match PgConnection::connect(&self.database_url).await {
            Ok(conn) => conn,
            Err(err) => {
                eprintln!(
                    "failed to connect for schema cleanup {}: {err}",
                    self.schema
                );
                self.record_cleanup(cleanup_started, false);
                return;
            }
        };
        let result = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            self.schema
        )))
        .execute(&mut admin)
        .await;
        if let Err(err) = &result {
            eprintln!("failed to drop temporary schema {}: {err}", self.schema);
        }
        let _ = admin.close().await;
        self.record_cleanup(cleanup_started, result.is_ok());
    }

    fn record_cleanup(&self, started: Option<Instant>, schema_dropped: bool) {
        if let (Some(timing), Some(started)) = (&self.timing, started) {
            timing.record(
                &self.schema,
                serde_json::json!({
                    "schema": self.schema.as_str(),
                    "phase": "cleanup",
                    "fixture_lifetime_ms": started.duration_since(timing.setup_finished).as_secs_f64() * 1_000.0,
                    "cleanup_ms": started.elapsed().as_secs_f64() * 1_000.0,
                    "schema_dropped": schema_dropped,
                }),
            );
        }
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

async fn seed_crypto_key_epochs(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    crate::CryptoKeyEpochRepo::new(pool.clone())
        .ensure_active(&notegate_core::security::PiiCrypto::test())
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

// The integration runner owns only schemas with its validated, unique run ID.
fn schema_name(run_id: Option<&str>, id: Uuid) -> Result<String, std::io::Error> {
    match run_id {
        None => Ok(format!("notegate_test_{}", id.simple())),
        Some(run_id)
            if run_id.len() == 16
                && run_id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) =>
        {
            Ok(format!("notegate_test_{run_id}_{}", id.simple()))
        }
        Some(_) => Err(std::io::Error::other(
            "NOTEGATE_TEST_RUN_ID must contain exactly 16 lowercase hexadecimal digits",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{integration_test_env_value, schema_name};
    use uuid::Uuid;

    #[test]
    fn integration_environment_is_required_by_default() {
        for name in ["NOTEGATE_TEST_DATABASE_URL", "NOTEGATE_TEST_S3_ENDPOINT"] {
            for value in [None, Some(""), Some(" \t\n")] {
                for ci in [false, true] {
                    assert!(integration_test_env_value(name, value, false, ci).is_err());
                }
            }
        }
    }

    #[test]
    fn partial_runs_require_an_explicit_local_opt_out() -> Result<(), std::io::Error> {
        for value in [None, Some("postgres://configured")] {
            assert!(integration_test_env_value("TEST_SETTING", value, true, false)?.is_none());
            assert!(integration_test_env_value("TEST_SETTING", value, true, true).is_err());
        }
        assert_eq!(
            integration_test_env_value("TEST_SETTING", Some("postgres://configured"), false, true)?,
            Some("postgres://configured".to_owned())
        );
        Ok(())
    }

    #[test]
    fn run_scoped_schema_names_fit_postgres_identifiers() -> Result<(), std::io::Error> {
        let id = Uuid::new_v4();
        let name = schema_name(Some("0123456789abcdef"), id)?;
        assert_eq!(
            name,
            format!("notegate_test_0123456789abcdef_{}", id.simple())
        );
        assert_eq!(name.len(), 63);
        assert_eq!(
            schema_name(None, id)?,
            format!("notegate_test_{}", id.simple())
        );
        Ok(())
    }

    #[test]
    fn invalid_run_ids_are_rejected() {
        for run_id in [
            "",
            "short",
            "0123456789ABCDEF",
            "0123456789abcdef0",
            "0123456789abcde_",
            "'; DROP SCHEMA x",
        ] {
            assert!(schema_name(Some(run_id), Uuid::new_v4()).is_err());
        }
    }
}
