-- SQLite migrations for local development
-- PostgreSQL-specific features (pgvector, RLS) are skipped

-- Enable foreign keys
PRAGMA foreign_keys = ON;

-- Organizations
CREATE TABLE organizations (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    settings TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Users
CREATE TABLE users (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'analyst',
    mfa_secret TEXT,
    last_login TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(org_id, email)
);

-- API Keys
CREATE TABLE api_keys (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL,
    scopes TEXT NOT NULL DEFAULT '[]',
    expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at TEXT
);

-- Projects
CREATE TABLE projects (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    scope TEXT NOT NULL DEFAULT '{}',
    settings TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Assets
CREATE TABLE assets (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    type TEXT NOT NULL,
    identifier TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    discovered_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(project_id, type, identifier)
);

-- Assessments
CREATE TABLE assessments (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    config TEXT NOT NULL DEFAULT '{}',
    created_by TEXT NOT NULL REFERENCES users(id),
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Tool Runs
CREATE TABLE tool_runs (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    assessment_id TEXT NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    command TEXT NOT NULL,
    args TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'pending',
    stdout TEXT,
    stderr TEXT,
    exit_code INTEGER,
    artifacts TEXT NOT NULL DEFAULT '[]',
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Findings
CREATE TABLE findings (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    assessment_id TEXT NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    asset_id TEXT REFERENCES assets(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    severity TEXT NOT NULL,
    cvss_vector TEXT,
    cvss_score REAL,
    epss_score REAL,
    kev_listed INTEGER NOT NULL DEFAULT 0,
    description TEXT NOT NULL,
    evidence TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'open',
    assignee_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Finding Evidence
CREATE TABLE finding_evidence (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    finding_id TEXT NOT NULL REFERENCES findings(id) ON DELETE CASCADE,
    type TEXT NOT NULL,
    content TEXT,
    hash TEXT NOT NULL,
    signature TEXT,
    collected_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Agent Conversations
CREATE TABLE agent_conversations (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    assessment_id TEXT REFERENCES assessments(id) ON DELETE SET NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    messages TEXT NOT NULL DEFAULT '[]',
    context TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Agent Plans
CREATE TABLE agent_plans (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    conversation_id TEXT NOT NULL REFERENCES agent_conversations(id) ON DELETE CASCADE,
    goal TEXT NOT NULL,
    steps TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'draft',
    approved_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    executed_at TEXT
);

-- Neural Memory (no vector index in SQLite)
CREATE TABLE neural_memory (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_neural_memory_org_id ON neural_memory(org_id);

-- Audit Logs
CREATE TABLE audit_logs (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    details TEXT NOT NULL DEFAULT '{}',
    ip TEXT,
    user_agent TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_audit_logs_org_id_created_at ON audit_logs(org_id, created_at DESC);
CREATE INDEX idx_audit_logs_user_id_created_at ON audit_logs(user_id, created_at DESC);

-- Compliance Frameworks
CREATE TABLE compliance_frameworks (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    controls TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Compliance Evidence
CREATE TABLE compliance_evidence (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    framework_id TEXT NOT NULL REFERENCES compliance_frameworks(id) ON DELETE CASCADE,
    control_id TEXT NOT NULL,
    finding_id TEXT REFERENCES findings(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    collected_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(framework_id, control_id, finding_id)
);

-- Updated_at trigger
CREATE TRIGGER update_organizations_updated_at 
    AFTER UPDATE ON organizations 
    BEGIN 
        UPDATE organizations SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER update_users_updated_at 
    AFTER UPDATE ON users 
    BEGIN 
        UPDATE users SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER update_projects_updated_at 
    AFTER UPDATE ON projects 
    BEGIN 
        UPDATE projects SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER update_assessments_updated_at 
    AFTER UPDATE ON assessments 
    BEGIN 
        UPDATE assessments SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER update_findings_updated_at 
    AFTER UPDATE ON findings 
    BEGIN 
        UPDATE findings SET updated_at = datetime('now') WHERE id = NEW.id;
    END;

CREATE TRIGGER update_agent_conversations_updated_at 
    AFTER UPDATE ON agent_conversations 
    BEGIN 
        UPDATE agent_conversations SET updated_at = datetime('now') WHERE id = NEW.id;
    END;