//! Postgres connection-pool construction.

use notegate_core::{Config, Error, Result};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tracing::log::LevelFilter;

/// Build a Postgres connection pool from configuration.
pub async fn connect(config: &Config) -> Result<PgPool> {
    connect_with(
        &config.database_url,
        config.db_max_connections,
        config.metrics_enabled,
    )
    .await
}

/// Build a Postgres connection pool for a process with a narrower configuration surface.
pub async fn connect_with(
    database_url: &str,
    max_connections: u32,
    metrics_enabled: bool,
) -> Result<PgPool> {
    let options = PgPoolOptions::new().max_connections(max_connections);
    let options = if metrics_enabled {
        options.acquire_time_level(LevelFilter::Trace)
    } else {
        options
    };
    options
        .connect(database_url)
        .await
        .map_err(|e| Error::internal(format!("failed to connect to database: {e}")))
}
