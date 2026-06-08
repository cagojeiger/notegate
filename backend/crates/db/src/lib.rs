//! Database access: migrations and aggregate repositories.
//!
//! Repositories here implement the `notegate-service` store traits. Pool
//! construction also lives here so Postgres infrastructure stays with the
//! Postgres adapter. Queries use runtime-checked
//! `query_as::<_, Row>()` / `query()` (not the `query!` macro) so a schema reset
//! never blocks compilation.

use notegate_core::{Error, Result};

pub mod access;
pub mod account_repo;
pub mod agent_repo;
pub mod files;
pub mod files_repo;
pub mod postgres_pool;
pub mod workspaces;

pub use access::AccessRepo;
pub use account_repo::AccountRepo;
pub use agent_repo::AgentRepo;
pub use files_repo::FilesRepo;
pub use postgres_pool::connect;
pub use sqlx::PgPool;
pub use workspaces::WorkspaceRepo;

/// Embedded migrations from `migrations/`, run at startup via [`run_migrations`].
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Apply any pending migrations.
pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    MIGRATOR
        .run(pool)
        .await
        .map_err(|e| Error::internal(format!("migration failed: {e}")))
}
