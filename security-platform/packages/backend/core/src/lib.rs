//! Core domain models, database, auth, config, and error types for Security Platform.

pub mod config;
pub mod db;
pub mod error;
pub mod models;
pub mod auth;
pub mod storage;

pub fn init() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    Ok(())
}