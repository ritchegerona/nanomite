# ADR 0001: Nanomite AI Agent Integration Pattern

## Status
Proposed

## Context
The Nanomite Command Center needs an AI agent to orchestrate security tools across Blue, Red, and Purple teams. The agent must have zero-refusal behavior for authorized security operations, persistent memory, and tool orchestration capabilities.

## Decision
Embed `nanomite-core` as a Rust library directly in the backend API service, exposing chat/plan/execute endpoints via SSE/WebSocket streaming.

## Consequences

### Positive
- Zero latency between agent and tool orchestration
- Shared memory space with backend state
- Single binary deployment
- Direct access to all Rust crates and modules

### Negative
- Backend must be Rust (already decided)
- Nanomite updates require backend rebuild
- Memory usage increases with embedded agent

## Alternatives Considered
- **Sidecar API**: Higher latency, separate deployment, but language-agnostic
- **WASM Plugin**: Sandboxed but limited system access for security tools
- **External Service**: Decoupled but introduces network failure points

---

# ADR 0002: Role-Based Access Control for Blue/Red/Purple Teams

## Status
Proposed

## Context
The platform serves three distinct teams with different access requirements:
- Blue Team: Defense, detection, SIEM, vulnerability management
- Red Team: Offense, penetration testing, adversary emulation
- Purple Team: Collaboration, detection engineering, validation

## Decision
Implement module-level RBAC with TeamRole enum and can_access_module() function. CISO and Auditor roles have read-all access. Team-specific modules enforce write access only for appropriate team members.

## Consequences

### Positive
- Clear separation of duties
- Prevents Red Team from accessing Blue Team detection rules
- Purple Team can read both sides for collaboration
- Audit trail for all access decisions

### Negative
- Complex permission matrix to maintain
- Requires careful onboarding of new team members

---

# ADR 0003: Database Schema with pgvector for Neural Memory

## Status
Proposed

## Context
Nanomite AI agent requires semantic memory storage with vector similarity search for context-aware conversations across sessions.

## Decision
Use PostgreSQL 16 with pgvector extension. Store embeddings as VECTOR(1536) with HNSW index for cosine similarity search.

## Consequences

### Positive
- Single database for all data (ACID compliance)
- Vector search integrated with relational queries
- Mature ecosystem and tooling
- Row-level security for multi-tenant isolation

### Negative
- pgvector requires PostgreSQL (not MySQL/SQLite)
- Vector index size grows with memory entries

---

# ADR 0004: Sandboxed Tool Execution with Podman

## Status
Proposed

## Context
Security tools (nmap, metasploit, hashcat, etc.) must run in isolated environments to prevent host compromise and limit blast radius.

## Decision
Use rootless Podman containers with --read-only filesystem, --cap-drop ALL, --security-opt no-new-privileges, resource limits (memory, CPU, pids), and no network by default.

## Consequences

### Positive
- Strong isolation from host
- Resource limits prevent DoS
- OCI-compliant container images
- Rootless operation for security

### Negative
- Container startup latency (~1-3s)
- Some tools require network access (must be explicitly enabled)
- Image management overhead

---

# ADR 0005: Real-Time Communication via WebSocket + SSE

## Status
Proposed

## Context
The Command Center needs real-time updates for tool execution streams, agent reasoning, SIEM alerts, and assessment status changes.

## Decision
Use WebSocket for bidirectional agent chat and SSE for server-sent tool execution streams and alerts. Redis pub/sub for horizontal scaling.

## Consequences

### Positive
- Low latency real-time updates
- SSE works over standard HTTP (easier firewall traversal)
- WebSocket for interactive agent conversations
- Redis pub/sub enables horizontal scaling

### Negative
- Two protocols to manage
- Connection state management across restarts

---

# ADR 0006: Multi-Tenant Row-Level Security

## Status
Proposed

## Context
Organizations must be isolated from each other. Data leakage between tenants is unacceptable.

## Decision
Enable PostgreSQL Row-Level Security on all tables. Set `app.current_org_id` session variable per request. Policies restrict access to org-scoped data.

## Consequences

### Positive
- Database-enforced isolation (not application-level)
- Cannot accidentally query another org's data
- Transparent to application code

### Negative
- Must set session variable on every connection
- Complex queries may need SECURITY DEFINER functions
- Superusers bypass RLS (must restrict superuser access)

---

# ADR 0007: Evidence Vault with Chain of Custody

## Status
Proposed

## Context
Both Red and Blue teams produce evidence that must be admissible in legal/compliance contexts. Evidence must be tamper-evident and attributable.

## Decision
Append-only evidence vault with SHA256 hashes, cosign signatures, Merkle tree leaf hashes, and retention policies. Every artifact is signed at collection time.

## Consequences

### Positive
- Tamper-evident evidence chain
- Cosign integration for keyless signing
- Merkle proofs for transparency
- Retention policy enforcement

### Negative
- Append-only means no edits (even for errors)
- Storage costs grow over time
- Signature verification overhead
