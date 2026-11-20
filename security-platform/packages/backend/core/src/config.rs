//! Configuration loading from environment variables (direct std::env).
//! Bypasses the `config` crate to avoid deserialization issues.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::env;

fn get_env(key: &str) -> Result<String> {
    env::var(key).with_context(|| format!("Missing required environment variable: {}", key))
}

fn get_env_opt(key: &str) -> Option<String> {
    env::var(key).ok()
}

fn get_env_bool(key: &str) -> Result<bool> {
    let val = get_env(key)?;
    Ok(val.to_lowercase() == "true" || val == "1")
}

fn get_env_u32(key: &str) -> Result<u32> {
    let val = get_env(key)?;
    val.parse::<u32>().with_context(|| format!("Invalid u32 value for {}: {}", key, val))
}

fn get_env_u64(key: &str) -> Result<u64> {
    let val = get_env(key)?;
    val.parse::<u64>().with_context(|| format!("Invalid u64 value for {}: {}", key, val))
}

fn get_env_usize(key: &str) -> Result<usize> {
    let val = get_env(key)?;
    val.parse::<usize>().with_context(|| format!("Invalid usize value for {}: {}", key, val))
}

fn get_env_f64(key: &str) -> Result<f64> {
    let val = get_env(key)?;
    val.parse::<f64>().with_context(|| format!("Invalid f64 value for {}: {}", key, val))
}

fn get_env_str_opt(key: &str) -> Option<String> {
    env::var(key).ok()
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub minio: MinioConfig,
    pub storage: StorageConfig,
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
    pub use_sqlite: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    pub storage_type: String,
    pub local_path: Option<String>,
    pub base_url: Option<String>,
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
        // Load .env file if present
        dotenvy::dotenv().ok();

        let server = ServerConfig {
            host: get_env("APP__SERVER__HOST")?,
            port: get_env("APP__SERVER__PORT")?.parse::<u16>()?,
            workers: get_env_usize("APP__SERVER__WORKERS")?,
            max_body_size: get_env_usize("APP__SERVER__MAX_BODY_SIZE")?,
            request_timeout_secs: get_env_u64("APP__SERVER__REQUEST_TIMEOUT_SECS")?,
            keep_alive_secs: get_env_u64("APP__SERVER__KEEP_ALIVE_SECS")?,
            cors: CorsConfig {
                allowed_origins: get_env("APP__SERVER__CORS__ALLOWED_ORIGINS")?,
            },
            rate_limit: RateLimitConfig {
                enabled: get_env_bool("APP__SERVER__RATE_LIMIT__ENABLED")?,
                requests_per_minute: get_env_u32("APP__SERVER__RATE_LIMIT__REQUESTS_PER_MINUTE")?,
                burst: get_env_u32("APP__SERVER__RATE_LIMIT__BURST")?,
            },
        };

        let database = DatabaseConfig {
            url: get_env("APP__DATABASE__URL")?,
            max_connections: get_env_u32("APP__DATABASE__MAX_CONNECTIONS")?,
            min_connections: get_env_u32("APP__DATABASE__MIN_CONNECTIONS")?,
            connect_timeout_secs: get_env_u64("APP__DATABASE__CONNECT_TIMEOUT_SECS")?,
            idle_timeout_secs: get_env_u64("APP__DATABASE__IDLE_TIMEOUT_SECS")?,
            max_lifetime_secs: get_env_u64("APP__DATABASE__MAX_LIFETIME_SECS")?,
            migrations_path: get_env("APP__DATABASE__MIGRATIONS_PATH")?,
            enable_pgvector: get_env_bool("APP__DATABASE__ENABLE_PGVECTOR")?,
            ssl_mode: get_env("APP__DATABASE__SSL_MODE")?,
            use_sqlite: get_env_bool("APP__DATABASE__USE_SQLITE")?,
        };

        let redis = RedisConfig {
            url: get_env("APP__REDIS__URL")?,
            max_connections: get_env_u32("APP__REDIS__MAX_CONNECTIONS")?,
            connection_timeout_secs: get_env_u64("APP__REDIS__CONNECTION_TIMEOUT_SECS")?,
            command_timeout_secs: get_env_u64("APP__REDIS__COMMAND_TIMEOUT_SECS")?,
            cluster_mode: get_env_bool("APP__REDIS__CLUSTER_MODE")?,
        };

        let storage = StorageConfig {
            storage_type: get_env("APP__STORAGE__STORAGE_TYPE")?,
            local_path: get_env_str_opt("APP__STORAGE__LOCAL_PATH"),
            base_url: get_env_str_opt("APP__STORAGE__BASE_URL"),
        };

        let minio = MinioConfig {
            endpoint: get_env("APP__MINIO__ENDPOINT")?,
            access_key: get_env("APP__MINIO__ACCESS_KEY")?,
            secret_key: get_env("APP__MINIO__SECRET_KEY")?,
            region: get_env("APP__MINIO__REGION")?,
            bucket: get_env("APP__MINIO__BUCKET")?,
            use_ssl: get_env_bool("APP__MINIO__USE_SSL")?,
            presigned_ttl_secs: get_env_u64("APP__MINIO__PRESIGNED_TTL_SECS")?,
        };

        let auth = AuthConfig {
            jwt_secret: get_env("APP__AUTH__JWT_SECRET")?,
            jwt_issuer: get_env("APP__AUTH__JWT_ISSUER")?,
            jwt_audience: get_env("APP__AUTH__JWT_AUDIENCE")?,
            access_token_ttl_secs: get_env_u64("APP__AUTH__ACCESS_TOKEN_TTL_SECS")?,
            refresh_token_ttl_secs: get_env_u64("APP__AUTH__REFRESH_TOKEN_TTL_SECS")?,
            mfa_issuer: get_env("APP__AUTH__MFA_ISSUER")?,
            password_min_length: get_env_usize("APP__AUTH__PASSWORD_MIN_LENGTH")?,
            max_login_attempts: get_env_u32("APP__AUTH__MAX_LOGIN_ATTEMPTS")?,
            lockout_duration_secs: get_env_u64("APP__AUTH__LOCKOUT_DURATION_SECS")?,
            session_timeout_secs: get_env_u64("APP__AUTH__SESSION_TIMEOUT_SECS")?,
            oidc: OidcConfig {
                enabled: get_env_bool("APP__AUTH__OIDC__ENABLED")?,
                issuer_url: get_env_str_opt("APP__AUTH__OIDC__ISSUER_URL").unwrap_or_default(),
                client_id: get_env_str_opt("APP__AUTH__OIDC__CLIENT_ID").unwrap_or_default(),
                client_secret: get_env_str_opt("APP__AUTH__OIDC__CLIENT_SECRET").unwrap_or_default(),
                redirect_url: get_env_str_opt("APP__AUTH__OIDC__REDIRECT_URL").unwrap_or_default(),
                scopes: get_env_str_opt("APP__AUTH__OIDC__SCOPES").unwrap_or_default(),
                pkce_enabled: get_env_bool("APP__AUTH__OIDC__PKCE_ENABLED").unwrap_or(true),
                provider: get_env_str_opt("APP__AUTH__OIDC__PROVIDER").unwrap_or_default(),
            },
        };

        let nanomite = NanomiteConfig {
            enabled: get_env_bool("APP__NANOMITE__ENABLED")?,
            binary_path: get_env_str_opt("APP__NANOMITE__BINARY_PATH").unwrap_or_default(),
            library_path: get_env_str_opt("APP__NANOMITE__LIBRARY_PATH").unwrap_or_default(),
            models_dir: get_env("APP__NANOMITE__MODELS_DIR")?,
            skills_dir: get_env("APP__NANOMITE__SKILLS_DIR")?,
            memory_dir: get_env("APP__NANOMITE__MEMORY_DIR")?,
            max_concurrent_sessions: get_env_u32("APP__NANOMITE__MAX_CONCURRENT_SESSIONS")?,
            default_max_steps: get_env_u32("APP__NANOMITE__DEFAULT_MAX_STEPS")?,
            reasoning_enabled: get_env_bool("APP__NANOMITE__REASONING_ENABLED")?,
            planning_enabled: get_env_bool("APP__NANOMITE__PLANNING_ENABLED")?,
            memory_enabled: get_env_bool("APP__NANOMITE__MEMORY_ENABLED")?,
            skills_auto_install: get_env_bool("APP__NANOMITE__SKILLS_AUTO_INSTALL")?,
            tool_timeout_secs: get_env_u64("APP__NANOMITE__TOOL_TIMEOUT_SECS")?,
        };

        let workers = WorkersConfig {
            tool_executor_concurrency: get_env_usize("APP__WORKERS__TOOL_EXECUTOR__CONCURRENCY")?,
            tool_executor_queue_size: get_env_usize("APP__WORKERS__TOOL_EXECUTOR__QUEUE_SIZE")?,
            agent_processor_concurrency: get_env_usize("APP__WORKERS__AGENT_PROCESSOR__CONCURRENCY")?,
            agent_processor_queue_size: get_env_usize("APP__WORKERS__AGENT_PROCESSOR__QUEUE_SIZE")?,
            report_generator_concurrency: get_env_usize("APP__WORKERS__REPORT_GENERATOR__CONCURRENCY")?,
            evidence_processor_concurrency: get_env_usize("APP__WORKERS__EVIDENCE_PROCESSOR__CONCURRENCY")?,
            compliance_checker_concurrency: get_env_usize("APP__WORKERS__COMPLIANCE_CHECKER__CONCURRENCY")?,
            scheduled_tasks_concurrency: get_env_usize("APP__WORKERS__SCHEDULED_TASKS__CONCURRENCY")?,
        };

        let observability = ObservabilityConfig {
            tracing: TracingConfig {
                enabled: get_env_bool("APP__OBSERVABILITY__TRACING__ENABLED")?,
                level: get_env("APP__OBSERVABILITY__TRACING__LEVEL")?,
                format: get_env("APP__OBSERVABILITY__TRACING__FORMAT")?,
                otlp_endpoint: get_env_str_opt("APP__OBSERVABILITY__TRACING__OTLP_ENDPOINT").unwrap_or_default(),
                otlp_protocol: get_env_str_opt("APP__OBSERVABILITY__TRACING__OTLP_PROTOCOL").unwrap_or_default(),
                jaeger_agent_endpoint: get_env_str_opt("APP__OBSERVABILITY__TRACING__JAEGER_AGENT_ENDPOINT").unwrap_or_default(),
                jaeger_collector_endpoint: get_env_str_opt("APP__OBSERVABILITY__TRACING__JAEGER_COLLECTOR_ENDPOINT").unwrap_or_default(),
                jaeger_service_name: get_env_str_opt("APP__OBSERVABILITY__TRACING__JAEGER_SERVICE_NAME").unwrap_or_default(),
                sampling_rate: get_env_f64("APP__OBSERVABILITY__TRACING__SAMPLING_RATE").unwrap_or(1.0),
            },
            metrics: MetricsConfig {
                enabled: get_env_bool("APP__OBSERVABILITY__METRICS__ENABLED")?,
                port: get_env("APP__OBSERVABILITY__METRICS__PORT")?.parse::<u16>()?,
                path: get_env_str_opt("APP__OBSERVABILITY__METRICS__PATH").unwrap_or("/metrics".to_string()),
            },
            logging: LoggingConfig {
                level: get_env_str_opt("APP__OBSERVABILITY__LOGGING__LEVEL").unwrap_or("info".to_string()),
                format: get_env_str_opt("APP__OBSERVABILITY__LOGGING__FORMAT").unwrap_or("json".to_string()),
                output: get_env_str_opt("APP__OBSERVABILITY__LOGGING__OUTPUT").unwrap_or("stdout".to_string()),
            },
            health_checks: HealthChecksConfig {
                enabled: get_env_bool("APP__OBSERVABILITY__HEALTH_CHECKS__ENABLED")?,
                port: get_env("APP__OBSERVABILITY__HEALTH_CHECKS__PORT")?.parse::<u16>()?,
                path: get_env_str_opt("APP__OBSERVABILITY__HEALTH_CHECKS__PATH").unwrap_or("/health".to_string()),
                interval_secs: get_env_u64("APP__OBSERVABILITY__HEALTH_CHECKS__INTERVAL_SECS").unwrap_or(30),
                timeout_secs: get_env_u64("APP__OBSERVABILITY__HEALTH_CHECKS__TIMEOUT_SECS").unwrap_or(10),
            },
        };

        let security = SecurityConfig {
            content_security_policy: get_env_str_opt("APP__SECURITY__CONTENT_SECURITY_POLICY").unwrap_or_default(),
            hsts_enabled: get_env_bool("APP__SECURITY__HSTS_ENABLED").unwrap_or(false),
            hsts_max_age_secs: get_env_u64("APP__SECURITY__HSTS_MAX_AGE_SECS").unwrap_or(0),
            hsts_include_subdomains: get_env_bool("APP__SECURITY__HSTS_INCLUDE_SUBDOMAINS").unwrap_or(false),
            hsts_preload: get_env_bool("APP__SECURITY__HSTS_PRELOAD").unwrap_or(false),
            frame_options: get_env_str_opt("APP__SECURITY__FRAME_OPTIONS").unwrap_or_default(),
            referrer_policy: get_env_str_opt("APP__SECURITY__REFERRER_POLICY").unwrap_or_default(),
            permissions_policy: get_env_str_opt("APP__SECURITY__PERMISSIONS_POLICY").unwrap_or_default(),
            x_content_type_options: get_env_bool("APP__SECURITY__X_CONTENT_TYPE_OPTIONS").unwrap_or(false),
            x_xss_protection: get_env_bool("APP__SECURITY__X_XSS_PROTECTION").unwrap_or(false),
            api_key_prefix: get_env_str_opt("APP__SECURITY__API_KEY_PREFIX").unwrap_or("sp_".to_string()),
            audit_log_retention_days: get_env_u32("APP__SECURITY__AUDIT_LOG_RETENTION_DAYS").unwrap_or(30),
        };

        let features = FeatureFlags {
            red_team: get_env_bool("APP__FEATURES__RED_TEAM").unwrap_or(true),
            blue_team: get_env_bool("APP__FEATURES__BLUE_TEAM").unwrap_or(true),
            purple_team: get_env_bool("APP__FEATURES__PURPLE_TEAM").unwrap_or(true),
            ai_agent: get_env_bool("APP__FEATURES__AI_AGENT").unwrap_or(true),
            siem: get_env_bool("APP__FEATURES__SIEM").unwrap_or(true),
            threat_intel: get_env_bool("APP__FEATURES__THREAT_INTEL").unwrap_or(true),
            compliance: get_env_bool("APP__FEATURES__COMPLIANCE").unwrap_or(true),
            evidence_vault: get_env_bool("APP__FEATURES__EVIDENCE_VAULT").unwrap_or(true),
            social_engineering: get_env_bool("APP__FEATURES__SOCIAL_ENGINEERING").unwrap_or(true),
            wireless: get_env_bool("APP__FEATURES__WIRELESS").unwrap_or(true),
            mobile: get_env_bool("APP__FEATURES__MOBILE").unwrap_or(true),
            hardware: get_env_bool("APP__FEATURES__HARDWARE").unwrap_or(true),
            aiml: get_env_bool("APP__FEATURES__AIML").unwrap_or(true),
            exploit_dev: get_env_bool("APP__FEATURES__EXPLOIT_DEV").unwrap_or(true),
            training: get_env_bool("APP__FEATURES__TRAINING").unwrap_or(true),
            marketplace: get_env_bool("APP__FEATURES__MARKETPLACE").unwrap_or(false),
            multi_tenant: get_env_bool("APP__FEATURES__MULTI_TENANT").unwrap_or(true),
            federation: get_env_bool("APP__FEATURES__FEDERATION").unwrap_or(false),
        };

        let encryption = EncryptionConfig {
            algorithm: get_env_str_opt("APP__SECURITY__ENCRYPTION__ALGORITHM").unwrap_or("AES-256-GCM".to_string()),
            key_derivation: get_env_str_opt("APP__SECURITY__ENCRYPTION__KEY_DERIVATION").unwrap_or("argon2id".to_string()),
            key_rotation_days: get_env_u32("APP__SECURITY__ENCRYPTION__KEY_ROTATION_DAYS").unwrap_or(90),
            envelope_encryption: get_env_bool("APP__SECURITY__ENCRYPTION__ENVELOPE_ENCRYPTION").unwrap_or(true),
        };

        Ok(AppConfig {
            server,
            database,
            redis,
            minio,
            storage,
            auth,
            nanomite,
            workers,
            observability,
            security,
            features,
            encryption,
        })
    }
}