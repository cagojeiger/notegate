//! Database access: migrations and aggregate repositories.
//!
//! Repositories here expose concrete Postgres-backed persistence methods. Pool
//! construction also lives here so Postgres infrastructure stays with the
//! Postgres adapter. Queries use runtime-checked
//! `query_as::<_, Row>()` / `query()` (not the `query!` macro) so a schema reset
//! never blocks compilation.

use notegate_core::{Error, Result};

mod account_delete;
pub mod account_repo;
pub mod agent_repo;
pub mod api_key_repo;
pub mod audit_event_repo;
mod audit_events;
pub mod browser_session_repo;
pub mod connection_repo;
mod event_history_query;
mod file_change_event_repo;
mod file_change_events;
pub mod files;
pub mod files_repo;
pub mod key_epoch_repo;
pub mod postgres_pool;
pub mod purge_repo;
mod space_permission;
mod space_usage;
pub mod space_usage_repo;
pub mod spaces_repo;
#[cfg(any(test, feature = "test-util"))]
pub mod test_support;
mod tier_lookup;

pub use account_repo::AccountRepo;
pub use agent_repo::AgentRepo;
pub use api_key_repo::ApiKeyRepo;
pub use audit_event_repo::AuditEventRepo;
pub use browser_session_repo::BrowserSessionRepo;
pub use connection_repo::ConnectionRepo;
pub use files_repo::{FilesRepo, MetadataMutationKind, TextMutationKind};
pub use key_epoch_repo::CryptoKeyEpochRepo;
pub use postgres_pool::connect;
pub use purge_repo::{PurgeRepo, PurgeRun};
pub use space_usage_repo::{SpaceUsageRepo, UsageCounts, UsageReconcileRun};
pub use spaces_repo::SpaceRepo;
pub use sqlx::PgPool;

/// Generic internal mapping for any repository query failure. Shared by every
/// repo so the mapping never drifts; detail is logged, not surfaced.
pub(crate) fn map_sqlx_error(error: sqlx::Error) -> Error {
    Error::internal(format!("database query failed: {error}"))
}

/// Convert a non-negative SQL count into `usize`. A negative value can only come
/// from a corrupt aggregate, so it is an internal error.
pub(crate) fn to_usize(value: i64, label: &str) -> Result<usize> {
    usize::try_from(value).map_err(|_error| Error::internal(format!("negative {label} count")))
}

/// The single SQL definition of a live account: active and not soft-deleted.
/// `prefix` qualifies the columns (e.g. `"acc."` inside a join, `""` otherwise) so
/// every account-liveness check across the repos shares one predicate and cannot
/// drift. Mirrors the Rust-side `notegate_model::account::Account::is_live`.
pub(crate) fn active_account_predicate(prefix: &str) -> String {
    format!("{prefix}is_active = true AND {prefix}deleted_at IS NULL")
}

/// Embedded migrations from `migrations/`, run at startup via [`run_migrations`].
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Apply any pending migrations.
pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    MIGRATOR
        .run(pool)
        .await
        .map_err(|e| Error::internal(format!("migration failed: {e}")))
}

/// Verify the database is usable by the API process.
///
/// This checks both connectivity and that every embedded migration has a
/// successful row with the expected checksum. Startup already runs migrations;
/// readiness repeats a cheap validation so load balancers do not route traffic
/// to a process connected to the wrong or reset database.
pub async fn check_readiness(pool: &PgPool) -> Result<()> {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .map_err(|e| Error::internal(format!("database ping failed: {e}")))?;

    for migration in MIGRATOR.iter() {
        let applied: Option<(Vec<u8>, bool)> =
            sqlx::query_as("SELECT checksum, success FROM _sqlx_migrations WHERE version = $1")
                .bind(migration.version)
                .fetch_optional(pool)
                .await
                .map_err(|e| Error::internal(format!("migration readiness check failed: {e}")))?;

        let Some((checksum, success)) = applied else {
            return Err(Error::internal(format!(
                "migration {} has not been applied",
                migration.version
            )));
        };
        if !success {
            return Err(Error::internal(format!(
                "migration {} was not successful",
                migration.version
            )));
        }
        if checksum.as_slice() != migration.checksum.as_ref() {
            return Err(Error::internal(format!(
                "migration {} checksum mismatch",
                migration.version
            )));
        }
    }

    Ok(())
}
