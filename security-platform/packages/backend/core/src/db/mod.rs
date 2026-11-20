//! Database connection and pool management.

use crate::config::DatabaseConfig;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

pub async fn create_pool(config: &DatabaseConfig) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.connect_timeout_secs))
        .idle_timeout(Duration::from_secs(config.idle_timeout_secs))
        .max_lifetime(Duration::from_secs(config.max_lifetime_secs))
        .connect(&config.url)
        .await?;

    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool, _migrations_path: &str) -> anyhow::Result<()> {
    // sqlx::migrate! requires a literal path; migrations run via CLI
    let _ = pool;
    Ok(())
}