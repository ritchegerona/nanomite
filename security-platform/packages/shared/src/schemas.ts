// Zod schemas for API validation
import { z } from 'zod';

// Common
export const UUIDSchema = z.string().uuid();
export const PaginationSchema = z.object({
  page: z.coerce.number().min(1).default(1),
  limit: z.coerce.number().min(1).max(100).default(20),
  sort: z.string().optional(),
  order: z.enum(['asc', 'desc']).default('desc'),
});

// Auth
export const LoginSchema = z.object({
  email: z.string().email(),
  password: z.string().min(8),
});

export const RegisterSchema = z.object({
  email: z.string().email(),
  password: z.string().min(12),
  org_name: z.string().min(1),
  org_slug: z.string().min(1),
});

// Organization
export const CreateOrganizationSchema = z.object({
  name: z.string().min(1),
  slug: z.string().min(1),
  settings: z.record(z.any()).optional().default({}),
});

// User
export const CreateUserSchema = z.object({
  org_id: UUIDSchema,
  email: z.string().email(),
  password: z.string().min(12),
  role: z.enum(['admin', 'analyst', 'viewer', 'operator', 'lead']).default('analyst'),
  mfa_secret: z.string().optional(),
});

// Project
export const CreateProjectSchema = z.object({
  org_id: UUIDSchema,
  name: z.string().min(1),
  description: z.string().optional(),
  scope: z.record(z.any()).optional().default({}),
  settings: z.record(z.any()).optional().default({}),
});

// Asset
export const CreateAssetSchema = z.object({
  project_id: UUIDSchema,
  type: z.enum(['domain', 'ip', 'cidr', 'cloud_resource', 'container', 'host']),
  identifier: z.string().min(1),
  metadata: z.record(z.any()).optional().default({}),
});

// Assessment
export const AssessmentTypeSchema = z.enum([
  'osint', 'pentest', 'vulnscan', 'siem', 'red_team', 'comprehensive', 'socmed',
]);

export const AssessmentTeamSchema = z.enum(['blue', 'red', 'purple']);

export const CreateAssessmentSchema = z.object({
  project_id: UUIDSchema,
  name: z.string().min(1),
  type: AssessmentTypeSchema,
  team: AssessmentTeamSchema,
  config: z.record(z.any()).optional().default({}),
  scope_validation: z.record(z.any()).optional().default({}),
});

// Tool Run
export const CreateToolRunSchema = z.object({
  assessment_id: UUIDSchema,
  tool_name: z.string().min(1),
  module: z.string().min(1),
  team: z.enum(['blue', 'red', 'purple']),
  command: z.string().min(1),
  args: z.record(z.any()).optional().default({}),
});

// Finding
export const CreateFindingSchema = z.object({
  assessment_id: UUIDSchema,
  asset_id: UUIDSchema.optional(),
  title: z.string().min(1),
  severity: z.enum(['critical', 'high', 'medium', 'low', 'info']),
  cvss_vector: z.string().optional(),
  cvss_score: z.number().min(0).max(10).optional(),
  epss_score: z.number().min(0).max(1).optional(),
  kev_listed: z.boolean().optional().default(false),
  description: z.string().min(1),
  evidence: z.record(z.any()).optional().default({}),
  team: z.enum(['blue', 'red', 'purple']),
  mitre_technique: z.string().optional(),
});

// Agent Conversation
export const AgentMessageSchema = z.object({
  role: z.enum(['user', 'assistant', 'system', 'tool']),
  content: z.string(),
  timestamp: z.string().datetime().optional(),
  type: z.enum(['text', 'thinking', 'tool_call', 'tool_result', 'finding', 'plan_step', 'complete', 'error']).optional(),
  data: z.record(z.any()).optional(),
});

export const CreateAgentConversationSchema = z.object({
  assessment_id: UUIDSchema.optional(),
  user_id: UUIDSchema,
  messages: z.array(AgentMessageSchema).optional().default([]),
  context: z.record(z.any()).optional().default({}),
});

// Agent Plan
export const CreateAgentPlanSchema = z.object({
  conversation_id: UUIDSchema,
  goal: z.string().min(1),
  steps: z.array(z.any()).optional().default([]),
});

// Nanomite Chat
export const NanomiteChatSchema = z.object({
  message: z.string().min(1),
  context: z.object({
    assessment_id: UUIDSchema.optional(),
    project_id: UUIDSchema.optional(),
    selected_findings: z.array(UUIDSchema).optional().default([]),
    selected_assets: z.array(UUIDSchema).optional().default([]),
    mode: z.enum(['chat', 'plan', 'execute', 'analyze']).default('chat'),
  }),
});

// SIEM - Sigma Rule
export const CreateSigmaRuleSchema = z.object({
  org_id: UUIDSchema,
  title: z.string().min(1),
  description: z.string().optional(),
  rule_yaml: z.string().min(1),
  level: z.enum(['low', 'medium', 'high', 'critical']).optional(),
  status: z.enum(['draft', 'testing', 'production', 'deprecated']).default('draft'),
  mitre_technique: z.string().optional(),
  created_by: UUIDSchema,
});

// SIEM - Alert
export const CreateSiemAlertSchema = z.object({
  org_id: UUIDSchema,
  rule_id: UUIDSchema.optional(),
  event_id: UUIDSchema.optional(),
  title: z.string().min(1),
  severity: z.enum(['low', 'medium', 'high', 'critical']),
  status: z.enum(['new', 'acknowledged', 'investigating', 'resolved', 'false_positive']).default('new'),
  mitre_tactic: z.string().optional(),
  mitre_technique: z.string().optional(),
  assigned_to: UUIDSchema.optional(),
});

// Threat Intel IOC
export const CreateThreatIntelIOCSchema = z.object({
  org_id: UUIDSchema,
  ioc_type: z.enum(['ip', 'domain', 'url', 'hash', 'email', 'filename']),
  ioc_value: z.string().min(1),
  source: z.string().min(1),
  confidence: z.number().min(0).max(100).default(50),
  severity: z.enum(['low', 'medium', 'high', 'critical']).optional(),
  tags: z.array(z.string()).optional().default([]),
  expires_at: z.string().datetime().optional(),
});

// SocMed Investigation
export const CreateSocMedInvestigationSchema = z.object({
  org_id: UUIDSchema,
  assessment_id: UUIDSchema.optional(),
  target_type: z.enum(['username', 'email', 'phone', 'name', 'domain']),
  target_value: z.string().min(1),
  created_by: UUIDSchema,
});

// Vulnerability Scan Template
export const CreateVulnScanTemplateSchema = z.object({
  org_id: UUIDSchema,
  name: z.string().min(1),
  scanner_type: z.enum(['nuclei', 'openvas', 'trivy', 'checkov', 'zap', 'burp']),
  template_content: z.string().min(1),
  targets: z.array(z.string()).optional().default([]),
  schedule: z.string().optional(),
  enabled: z.boolean().optional().default(true),
  created_by: UUIDSchema,
});

// Cloud Assessment
export const CreateCloudAssessmentSchema = z.object({
  org_id: UUIDSchema,
  provider: z.enum(['aws', 'azure', 'gcp']),
  account_id: z.string().optional(),
  region: z.string().optional(),
  resource_type: z.string().optional(),
  resource_id: z.string().optional(),
  check_id: z.string().optional(),
  check_title: z.string().optional(),
  severity: z.enum(['critical', 'high', 'medium', 'low', 'info']).optional(),
  status: z.enum(['pass', 'fail', 'warning', 'info']).optional(),
  remediation: z.string().optional(),
});

// SBOM Entry
export const CreateSBOMEntrySchema = z.object({
  org_id: UUIDSchema,
  project_id: UUIDSchema.optional(),
  component_name: z.string().min(1),
  component_version: z.string().min(1),
  package_manager: z.string().optional(),
  license: z.string().optional(),
  sbom_format: z.enum(['cyclonedx', 'spdx']).optional(),
  vulnerabilities: z.array(z.any()).optional().default([]),
});

// Detection Rule (Purple Team)
export const CreateDetectionRuleSchema = z.object({
  org_id: UUIDSchema,
  name: z.string().min(1),
  rule_type: z.enum(['sigma', 'yaral', 'kql', 'splunk']),
  rule_content: z.string().min(1),
  mitre_technique: z.string().optional(),
  status: z.enum(['draft', 'testing', 'production', 'deprecated']).default('draft'),
  created_by: UUIDSchema,
});

// Attack Simulation (Purple Team)
export const CreateAttackSimulationSchema = z.object({
  org_id: UUIDSchema,
  name: z.string().min(1),
  technique_id: z.string().optional(),
  created_by: UUIDSchema,
});

// Red Team C2 Server
export const CreateC2ServerSchema = z.object({
  org_id: UUIDSchema,
  assessment_id: UUIDSchema.optional(),
  name: z.string().min(1),
  c2_framework: z.enum(['sliver', 'havoc', 'mythic', 'cobalt_strike', 'bruteratel']),
  ip_address: z.string().ip(),
  port: z.number().min(1).max(65535),
  domain: z.string().optional(),
  redirector_enabled: z.boolean().optional().default(false),
  opsec_level: z.enum(['standard', 'high', 'paranoid']).default('standard'),
});

// Response types
export const ApiResponseSchema = <T extends z.ZodTypeAny>(inner: T) =>
  z.object({
    success: z.boolean(),
    data: inner.optional(),
    error: z.string().optional(),
    meta: z.object({
      timestamp: z.string().datetime(),
      request_id: z.string(),
    }).optional(),
  });

export const PaginatedResponseSchema = <T extends z.ZodTypeAny>(inner: T) =>
  z.object({
    items: z.array(inner),
    total: z.number(),
    page: z.number(),
    limit: z.number(),
    total_pages: z.number(),
  });

// Type exports inferred from schemas
export type LoginInput = z.infer<typeof LoginSchema>;
export type RegisterInput = z.infer<typeof RegisterSchema>;
export type CreateOrganizationInput = z.infer<typeof CreateOrganizationSchema>;
export type CreateUserInput = z.infer<typeof CreateUserSchema>;
export type CreateProjectInput = z.infer<typeof CreateProjectSchema>;
export type CreateAssetInput = z.infer<typeof CreateAssetSchema>;
export type CreateAssessmentInput = z.infer<typeof CreateAssessmentSchema>;
export type CreateToolRunInput = z.infer<typeof CreateToolRunSchema>;
export type CreateFindingInput = z.infer<typeof CreateFindingSchema>;
export type CreateAgentConversationInput = z.infer<typeof CreateAgentConversationSchema>;
export type CreateAgentPlanInput = z.infer<typeof CreateAgentPlanSchema>;
export type NanomiteChatInput = z.infer<typeof NanomiteChatSchema>;
export type CreateSigmaRuleInput = z.infer<typeof CreateSigmaRuleSchema>;
export type CreateSiemAlertInput = z.infer<typeof CreateSiemAlertSchema>;
export type CreateThreatIntelIOCInput = z.infer<typeof CreateThreatIntelIOCSchema>;
export type CreateSocMedInvestigationInput = z.infer<typeof CreateSocMedInvestigationSchema>;
export type CreateVulnScanTemplateInput = z.infer<typeof CreateVulnScanTemplateSchema>;
export type CreateCloudAssessmentInput = z.infer<typeof CreateCloudAssessmentSchema>;
export type CreateSBOMEntryInput = z.infer<typeof CreateSBOMEntrySchema>;
export type CreateDetectionRuleInput = z.infer<typeof CreateDetectionRuleSchema>;
export type CreateAttackSimulationInput = z.infer<typeof CreateAttackSimulationSchema>;
export type CreateC2ServerInput = z.infer<typeof CreateC2ServerSchema>;