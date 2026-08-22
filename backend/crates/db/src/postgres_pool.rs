//! Postgres connection-pool construction.

use notegate_core::{Config, Error, Result};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tracing::log::LevelFilter;

/// Process-local database pools. The read handle aliases `primary` unless the
/// composition root requests a configured dedicated read endpoint.
#[derive(Debug, Clone)]
pub struct PgPools {
    primary: PgPool,
    read: PgPool,
    primary_max_connections: u32,
    read_max_connections: u32,
    separate_read: bool,
}

impl PgPools {
    pub async fn connect(config: &Config, use_read_pool: bool) -> Result<Self> {
        let primary = connect_with(
            &config.database_url,
            config.db_max_connections,
            config.metrics_enabled,
            "primary",
        )
        .await?;
        let (read, read_max_connections, separate_read) =
            match select_read_endpoint(use_read_pool, config.read_database_url.as_deref()) {
                Some(url) => {
                    match connect_with(
                        url,
                        config.read_db_max_connections,
                        config.metrics_enabled,
                        "read",
                    )
                    .await
                    {
                        Ok(read) => (read, config.read_db_max_connections, true),
                        Err(error) => {
                            primary.close().await;
                            return Err(error);
                        }
                    }
                }
                None => (primary.clone(), config.db_max_connections, false),
            };
        Ok(Self {
            primary,
            read,
            primary_max_connections: config.db_max_connections,
            read_max_connections,
            separate_read,
        })
    }

    pub fn primary(&self) -> &PgPool {
        &self.primary
    }

    pub fn read(&self) -> &PgPool {
        &self.read
    }

    pub const fn primary_max_connections(&self) -> u32 {
        self.primary_max_connections
    }

    pub const fn read_max_connections(&self) -> u32 {
        self.read_max_connections
    }

    pub const fn has_separate_read_pool(&self) -> bool {
        self.separate_read
    }

    pub async fn close(self) {
        if self.separate_read {
            self.read.close().await;
        }
        self.primary.close().await;
    }
}

fn select_read_endpoint(use_read_pool: bool, configured_url: Option<&str>) -> Option<&str> {
    use_read_pool.then_some(configured_url).flatten()
}

/// Build a Postgres connection pool from configuration.
pub async fn connect(config: &Config) -> Result<PgPool> {
    connect_with(
        &config.database_url,
        config.db_max_connections,
        config.metrics_enabled,
        "primary",
    )
    .await
}

async fn connect_with(
    database_url: &str,
    max_connections: u32,
    metrics_enabled: bool,
    role: &'static str,
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
        .map_err(|e| Error::internal(format!("failed to connect to {role} database: {e}")))
}

#[cfg(test)]
mod tests {
    use super::select_read_endpoint;

    #[test]
    fn read_endpoint_is_selected_only_when_requested() {
        let configured = Some("postgres://read");

        assert_eq!(select_read_endpoint(false, configured), None);
        assert_eq!(select_read_endpoint(true, configured), configured);
        assert_eq!(select_read_endpoint(true, None), None);
    }
}
