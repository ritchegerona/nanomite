#!/bin/bash
# Start Nanomite Command Center Backend - Native Mode
set -e

echo "🚀 Starting Nanomite Command Center Backend"

cd packages/backend

# Create data directories
mkdir -p ../../data/storage
mkdir -p ../../data/nanomite/{models,skills,memory}
mkdir -p ../../logs

# Export core environment variables directly
export APP__SERVER__HOST=0.0.0.0
export APP__SERVER__PORT=8080
export APP__SERVER__WORKERS=2
export APP__SERVER__MAX_BODY_SIZE=52428800
export APP__SERVER__REQUEST_TIMEOUT_SECS=300
export APP__SERVER__KEEP_ALIVE_SECS=75
export APP__SERVER__CORS__ALLOWED_ORIGINS="http://localhost:3000,http://localhost:5173"
export APP__SERVER__RATE_LIMIT__ENABLED=true
export APP__SERVER__RATE_LIMIT__REQUESTS_PER_MINUTE=1000
export APP__SERVER__RATE_LIMIT__BURST=200
export APP__DATABASE__URL="sqlite://./data/security_platform.db"
export APP__DATABASE__MAX_CONNECTIONS=5
export APP__DATABASE__MIN_CONNECTIONS=1
export APP__DATABASE__CONNECT_TIMEOUT_SECS=10
export APP__DATABASE__IDLE_TIMEOUT_SECS=600
export APP__DATABASE__MAX_LIFETIME_SECS=1800
export APP__DATABASE__MIGRATIONS_PATH="./migrations"
export APP__DATABASE__ENABLE_PGVECTOR=false
export APP__DATABASE__SSL_MODE=Prefer
export APP__DATABASE__USE_SQLITE=true
export APP__REDIS__URL=redis://localhost:6379
export APP__REDIS__MAX_CONNECTIONS=10
export APP__REDIS__CONNECTION_TIMEOUT_SECS=5
export APP__REDIS__COMMAND_TIMEOUT_SECS=30
export APP__REDIS__CLUSTER_MODE=false
export APP__STORAGE__STORAGE_TYPE=local
export APP__STORAGE__LOCAL_PATH="./data/storage"
export APP__STORAGE__BASE_URL=http://localhost:8080/storage
export APP__MINIO__ENDPOINT=localhost:9000
export APP__MINIO__ACCESS_KEY=minioadmin
export APP__MINIO__SECRET_KEY=minioadmin
export APP__MINIO__REGION=us-east-1
export APP__MINIO__BUCKET=security-platform
export APP__MINIO__USE_SSL=false
export APP__MINIO__PRESIGNED_TTL_SECS=3600
export APP__AUTH__JWT_SECRET="dev-secret-change-in-production-min-32-chars-long-enough"
export APP__AUTH__JWT_ISSUER=security-platform
export APP__AUTH__JWT_AUDIENCE=security-platform-api
export APP__AUTH__ACCESS_TOKEN_TTL_SECS=900
export APP__AUTH__REFRESH_TOKEN_TTL_SECS=604800
export APP__AUTH__MFA_ISSUER=Security_Platform
export APP__AUTH__PASSWORD_MIN_LENGTH=12
export APP__AUTH__MAX_LOGIN_ATTEMPTS=5
export APP__AUTH__LOCKOUT_DURATION_SECS=900
export APP__AUTH__SESSION_TIMEOUT_SECS=3600
export APP__AUTH__OIDC__ENABLED=false
export APP__NANOMITE__ENABLED=true
export APP__NANOMITE__MODELS_DIR="./data/nanomite/models"
export APP__NANOMITE__SKILLS_DIR="./data/nanomite/skills"
export APP__NANOMITE__MEMORY_DIR="./data/nanomite/memory"
export APP__NANOMITE__MAX_CONCURRENT_SESSIONS=10
export APP__NANOMITE__DEFAULT_MAX_STEPS=10
export APP__NANOMITE__REASONING_ENABLED=true
export APP__NANOMITE__PLANNING_ENABLED=true
export APP__NANOMITE__MEMORY_ENABLED=true
export APP__NANOMITE__SKILLS_AUTO_INSTALL=true
export APP__NANOMITE__TOOL_TIMEOUT_SECS=120
export APP__WORKERS__TOOL_EXECUTOR__CONCURRENCY=3
export APP__WORKERS__TOOL_EXECUTOR__QUEUE_SIZE=100
export APP__WORKERS__AGENT_PROCESSOR__CONCURRENCY=2
export APP__WORKERS__AGENT_PROCESSOR__QUEUE_SIZE=50
export APP__WORKERS__REPORT_GENERATOR__CONCURRENCY=1
export APP__WORKERS__EVIDENCE_PROCESSOR__CONCURRENCY=2
export APP__WORKERS__COMPLIANCE_CHECKER__CONCURRENCY=1
export APP__WORKERS__SCHEDULED_TASKS__CONCURRENCY=1
export APP__OBSERVABILITY__TRACING__ENABLED=true
export APP__OBSERVABILITY__TRACING__LEVEL=warn
export APP__OBSERVABILITY__TRACING__FORMAT=json
export APP__OBSERVABILITY__TRACING__OTLP_ENDPOINT=http://localhost:4317
export APP__OBSERVABILITY__TRACING__OTLP_PROTOCOL=grpc
export APP__OBSERVABILITY__TRACING__JAEGER_AGENT_ENDPOINT=http://localhost:6831
export APP__OBSERVABILITY__TRACING__JAEGER_COLLECTOR_ENDPOINT=http://localhost:14268
export APP__OBSERVABILITY__TRACING__JAEGER_SERVICE_NAME=security-platform-api
export APP__OBSERVABILITY__TRACING__SAMPLING_RATE=1.0
export APP__OBSERVABILITY__METRICS__ENABLED=true
export APP__OBSERVABILITY__METRICS__PORT=9090
export APP__OBSERVABILITY__HEALTH_CHECKS__ENABLED=true
export APP__OBSERVABILITY__HEALTH_CHECKS__PORT=8081
export APP__SECURITY__CONTENT_SECURITY_POLICY="default-src 'self'"
export APP__SECURITY__HSTS_ENABLED=false
export APP__SECURITY__HSTS_MAX_AGE_SECS=0
export APP__SECURITY__HSTS_INCLUDE_SUBDOMAINS=false
export APP__SECURITY__HSTS_PRELOAD=false
export APP__SECURITY__FRAME_OPTIONS=deny
export APP__SECURITY__REFERRER_POLICY=strict-origin-when-cross-origin
export APP__SECURITY__PERMISSIONS_POLICY=""
export APP__SECURITY__X_CONTENT_TYPE_OPTIONS=true
export APP__SECURITY__X_XSS_PROTECTION=true
export APP__SECURITY__API_KEY_PREFIX=sp_
export APP__SECURITY__AUDIT_LOG_RETENTION_DAYS=30
export APP__SECURITY__ENCRYPTION__ALGORITHM=AES-256-GCM
export APP__SECURITY__ENCRYPTION__KEY_DERIVATION=argon2id
export APP__SECURITY__ENCRYPTION__KEY_ROTATION_DAYS=90
export APP__SECURITY__ENCRYPTION__ENVELOPE_ENCRYPTION=true
export APP__FEATURES__RED_TEAM=true
export APP__FEATURES__BLUE_TEAM=true
export APP__FEATURES__PURPLE_TEAM=true
export APP__FEATURES__AI_AGENT=true
export APP__FEATURES__SIEM=true
export APP__FEATURES__THREAT_INTEL=true
export APP__FEATURES__COMPLIANCE=true
export APP__FEATURES__EVIDENCE_VAULT=true
export APP__SECURITY__ENCRYPTION__ALGORITHM=AES-256-GCM
export APP__SECURITY__ENCRYPTION__KEY_DERIVATION=argon2id
export APP_ENV=development
export APP_DEBUG=true
export APP_LOG_LEVEL=warn

export RUST_LOG=warn

echo "Starting API server on port 8080..."
cargo run --bin main &
BACKEND_PID=$!

echo "Waiting for backend to be ready..."
for i in $(seq 1 60); do
    if curl -s http://localhost:8081/health > /dev/null 2>&1; then
        echo "✓ Backend ready at http://localhost:8080"
        echo "Health endpoint: http://localhost:8081/health"
        break
    fi
    if ! kill -0 $BACKEND_PID 2>/dev/null; then
        echo "✗ Backend process exited unexpectedly"
        wait $BACKEND_PID
        exit 1
    fi
    sleep 1
done

if [ $i -eq 60 ]; then
    echo "✗ Backend failed to start within 60 seconds"
    exit 1
fi

wait $BACKEND_PID