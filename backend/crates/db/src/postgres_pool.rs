//! Postgres connection-pool construction.

use notegate_core::{Config, Error, Result};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Build a Postgres connection pool from configuration.
pub async fn connect(config: &Config) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .connect(&config.database_url)
        .await
        .map_err(|e| Error::internal(format!("failed to connect to database: {e}")))
}
