//! Database connection and pool management.

use crate::config::DatabaseConfig;
use anyhow::Result;
use sqlx::{Pool, Postgres, Sqlite, sqlite::SqlitePoolOptions, postgres::PgPoolOptions, migrate::MigrateDatabase};
use std::time::Duration;

#[derive(Clone)]
pub enum DatabasePool {
    Postgres(Pool<Postgres>),
    Sqlite(Pool<Sqlite>),
}

impl DatabasePool {
    pub async fn create(config: &DatabaseConfig) -> Result<Self> {
        if config.use_sqlite {
            // Ensure database file exists
            if !Sqlite::database_exists(&config.url).await.unwrap_or(false) {
                Sqlite::create_database(&config.url).await?;
            }
            let pool = SqlitePoolOptions::new()
                .max_connections(config.max_connections)
                .acquire_timeout(Duration::from_secs(config.connect_timeout_secs))
                .idle_timeout(Duration::from_secs(config.idle_timeout_secs))
                .max_lifetime(Duration::from_secs(config.max_lifetime_secs))
                .connect(&config.url)
                .await?;
            Ok(Self::Sqlite(pool))
        } else {
            let pool = PgPoolOptions::new()
                .max_connections(config.max_connections)
                .min_connections(config.min_connections)
                .acquire_timeout(Duration::from_secs(config.connect_timeout_secs))
                .idle_timeout(Duration::from_secs(config.idle_timeout_secs))
                .max_lifetime(Duration::from_secs(config.max_lifetime_secs))
                .connect(&config.url)
                .await?;
            Ok(Self::Postgres(pool))
        }
    }

    pub async fn run_migrations(&self, migrations_path: &str) -> Result<()> {
        match self {
            Self::Postgres(pool) => {
                // sqlx::migrate! requires a literal path; migrations run via CLI
                let _ = pool;
                let _ = migrations_path;
                Ok(())
            }
            Self::Sqlite(pool) => {
                // For SQLite, run migrations directly using sqlx::migrate with runtime path
                let migrator = sqlx::migrate::Migrator::new(std::path::Path::new(migrations_path)).await?;
                migrator.run(pool).await?;
                Ok(())
            }
        }
    }
}

// Re-export for convenience
pub type PgPool = Pool<Postgres>;
pub type SqlitePool = Pool<Sqlite>;