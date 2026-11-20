//! Error types for Security Platform.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(#[from] anyhow::Error),

    #[error("Not found: {resource} {id}")]
    NotFound { resource: String, id: String },

    #[error("Validation error: {field} - {message}")]
    Validation { field: String, message: String },

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Tool execution error: {tool} - {message}")]
    ToolExecution { tool: String, message: String },

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;