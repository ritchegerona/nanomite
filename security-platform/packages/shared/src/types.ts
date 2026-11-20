// Shared TypeScript types for Security Platform

export type UserRole = 'admin' | 'analyst' | 'viewer' | 'operator' | 'lead';
export type TeamType = 'blue' | 'red' | 'purple';
export type TeamRole =
  | 'blue_analyst'
  | 'blue_lead'
  | 'red_operator'
  | 'red_lead'
  | 'purple_engineer'
  | 'ciso'
  | 'auditor';

export type AssessmentType =
  | 'osint'
  | 'pentest'
  | 'vulnscan'
  | 'siem'
  | 'red_team'
  | 'comprehensive'
  | 'socmed';

export type AssessmentStatus = 'draft' | 'running' | 'paused' | 'completed' | 'failed' | 'cancelled';

export type FindingSeverity = 'critical' | 'high' | 'medium' | 'low' | 'info';
export type FindingStatus = 'open' | 'triaged' | 'in_progress' | 'fixed' | 'false_positive' | 'accepted_risk';

export type ToolRunStatus = 'pending' | 'running' | 'completed' | 'failed' | 'timeout' | 'cancelled';

export type AgentMessageType =
  | 'text'
  | 'thinking'
  | 'tool_call'
  | 'tool_result'
  | 'finding'
  | 'plan_step'
  | 'complete'
  | 'error';

export type ModuleType =
  | 'osint'
  | 'socmed'
  | 'siem'
  | 'pentesting'
  | 'vulnerability_scanning'
  | 'red_team_infra'
  | 'social_engineering'
  | 'wireless'
  | 'mobile_security'
  | 'hardware_firmware'
  | 'exploit_development'
  | 'attack_simulation'
  | 'adversary_emulation'
  | 'coverage_mapping'
  | 'joint_exercises'
  | 'cloud_container'
  | 'supply_chain'
  | 'api_security'
  | 'compliance'
  | 'asset_inventory'
  | 'detection_engineering'
  | 'shared';

export interface PaginationParams {
  page: number;
  limit: number;
  sort?: string;
  order: 'asc' | 'desc';
}

export interface PaginatedResponse<T> {
  items: T[];
  total: number;
  page: number;
  limit: number;
  total_pages: number;
}

export interface ApiResponse<T> {
  success: boolean;
  data?: T;
  error?: string;
  meta?: {
    timestamp: string;
    request_id: string;
  };
}

export interface Organization {
  id: string;
  name: string;
  slug: string;
  settings: Record<string, any>;
  created_at: string;
  updated_at: string;
}

export interface User {
  id: string;
  org_id: string;
  email: string;
  role: UserRole;
  team_role?: TeamRole;
  mfa_secret?: string;
  last_login?: string;
  created_at: string;
  updated_at: string;
}

export interface UserResponse {
  id: string;
  org_id: string;
  email: string;
  role: UserRole;
  last_login?: string;
  created_at: string;
}

export interface Project {
  id: string;
  org_id: string;
  name: string;
  description?: string;
  scope: Record<string, any>;
  settings: Record<string, any>;
  created_at: string;
  updated_at: string;
}

export interface Asset {
  id: string;
  project_id: string;
  type: string;
  identifier: string;
  metadata: Record<string, any>;
  discovered_at: string;
  last_seen_at: string;
}

export interface Assessment {
  id: string;
  project_id: string;
  name: string;
  type: AssessmentType;
  team: TeamType;
  status: AssessmentStatus;
  config: Record<string, any>;
  scope_validation: Record<string, any>;
  created_by: string;
  started_at?: string;
  completed_at?: string;
  created_at: string;
  updated_at: string;
}

export interface ToolRun {
  id: string;
  assessment_id: string;
  tool_name: string;
  module: string;
  team: TeamType;
  command: string;
  args: Record<string, any>;
  status: ToolRunStatus;
  stdout?: string;
  stderr?: string;
  exit_code?: number;
  artifacts: Array<{
    path: string;
    type: string;
    hash: string;
    size: number;
  }>;
  started_at?: string;
  completed_at?: string;
  created_at: string;
}

export interface Finding {
  id: string;
  assessment_id: string;
  asset_id?: string;
  title: string;
  severity: FindingSeverity;
  cvss_vector?: string;
  cvss_score?: number;
  epss_score?: number;
  kev_listed: boolean;
  description: string;
  evidence: Record<string, any>;
  status: FindingStatus;
  assignee_id?: string;
  team: TeamType;
  mitre_technique?: string;
  created_at: string;
  updated_at: string;
}

export interface AgentMessage {
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  timestamp?: string;
  type?: AgentMessageType;
  data?: Record<string, any>;
}

export interface AgentConversation {
  id: string;
  assessment_id?: string;
  user_id: string;
  messages: AgentMessage[];
  context: Record<string, any>;
  created_at: string;
  updated_at: string;
}

export interface AgentPlanStep {
  id: string;
  description: string;
  tool: string;
  args: Record<string, any>;
  status: 'pending' | 'running' | 'completed' | 'failed' | 'skipped';
  result?: any;
  approved: boolean;
}

export interface AgentPlan {
  id: string;
  conversation_id: string;
  goal: string;
  steps: AgentPlanStep[];
  status: 'draft' | 'pending_approval' | 'approved' | 'executing' | 'completed' | 'failed';
  approved_by?: string;
  created_at: string;
  executed_at?: string;
}

export interface SigmaRule {
  id: string;
  org_id: string;
  title: string;
  description?: string;
  rule_yaml: string;
  level?: 'low' | 'medium' | 'high' | 'critical';
  status: 'draft' | 'testing' | 'production' | 'deprecated';
  mitre_technique?: string;
  created_by: string;
  created_at: string;
  updated_at: string;
}

export interface SiemAlert {
  id: string;
  org_id: string;
  rule_id?: string;
  event_id?: string;
  title: string;
  severity: 'low' | 'medium' | 'high' | 'critical';
  status: 'new' | 'acknowledged' | 'investigating' | 'resolved' | 'false_positive';
  mitre_tactic?: string;
  mitre_technique?: string;
  assigned_to?: string;
  enriched: boolean;
  enrichment_data: Record<string, any>;
  created_at: string;
  updated_at: string;
  resolved_at?: string;
}

export interface ThreatIntelIOC {
  id: string;
  org_id: string;
  ioc_type: 'ip' | 'domain' | 'url' | 'hash' | 'email' | 'filename';
  ioc_value: string;
  source: string;
  confidence: number;
  severity?: 'low' | 'medium' | 'high' | 'critical';
  tags: string[];
  first_seen?: string;
  last_seen?: string;
  expires_at?: string;
  created_at: string;
}

export interface SocMedInvestigation {
  id: string;
  org_id: string;
  assessment_id?: string;
  target_type: 'username' | 'email' | 'phone' | 'name' | 'domain';
  target_value: string;
  status: string;
  findings_count: number;
  risk_score: number;
  created_by: string;
  created_at: string;
  updated_at: string;
}

export interface SocMedProfile {
  id: string;
  investigation_id: string;
  platform: string;
  username: string;
  profile_url?: string;
  data: Record<string, any>;
  confidence_score: number;
  scraped_at: string;
}

export interface VulnScanTemplate {
  id: string;
  org_id: string;
  name: string;
  scanner_type: 'nuclei' | 'openvas' | 'trivy' | 'checkov' | 'zap' | 'burp';
  template_content: string;
  targets: string[];
  schedule?: string;
  enabled: boolean;
  created_by: string;
  created_at: string;
  updated_at: string;
}

export interface CloudAssessment {
  id: string;
  org_id: string;
  provider: 'aws' | 'azure' | 'gcp';
  account_id?: string;
  region?: string;
  resource_type?: string;
  resource_id?: string;
  check_id?: string;
  check_title?: string;
  severity?: 'critical' | 'high' | 'medium' | 'low' | 'info';
  status?: 'pass' | 'fail' | 'warning' | 'info';
  remediation?: string;
  scanned_at: string;
}

export interface SBOMEntry {
  id: string;
  org_id: string;
  project_id?: string;
  component_name: string;
  component_version: string;
  package_manager?: string;
  license?: string;
  sbom_format?: 'cyclonedx' | 'spdx';
  vulnerabilities: any[];
  created_at: string;
  updated_at: string;
}

export interface DetectionRule {
  id: string;
  org_id: string;
  name: string;
  rule_type: 'sigma' | 'yaral' | 'kql' | 'splunk';
  rule_content: string;
  mitre_technique?: string;
  status: 'draft' | 'testing' | 'production' | 'deprecated';
  coverage_percentage: number;
  test_results: any[];
  last_tested?: string;
  created_by: string;
  created_at: string;
  updated_at: string;
}

export interface AttackSimulation {
  id: string;
  org_id: string;
  name: string;
  technique_id?: string;
  status: 'planned' | 'running' | 'completed' | 'failed';
  start_time?: string;
  end_time?: string;
  detection_triggered: boolean;
  detection_delay_seconds?: number;
  results: any[];
  created_by: string;
  created_at: string;
}

export interface C2Server {
  id: string;
  org_id: string;
  assessment_id?: string;
  name: string;
  c2_framework: 'sliver' | 'havoc' | 'mythic' | 'cobalt_strike' | 'bruteratel';
  ip_address: string;
  port: number;
  domain?: string;
  status: string;
  redirector_enabled: boolean;
  opsec_level: 'standard' | 'high' | 'paranoid';
  created_at: string;
  destroyed_at?: string;
}

export interface EvidenceVaultItem {
  id: string;
  org_id: string;
  assessment_id?: string;
  tool_run_id?: string;
  content_type: string;
  content_hash: string;
  content_signature?: string;
  storage_path: string;
  merkle_leaf_hash?: string;
  collected_by: string;
  team: TeamType;
  purpose: string;
  retention_days: number;
  collected_at: string;
  verified_at?: string;
}

export interface NeuralMemorySearchResult {
  id: string;
  content: string;
  score: number;
  metadata: Record<string, any>;
}

export interface NanomiteChatContext {
  assessment_id?: string;
  project_id?: string;
  selected_findings: string[];
  selected_assets: string[];
  mode: 'chat' | 'plan' | 'execute' | 'analyze';
}

export interface NanomiteChatEvent {
  type: 'thinking' | 'tool_call' | 'tool_result' | 'finding' | 'plan_step' | 'complete' | 'error';
  payload: any;
}