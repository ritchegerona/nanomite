#!/bin/bash
# Native development setup - no Docker required
# Uses SQLite (no PostgreSQL needed), local Redis, and file storage

set -e

echo "🚀 Setting up Nanomite Command Center - Native Development (Lightweight)"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

# Check dependencies
check_dep() {
    if ! command -v $1 &> /dev/null; then
        echo -e "${RED}❌ $1 not found. Please install: $2${NC}"
        return 1
    fi
    echo -e "${GREEN}✓ $1 found${NC}"
}

echo "📋 Checking dependencies..."
check_dep cargo "https://rustup.rs/" || exit 1
check_dep node "https://nodejs.org/" || exit 1
check_dep npm "comes with node" || exit 1

# Redis is optional for lightweight dev
if command -v redis-server &> /dev/null; then
    echo -e "${GREEN}✓ redis-server found${NC}"
    HAS_REDIS=true
else
    echo -e "${YELLOW}⚠ redis-server not found - will use in-memory queue (limited)${NC}"
    HAS_REDIS=false
fi

# Create local directories
echo "📁 Creating local directories..."
mkdir -p data/storage
mkdir -p data/nanomite/{models,skills,memory}
mkdir -p logs

# Start Redis if available
if [ "$HAS_REDIS" = true ]; then
    if ! redis-cli ping &> /dev/null; then
        echo "🔴 Starting Redis..."
        if command -v brew &> /dev/null; then
            brew services start redis || true
        elif command -v systemctl &> /dev/null; then
            sudo systemctl start redis || true
        else
            redis-server --daemonize yes --dir ./data/redis
        fi
        sleep 2
    fi
fi

# Create .env.local if not exists (overrides .env for local dev)
if [ ! -f .env.local ]; then
    echo "⚙️  Creating .env.local..."
    cat > .env.local << 'EOF'
# Lightweight Native Development Configuration
# Uses SQLite + Local Storage + Optional Redis

# Application
APP_ENV=development
APP_DEBUG=true
APP_LOG_LEVEL=debug

# Server
APP__SERVER__HOST=0.0.0.0
APP__SERVER__PORT=8080
APP__SERVER__WORKERS=2
APP__SERVER__MAX_BODY_SIZE=52428800
APP__SERVER__REQUEST_TIMEOUT_SECS=300
APP__SERVER__KEEP_ALIVE_SECS=75

# CORS
APP__SERVER__CORS__ALLOWED_ORIGINS=http://localhost:3000,http://localhost:5173

# Rate Limiting
APP__SERVER__RATE_LIMIT__ENABLED=true
APP__SERVER__RATE_LIMIT__REQUESTS_PER_MINUTE=1000
APP__SERVER__RATE_LIMIT__BURST=200

# Database - SQLite for zero-config local dev
APP__DATABASE__URL=sqlite://./data/security_platform.db
APP__DATABASE__MAX_CONNECTIONS=5
APP__DATABASE__MIN_CONNECTIONS=1
APP__DATABASE__CONNECT_TIMEOUT_SECS=10
APP__DATABASE__IDLE_TIMEOUT_SECS=600
APP__DATABASE__MAX_LIFETIME_SECS=1800
APP__DATABASE__MIGRATIONS_PATH=./migrations
APP__DATABASE__ENABLE_PGVECTOR=false
APP__DATABASE__SSL_MODE=Prefer
APP__DATABASE__USE_SQLITE=true

# Redis - optional
APP__REDIS__URL=redis://localhost:6379
APP__REDIS__MAX_CONNECTIONS=10
APP__REDIS__CONNECTION_TIMEOUT_SECS=5
APP__REDIS__COMMAND_TIMEOUT_SECS=30
APP__REDIS__CLUSTER_MODE=false

# Storage - Local filesystem
APP__STORAGE__STORAGE_TYPE=local
APP__STORAGE__LOCAL_PATH=./data/storage
APP__STORAGE__BASE_URL=http://localhost:8080/storage

# Auth
APP__AUTH__JWT_SECRET=dev-secret-change-in-production-min-32-chars-long-enough
APP__AUTH__JWT_ISSUER=security-platform
APP__AUTH__JWT_AUDIENCE=security-platform-api
APP__AUTH__ACCESS_TOKEN_TTL_SECS=900
APP__AUTH__REFRESH_TOKEN_TTL_SECS=604800
APP__AUTH__MFA_ISSUER=Security Platform
APP__AUTH__PASSWORD_MIN_LENGTH=12
APP__AUTH__MAX_LOGIN_ATTEMPTS=5
APP__AUTH__LOCKOUT_DURATION_SECS=900
APP__AUTH__SESSION_TIMEOUT_SECS=3600
APP__AUTH__OIDC__ENABLED=false

# Nanomite AI Agent
APP__NANOMITE__ENABLED=true
APP__NANOMITE__BINARY_PATH=
APP__NANOMITE__LIBRARY_PATH=
APP__NANOMITE__MODELS_DIR=./data/nanomite/models
APP__NANOMITE__SKILLS_DIR=./data/nanomite/skills
APP__NANOMITE__MEMORY_DIR=./data/nanomite/memory
APP__NANOMITE__MAX_CONCURRENT_SESSIONS=10
APP__NANOMITE__DEFAULT_MAX_STEPS=10
APP__NANOMITE__REASONING_ENABLED=true
APP__NANOMITE__PLANNING_ENABLED=true
APP__NANOMITE__MEMORY_ENABLED=true
APP__NANOMITE__SKILLS_AUTO_INSTALL=true
APP__NANOMITE__TOOL_TIMEOUT_SECS=120

# Workers - reduced for lightweight
APP__WORKERS__TOOL_EXECUTOR__CONCURRENCY=3
APP__WORKERS__TOOL_EXECUTOR__QUEUE_SIZE=100
APP__WORKERS__AGENT_PROCESSOR__CONCURRENCY=2
APP__WORKERS__AGENT_PROCESSOR__QUEUE_SIZE=50
APP__WORKERS__REPORT_GENERATOR__CONCURRENCY=1
APP__WORKERS__EVIDENCE_PROCESSOR__CONCURRENCY=2
APP__WORKERS__COMPLIANCE_CHECKER__CONCURRENCY=1
APP__WORKERS__SCHEDULED_TASKS__CONCURRENCY=1

# Observability
APP__OBSERVABILITY__TRACING__ENABLED=true
APP__OBSERVABILITY__TRACING__LEVEL=debug
APP__OBSERVABILITY__TRACING__FORMAT=json
APP__OBSERVABILITY__TRACING__OTLP_ENDPOINT=http://localhost:4317
APP__OBSERVABILITY__TRACING__OTLP_PROTOCOL=grpc
APP__OBSERVABILITY__TRACING__JAEGER_AGENT_ENDPOINT=http://localhost:6831
APP__OBSERVABILITY__TRACING__JAEGER_COLLECTOR_ENDPOINT=http://localhost:14268
APP__OBSERVABILITY__TRACING__JAEGER_SERVICE_NAME=security-platform-api
APP__OBSERVABILITY__TRACING__SAMPLING_RATE=1.0

APP__OBSERVABILITY__METRICS__ENABLED=true
APP__OBSERVABILITY__METRICS__PORT=9090
APP__OBSERVABILITY__METRICS__PATH=/metrics

APP__OBSERVABILITY__LOGGING__LEVEL=debug
APP__OBSERVABILITY__LOGGING__FORMAT=json
APP__OBSERVABILITY__LOGGING__OUTPUT=stdout

APP__OBSERVABILITY__HEALTH_CHECKS__ENABLED=true
APP__OBSERVABILITY__HEALTH_CHECKS__PORT=8081
APP__OBSERVABILITY__HEALTH_CHECKS__PATH=/health
APP__OBSERVABILITY__HEALTH_CHECKS__INTERVAL_SECS=30
APP__OBSERVABILITY__HEALTH_CHECKS__TIMEOUT_SECS=10

# Security
APP__SECURITY__CONTENT_SECURITY_POLICY=default-src 'self'; script-src 'self' 'unsafe-inline' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; font-src 'self' data:; connect-src 'self' wss: https:; frame-ancestors 'none'; base-uri 'self'; form-action 'self'
APP__SECURITY__HSTS_ENABLED=false
APP__SECURITY__FRAME_OPTIONS=deny
APP__SECURITY__REFERRER_POLICY=strict-origin-when-cross-origin
APP__SECURITY__X_CONTENT_TYPE_OPTIONS=true
APP__SECURITY__X_XSS_PROTECTION=true
APP__SECURITY__API_KEY_PREFIX=sp_
APP__SECURITY__AUDIT_LOG_RETENTION_DAYS=30

# Feature Flags - all enabled for dev
APP__FEATURES__RED_TEAM=true
APP__FEATURES__BLUE_TEAM=true
APP__FEATURES__PURPLE_TEAM=true
APP__FEATURES__AI_AGENT=true
APP__FEATURES__SIEM=true
APP__FEATURES__THREAT_INTEL=true
APP__FEATURES__COMPLIANCE=true
APP__FEATURES__EVIDENCE_VAULT=true
APP__FEATURES__SOCIAL_ENGINEERING=true
APP__FEATURES__WIRELESS=true
APP__FEATURES__MOBILE=true
APP__FEATURES__HARDWARE=true
APP__FEATURES__AIML=true
APP__FEATURES__EXPLOIT_DEV=true
APP__FEATURES__TRAINING=true
APP__FEATURES__MARKETPLACE=false
APP__FEATURES__MULTI_TENANT=true
APP__FEATURES__FEDERATION=false

# Encryption
APP__SECURITY__ENCRYPTION__ALGORITHM=AES-256-GCM
APP__SECURITY__ENCRYPTION__KEY_DERIVATION=argon2id
APP__SECURITY__ENCRYPTION__KEY_ROTATION_DAYS=90
APP__SECURITY__ENCRYPTION__ENVELOPE_ENCRYPTION=true
EOF
fi

# Install frontend dependencies
echo "📦 Installing frontend dependencies..."
cd packages/frontend
npm install --legacy-peer-deps 2>/dev/null || npm install

echo -e "${GREEN}✅ Native development setup complete!${NC}"
echo ""
echo "To start the backend:"
echo "  cd security-platform/packages/backend && cargo run --bin main"
echo ""
echo "To start the frontend:"
echo "  cd security-platform/packages/frontend && npm run dev"
echo ""
echo "Then open http://localhost:5173"
echo ""
echo "Backend API: http://localhost:8080"
echo "Health check: http://localhost:8081/health"
echo "Metrics: http://localhost:9090/metrics"