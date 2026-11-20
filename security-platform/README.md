# Nanomite Command Center

Unified Security Operations Platform for Blue, Red & Purple Teams with Nanomite AI Agent integration.

## Overview

The Nanomite Command Center is a comprehensive security operations platform that unifies:

- **🔵 Blue Team**: SIEM, threat hunting, incident response, vulnerability scanning, compliance
- **🔴 Red Team**: Penetration testing, adversary emulation, red team infrastructure, social engineering
- **🟣 Purple Team**: Detection engineering, attack simulation, coverage mapping, joint exercises

Powered by the **Nanomite AI Agent** for intelligent orchestration, autonomous planning, and persistent neural memory.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    NANOMITE COMMAND CENTER                      │
├─────────────┬─────────────┬─────────────┬─────────────┬────────┤
│   BLUE      │    RED      │   PURPLE    │   SHARED    │ AGENT  │
│ SIEM        │ Pentesting  │ Detection   │ OSINT       │ Nanomite│
│ Threat Intel│ Red Team    │ Attack Sim  │ SocMed      │ AI Core │
│ IR/Forensics│ Infrastructure│Coverage  │ Compliance  │ Memory  │
│ Vuln Scan   │ Social Eng  │ Joint Ex.  │ Training    │ Planning│
└─────────────┴─────────────┴─────────────┴─────────────┴────────┘
         │         │         │         │         │
         └─────────┴─────────┴─────────┴─────────┘
                   SHARED INFRASTRUCTURE
           PostgreSQL + pgvector + Redis + MinIO + Authentik
```

## Quick Start

### Prerequisites

- Docker & Docker Compose
- Node.js 20+
- Rust 1.78+

### 1. Start Infrastructure

```bash
cd security-platform/docker
docker-compose up -d
```

### 2. Run Database Migrations

```bash
cd security-platform
npm run db:migrate
```

### 3. Start Backend

```bash
cd security-platform/packages/backend
cargo run --bin main
```

### 4. Start Frontend

```bash
cd security-platform/packages/frontend
npm install
npm run dev
```

Open http://localhost:5173

## Module Catalog

| Team | Modules |
|------|---------|
| Blue | SIEM, Threat Intel, IR/Forensics, Vuln Scan, Cloud/Container, Supply Chain, API Security, DevSecOps, Compliance, Training |
| Red | Pentesting, Red Team Infra, Social Engineering, Wireless/RF, Mobile Security, Hardware/Firmware, Exploit Dev |
| Purple | Detection Engineering, Attack Simulation, Coverage Mapping, Joint Exercises |
| Shared | OSINT, SocMed & Doxxing, Asset Inventory, Evidence Vault |

## Nanomite AI Agent

The Nanomite AI Agent provides:

- **Chat Interface**: Natural language interaction with security tools
- **Autonomous Planning**: Generate and execute multi-step assessment plans
- **Neural Memory**: Persistent semantic memory across sessions
- **Skill Registry**: Dynamic tool capability discovery
- **Zero Refusals**: Authorized security operations always permitted

## Security Model

- **RBAC**: Blue/Red/Purple team roles with CISO/Auditor oversight
- **Authorization**: Red Team engagements require signed authorization documents
- **RLS**: PostgreSQL row-level security for multi-tenant isolation
- **Evidence Vault**: Append-only, signed chain of custody for all artifacts
- **Sandbox**: Rootless Podman containers for tool execution

## License

MIT License - see LICENSE file for details.
