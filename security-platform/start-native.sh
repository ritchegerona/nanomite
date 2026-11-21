#!/bin/bash
# Start Nanomite Command Center - Native (no Docker)

set -e

echo "🚀 Starting Nanomite Command Center - Native Mode"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Create data directories
mkdir -p data/storage
mkdir -p data/nanomite/{models,skills,memory}
mkdir -p logs

# Start Redis if available
if command -v redis-server &> /dev/null; then
    if ! redis-cli ping &> /dev/null; then
        echo -e "${YELLOW}Starting Redis...${NC}"
        redis-server --daemonize yes --dir ./data/redis 2>/dev/null || true
        sleep 1
    fi
fi

# Function to cleanup on exit
cleanup() {
    echo -e "\n${YELLOW}Shutting down...${NC}"
    kill $BACKEND_PID $FRONTEND_PID 2>/dev/null || true
    exit 0
}
trap cleanup INT TERM

# Start backend with env file
echo -e "${BLUE}Starting backend API on port 8080...${NC}"
cd packages/backend
if [ -f ../.env.local ]; then
    export $(cat ../.env.local | grep -v '^#' | xargs)
fi
RUST_LOG=info cargo run --bin main &
BACKEND_PID=$!
cd ../..

# Wait for backend to be ready
echo -e "${YELLOW}Waiting for backend to start...${NC}"
for i in {1..30}; do
    if curl -s http://localhost:8081/health > /dev/null 2>&1; then
        echo -e "${GREEN}✓ Backend ready${NC}"
        break
    fi
    sleep 1
done

# Start frontend
echo -e "${BLUE}Starting frontend on port 5173...${NC}"
cd packages/frontend
npm run dev -- --host 0.0.0.0 --port 5173 &
FRONTEND_PID=$!
cd ../..

echo -e "${GREEN}✅ Nanomite Command Center is running!${NC}"
echo ""
echo "┌─────────────────────────────────────────────┐"
echo "│  🎯 NANOMITE COMMAND CENTER                 │"
echo "├─────────────────────────────────────────────┤"
echo "│  Frontend:  http://localhost:5173           │"
echo "│  Backend:   http://localhost:8080           │"
echo "│  Health:    http://localhost:8081/health    │"
echo "│  Metrics:   http://localhost:9090/metrics   │"
echo "├─────────────────────────────────────────────┤"
echo "│  Default Login:                             │"
echo "│  Email:    admin@nanomite.local             │"
echo "│  Password: nanomite123                      │"
echo "└─────────────────────────────────────────────┘"
echo ""
echo "Press Ctrl+C to stop all services"

wait