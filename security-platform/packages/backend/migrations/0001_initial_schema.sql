-- ============================================================================
-- NANOMITE COMMAND CENTER — Initial Database Schema
-- Blue Team / Red Team / Purple Team Support
-- ============================================================================

-- Enable extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";
-- pgvector not available locally; neural_memory uses TEXT embedding placeholder
-- CREATE EXTENSION IF NOT EXISTS "vector";

-- ============================================================================
-- Organizations (Multi-tenancy)
-- ============================================================================
CREATE TABLE organizations (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(100) NOT NULL UNIQUE,
    settings JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Users (with Blue/Red/Purple team roles)
-- ============================================================================
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email VARCHAR(255) NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    role VARCHAR(50) NOT NULL DEFAULT 'analyst',
    mfa_secret VARCHAR(255),
    last_login TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(org_id, email)
);

-- ============================================================================
-- API Keys
-- ============================================================================
CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    key_hash VARCHAR(255) NOT NULL,
    scopes JSONB NOT NULL DEFAULT '[]',
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ
);

-- ============================================================================
-- Projects
-- ============================================================================
CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    scope JSONB NOT NULL DEFAULT '{}',
    settings JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Assets
-- ============================================================================
CREATE TABLE assets (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    type VARCHAR(50) NOT NULL,
    identifier VARCHAR(500) NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    discovered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(project_id, type, identifier)
);

-- ============================================================================
-- Assessments (Blue/Red/Purple team support)
-- ============================================================================
CREATE TABLE assessments (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    type VARCHAR(50) NOT NULL,
    team VARCHAR(20) NOT NULL DEFAULT 'blue',
    status VARCHAR(50) NOT NULL DEFAULT 'draft',
    config JSONB NOT NULL DEFAULT '{}',
    scope_validation JSONB NOT NULL DEFAULT '{}',
    created_by UUID NOT NULL REFERENCES users(id),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Tool Runs
-- ============================================================================
CREATE TABLE tool_runs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    assessment_id UUID NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    tool_name VARCHAR(100) NOT NULL,
    module VARCHAR(50) NOT NULL,
    team VARCHAR(20) NOT NULL,
    command TEXT NOT NULL,
    args JSONB NOT NULL DEFAULT '[]',
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    stdout TEXT,
    stderr TEXT,
    exit_code INTEGER,
    artifacts JSONB NOT NULL DEFAULT '[]',
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Findings
-- ============================================================================
CREATE TABLE findings (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    assessment_id UUID NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    asset_id UUID REFERENCES assets(id) ON DELETE SET NULL,
    title VARCHAR(500) NOT NULL,
    severity VARCHAR(20) NOT NULL,
    cvss_vector VARCHAR(200),
    cvss_score DECIMAL(3,1),
    epss_score DECIMAL(4,3),
    kev_listed BOOLEAN NOT NULL DEFAULT FALSE,
    description TEXT NOT NULL,
    evidence JSONB NOT NULL DEFAULT '{}',
    status VARCHAR(50) NOT NULL DEFAULT 'open',
    assignee_id UUID REFERENCES users(id) ON DELETE SET NULL,
    team VARCHAR(20) NOT NULL,
    mitre_technique VARCHAR(100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Finding Evidence
-- ============================================================================
CREATE TABLE finding_evidence (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    finding_id UUID NOT NULL REFERENCES findings(id) ON DELETE CASCADE,
    type VARCHAR(50) NOT NULL,
    content TEXT,
    hash VARCHAR(64) NOT NULL,
    signature VARCHAR(512),
    collected_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Evidence Vault (Chain of Custody, Append-Only)
-- ============================================================================
CREATE TABLE evidence_vault (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    assessment_id UUID REFERENCES assessments(id) ON DELETE SET NULL,
    tool_run_id UUID REFERENCES tool_runs(id) ON DELETE SET NULL,
    content_type VARCHAR(100) NOT NULL,
    content_hash VARCHAR(64) NOT NULL,
    content_signature VARCHAR(512),
    storage_path VARCHAR(500) NOT NULL,
    merkle_leaf_hash VARCHAR(64),
    collected_by UUID NOT NULL REFERENCES users(id),
    team VARCHAR(20) NOT NULL,
    purpose VARCHAR(100) NOT NULL,
    retention_days INTEGER NOT NULL DEFAULT 2555,
    collected_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    verified_at TIMESTAMPTZ
);

-- ============================================================================
-- Agent Conversations (Nanomite AI)
-- ============================================================================
CREATE TABLE agent_conversations (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    assessment_id UUID REFERENCES assessments(id) ON DELETE SET NULL,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    messages JSONB NOT NULL DEFAULT '[]',
    context JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Agent Plans (Autonomous Planner)
-- ============================================================================
CREATE TABLE agent_plans (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    conversation_id UUID NOT NULL REFERENCES agent_conversations(id) ON DELETE CASCADE,
    goal TEXT NOT NULL,
    steps JSONB NOT NULL DEFAULT '[]',
    status VARCHAR(50) NOT NULL DEFAULT 'draft',
    approved_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    executed_at TIMESTAMPTZ
);

-- ============================================================================
-- Neural Memory (pgvector embeddings for Nanomite)
-- ============================================================================
CREATE TABLE neural_memory (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    embedding TEXT NOT NULL
    content TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX ON neural_memory (org_id);

-- ============================================================================
-- Audit Logs
-- ============================================================================
CREATE TABLE audit_logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    action VARCHAR(100) NOT NULL,
    resource_type VARCHAR(50),
    resource_id UUID,
    details JSONB NOT NULL DEFAULT '{}',
    ip INET,
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX ON audit_logs (org_id, created_at DESC);
CREATE INDEX ON audit_logs (user_id, created_at DESC);

-- ============================================================================
-- Compliance Frameworks & Evidence
-- ============================================================================
CREATE TABLE compliance_frameworks (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(100) NOT NULL,
    version VARCHAR(50) NOT NULL,
    controls JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE compliance_evidence (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    framework_id UUID NOT NULL REFERENCES compliance_frameworks(id) ON DELETE CASCADE,
    control_id VARCHAR(100) NOT NULL,
    finding_id UUID REFERENCES findings(id) ON DELETE SET NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    collected_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(framework_id, control_id, finding_id)
);

-- ============================================================================
-- SIEM Tables (Blue Team)
-- ============================================================================

-- Log sources
CREATE TABLE siem_log_sources (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    source_type VARCHAR(100) NOT NULL, -- syslog, winlogbeat, fluentd, vector, otel
    config JSONB NOT NULL DEFAULT '{}',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    eps_rate DECIMAL(10,2) DEFAULT 0,
    last_seen TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Parsed log events (partitioned by time in production)
CREATE TABLE siem_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    log_source_id UUID NOT NULL REFERENCES siem_log_sources(id) ON DELETE CASCADE,
    event_time TIMESTAMPTZ NOT NULL,
    raw_event JSONB NOT NULL,
    parsed_fields JSONB NOT NULL DEFAULT '{}',
    hostname VARCHAR(255),
    source_ip INET,
    dest_ip INET,
    username VARCHAR(255),
    process VARCHAR(255),
    mitre_tactic VARCHAR(100),
    mitre_technique VARCHAR(100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX ON siem_events (log_source_id, event_time DESC);
CREATE INDEX ON siem_events (mitre_tactic, mitre_technique);
CREATE INDEX ON siem_events (source_ip);
CREATE INDEX ON siem_events (dest_ip);

-- Sigma rules
CREATE TABLE siem_sigma_rules (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    title VARCHAR(500) NOT NULL,
    description TEXT,
    rule_yaml TEXT NOT NULL,
    level VARCHAR(50), -- low, medium, high, critical
    status VARCHAR(50) NOT NULL DEFAULT 'draft', -- draft, testing, production, deprecated
    mitre_technique VARCHAR(100),
    false_positive_rate DECIMAL(5,2),
    last_tested TIMESTAMPTZ,
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Alerts
CREATE TABLE siem_alerts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    rule_id UUID REFERENCES siem_sigma_rules(id) ON DELETE SET NULL,
    event_id UUID REFERENCES siem_events(id) ON DELETE SET NULL,
    title VARCHAR(500) NOT NULL,
    severity VARCHAR(20) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'new', -- new, acknowledged, investigating, resolved, false_positive
    mitre_tactic VARCHAR(100),
    mitre_technique VARCHAR(100),
    assigned_to UUID REFERENCES users(id),
    enriched BOOLEAN NOT NULL DEFAULT FALSE,
    enrichment_data JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ
);
CREATE INDEX ON siem_alerts (org_id, status, created_at DESC);
CREATE INDEX ON siem_alerts (severity);
CREATE INDEX ON siem_alerts (mitre_technique);

-- Threat Intelligence IOCs
CREATE TABLE threat_intel_iocs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    ioc_type VARCHAR(50) NOT NULL, -- ip, domain, url, hash, email, filename
    ioc_value TEXT NOT NULL,
    source VARCHAR(100) NOT NULL,
    confidence INTEGER DEFAULT 50,
    severity VARCHAR(20),
    tags JSONB NOT NULL DEFAULT '[]',
    first_seen TIMESTAMPTZ,
    last_seen TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(ioc_type, ioc_value, source)
);
CREATE INDEX ON threat_intel_iocs (ioc_type, ioc_value);

-- ============================================================================
-- Threat Hunting (Blue Team)
-- ============================================================================
CREATE TABLE hunt_queries (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    query_text TEXT NOT NULL,
    query_type VARCHAR(50) NOT NULL, -- sql, lucene, kql, sigma
    mitre_technique VARCHAR(100),
    frequency VARCHAR(50), -- hourly, daily, weekly, on_demand
    last_run TIMESTAMPTZ,
    results_count INTEGER DEFAULT 0,
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE hunt_results (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    hunt_query_id UUID NOT NULL REFERENCES hunt_queries(id) ON DELETE CASCADE,
    event_id UUID NOT NULL REFERENCES siem_events(id) ON DELETE CASCADE,
    match_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Red Team Infrastructure
-- ============================================================================
CREATE TABLE redteam_c2_servers (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    assessment_id UUID REFERENCES assessments(id) ON DELETE SET NULL,
    name VARCHAR(255) NOT NULL,
    c2_framework VARCHAR(100) NOT NULL, -- sliver, havoc, mythic, cobalt_strike, bruteratel
    ip_address INET NOT NULL,
    port INTEGER NOT NULL,
    domain VARCHAR(255),
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    redirector_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    opsec_level VARCHAR(50) NOT NULL DEFAULT 'standard',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    destroyed_at TIMESTAMPTZ
);

CREATE TABLE redteam_payloads (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    assessment_id UUID REFERENCES assessments(id) ON DELETE SET NULL,
    name VARCHAR(255) NOT NULL,
    payload_type VARCHAR(100) NOT NULL, -- stageless, staged, dll, exe, shellcode, macro
    format VARCHAR(50) NOT NULL,
    hash VARCHAR(64) NOT NULL,
    storage_path VARCHAR(500) NOT NULL,
    opsec_notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Social Engineering & SocMed
-- ============================================================================
CREATE TABLE socmed_investigations (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    assessment_id UUID REFERENCES assessments(id) ON DELETE SET NULL,
    target_type VARCHAR(50) NOT NULL, -- username, email, phone, name, domain
    target_value TEXT NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    findings_count INTEGER DEFAULT 0,
    risk_score INTEGER DEFAULT 0,
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE socmed_profiles (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    investigation_id UUID NOT NULL REFERENCES socmed_investigations(id) ON DELETE CASCADE,
    platform VARCHAR(100) NOT NULL, -- linkedin, twitter, facebook, instagram, github, etc.
    username VARCHAR(255) NOT NULL,
    profile_url TEXT,
    data JSONB NOT NULL DEFAULT '{}',
    confidence_score INTEGER DEFAULT 0,
    scraped_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(investigation_id, platform, username)
);

-- ============================================================================
-- Vulnerability Scanning (Blue Team)
-- ============================================================================
CREATE TABLE vuln_scan_templates (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    scanner_type VARCHAR(50) NOT NULL, -- nuclei, openvas, trivy, checkov, zap, burp
    template_content TEXT NOT NULL,
    targets JSONB NOT NULL DEFAULT '[]',
    schedule VARCHAR(100), -- cron expression
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Cloud & Container Security
-- ============================================================================
CREATE TABLE cloud_assessments (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    provider VARCHAR(50) NOT NULL, -- aws, azure, gcp
    account_id VARCHAR(255),
    region VARCHAR(100),
    resource_type VARCHAR(100),
    resource_id VARCHAR(500),
    check_id VARCHAR(100),
    check_title VARCHAR(500),
    severity VARCHAR(20),
    status VARCHAR(50), -- pass, fail, warning, info
    remediation TEXT,
    scanned_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Supply Chain & SBOM
-- ============================================================================
CREATE TABLE sbom_entries (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    component_name VARCHAR(255) NOT NULL,
    component_version VARCHAR(100) NOT NULL,
    package_manager VARCHAR(50), -- npm, pypi, maven, go, cargo, docker
    license VARCHAR(100),
    sbom_format VARCHAR(50), -- cyclonedx, spdx
    vulnerabilities JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Detection Engineering (Purple Team)
-- ============================================================================
CREATE TABLE detection_rules (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name VARCHAR(500) NOT NULL,
    rule_type VARCHAR(50) NOT NULL, -- sigma, yaral, kql, splunk
    rule_content TEXT NOT NULL,
    mitre_technique VARCHAR(100),
    status VARCHAR(50) NOT NULL DEFAULT 'draft',
    coverage_percentage DECIMAL(5,2) DEFAULT 0,
    test_results JSONB NOT NULL DEFAULT '[]',
    last_tested TIMESTAMPTZ,
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Attack Simulation (Purple Team)
-- ============================================================================
CREATE TABLE attack_simulations (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    technique_id VARCHAR(100), -- MITRE ATT&CK technique
    status VARCHAR(50) NOT NULL DEFAULT 'planned',
    start_time TIMESTAMPTZ,
    end_time TIMESTAMPTZ,
    detection_triggered BOOLEAN NOT NULL DEFAULT FALSE,
    detection_delay_seconds INTEGER,
    results JSONB NOT NULL DEFAULT '[]',
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Updated At Triggers
-- ============================================================================
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_organizations_updated_at BEFORE UPDATE ON organizations FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_users_updated_at BEFORE UPDATE ON users FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_projects_updated_at BEFORE UPDATE ON projects FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_assessments_updated_at BEFORE UPDATE ON assessments FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_findings_updated_at BEFORE UPDATE ON findings FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_agent_conversations_updated_at BEFORE UPDATE ON agent_conversations FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_siem_sigma_rules_updated_at BEFORE UPDATE ON siem_sigma_rules FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_siem_alerts_updated_at BEFORE UPDATE ON siem_alerts FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_socmed_investigations_updated_at BEFORE UPDATE ON socmed_investigations FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_vuln_scan_templates_updated_at BEFORE UPDATE ON vuln_scan_templates FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_detection_rules_updated_at BEFORE UPDATE ON detection_rules FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_sbom_entries_updated_at BEFORE UPDATE ON sbom_entries FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- ============================================================================
-- Row Level Security (Multi-tenant isolation)
-- ============================================================================
ALTER TABLE organizations ENABLE ROW LEVEL SECURITY;
ALTER TABLE users ENABLE ROW LEVEL SECURITY;
ALTER TABLE projects ENABLE ROW LEVEL SECURITY;
ALTER TABLE assets ENABLE ROW LEVEL SECURITY;
ALTER TABLE assessments ENABLE ROW LEVEL SECURITY;
ALTER TABLE findings ENABLE ROW LEVEL SECURITY;
ALTER TABLE tool_runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_conversations ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_plans ENABLE ROW LEVEL SECURITY;
ALTER TABLE neural_memory ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_logs ENABLE ROW LEVEL SECURITY;
ALTER TABLE siem_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE siem_alerts ENABLE ROW LEVEL SECURITY;
ALTER TABLE threat_intel_iocs ENABLE ROW LEVEL SECURITY;
ALTER TABLE evidence_vault ENABLE ROW LEVEL SECURITY;
ALTER TABLE socmed_investigations ENABLE ROW LEVEL SECURITY;
ALTER TABLE cloud_assessments ENABLE ROW LEVEL SECURITY;
ALTER TABLE sbom_entries ENABLE ROW LEVEL SECURITY;
ALTER TABLE detection_rules ENABLE ROW LEVEL SECURITY;
ALTER TABLE attack_simulations ENABLE ROW LEVEL SECURITY;

-- RLS Policies (users can only access their org's data)
CREATE POLICY org_isolation ON organizations
    FOR ALL TO PUBLIC
    USING (true);

CREATE POLICY user_org_isolation ON users
    FOR ALL
    USING (org_id = current_setting('app.current_org_id', true)::uuid);

CREATE POLICY project_org_isolation ON projects
    FOR ALL
    USING (org_id = current_setting('app.current_org_id', true)::uuid);

CREATE POLICY asset_org_isolation ON assets
    FOR ALL
    USING (
        project_id IN (
            SELECT id FROM projects WHERE org_id = current_setting('app.current_org_id', true)::uuid
        )
    );

CREATE POLICY assessment_org_isolation ON assessments
    FOR ALL
    USING (
        project_id IN (
            SELECT id FROM projects WHERE org_id = current_setting('app.current_org_id', true)::uuid
        )
    );

-- ============================================================================
-- End of Schema
-- ============================================================================