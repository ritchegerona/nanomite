//! Configuration loading from environment variables.

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub minio: MinioConfig,
    pub auth: AuthConfig,
    pub nanomite: NanomiteConfig,
    pub workers: WorkersConfig,
    pub observability: ObservabilityConfig,
    pub security: SecurityConfig,
    pub features: FeatureFlags,
    pub encryption: EncryptionConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub workers: usize,
    pub max_body_size: usize,
    pub request_timeout_secs: u64,
    pub keep_alive_secs: u64,
    pub cors: CorsConfig,
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorsConfig {
    pub allowed_origins: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub requests_per_minute: u32,
    pub burst: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_secs: u64,
    pub idle_timeout_secs: u64,
    pub max_lifetime_secs: u64,
    pub migrations_path: String,
    pub enable_pgvector: bool,
    pub ssl_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisConfig {
    pub url: String,
    pub max_connections: u32,
    pub connection_timeout_secs: u64,
    pub command_timeout_secs: u64,
    pub cluster_mode: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinioConfig {
    pub endpoint: String,
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
    pub bucket: String,
    pub use_ssl: bool,
    pub presigned_ttl_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub jwt_issuer: String,
    pub jwt_audience: String,
    pub access_token_ttl_secs: u64,
    pub refresh_token_ttl_secs: u64,
    pub mfa_issuer: String,
    pub password_min_length: usize,
    pub max_login_attempts: u32,
    pub lockout_duration_secs: u64,
    pub session_timeout_secs: u64,
    pub oidc: OidcConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OidcConfig {
    pub enabled: bool,
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
    pub scopes: String,
    pub pkce_enabled: bool,
    pub provider: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NanomiteConfig {
    pub enabled: bool,
    pub binary_path: String,
    pub library_path: String,
    pub models_dir: String,
    pub skills_dir: String,
    pub memory_dir: String,
    pub max_concurrent_sessions: u32,
    pub default_max_steps: u32,
    pub reasoning_enabled: bool,
    pub planning_enabled: bool,
    pub memory_enabled: bool,
    pub skills_auto_install: bool,
    pub tool_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub runtime: String,
    pub network_enabled: bool,
    pub filesystem_read_only: bool,
    pub allowed_paths: Vec<String>,
    pub blocked_paths: Vec<String>,
    pub max_memory_mb: u64,
    pub max_cpu_percent: u32,
    pub max_pids: u32,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkersConfig {
    pub tool_executor_concurrency: usize,
    pub tool_executor_queue_size: usize,
    pub agent_processor_concurrency: usize,
    pub agent_processor_queue_size: usize,
    pub report_generator_concurrency: usize,
    pub evidence_processor_concurrency: usize,
    pub compliance_checker_concurrency: usize,
    pub scheduled_tasks_concurrency: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TracingConfig {
    pub enabled: bool,
    pub level: String,
    pub format: String,
    pub otlp_endpoint: String,
    pub otlp_protocol: String,
    pub jaeger_agent_endpoint: String,
    pub jaeger_collector_endpoint: String,
    pub jaeger_service_name: String,
    pub sampling_rate: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub port: u16,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
    pub output: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HealthChecksConfig {
    pub enabled: bool,
    pub port: u16,
    pub path: String,
    pub interval_secs: u64,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObservabilityConfig {
    pub tracing: TracingConfig,
    pub metrics: MetricsConfig,
    pub logging: LoggingConfig,
    pub health_checks: HealthChecksConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    pub content_security_policy: String,
    pub hsts_enabled: bool,
    pub hsts_max_age_secs: u64,
    pub hsts_include_subdomains: bool,
    pub hsts_preload: bool,
    pub frame_options: String,
    pub referrer_policy: String,
    pub permissions_policy: String,
    pub x_content_type_options: bool,
    pub x_xss_protection: bool,
    pub api_key_prefix: String,
    pub audit_log_retention_days: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeatureFlags {
    pub red_team: bool,
    pub blue_team: bool,
    pub purple_team: bool,
    pub ai_agent: bool,
    pub siem: bool,
    pub threat_intel: bool,
    pub compliance: bool,
    pub evidence_vault: bool,
    pub social_engineering: bool,
    pub wireless: bool,
    pub mobile: bool,
    pub hardware: bool,
    pub aiml: bool,
    pub exploit_dev: bool,
    pub training: bool,
    pub marketplace: bool,
    pub multi_tenant: bool,
    pub federation: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EncryptionConfig {
    pub algorithm: String,
    pub key_derivation: String,
    pub key_rotation_days: u32,
    pub envelope_encryption: bool,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        let config = config::Config::builder()
            .add_source(config::Environment::with_prefix("APP__").separator("__"))
            .build()
            .context("Failed to build configuration from environment")?;

        config
            .try_deserialize::<AppConfig>()
            .context("Failed to deserialize configuration")
    }
}