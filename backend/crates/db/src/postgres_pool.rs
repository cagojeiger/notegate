//! Postgres connection-pool construction.

use notegate_core::{Config, Error, Result};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tracing::log::LevelFilter;

/// Build a Postgres connection pool from configuration.
pub async fn connect(config: &Config) -> Result<PgPool> {
    let options = PgPoolOptions::new().max_connections(config.db_max_connections);
    let options = if config.metrics_enabled {
        options.acquire_time_level(LevelFilter::Trace)
    } else {
        options
    };
    options
        .connect(&config.database_url)
        .await
        .map_err(|e| Error::internal(format!("failed to connect to database: {e}")))
}
