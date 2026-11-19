# NANOMITE COMMAND CENTER — Design Specification

> **The central nervous system of your security operations.** Real-time visibility, AI-driven orchestration, and unified control across all security modules.

---

## 1. Vision & Principles

| Principle | Description |
|-----------|-------------|
| **AI-First** | Nanomite agent is not a sidebar — it's the primary interface. Natural language drives everything. |
| **Real-Time Everything** | WebSocket/SSE streams for tool output, findings, agent reasoning, system health. |
| **Context-Aware** | Every view understands: current project, assessment, selected assets, active findings. |
| **Operator-Centric** | Designed for security operators — dense information, keyboard-first, minimal clicks. |
| **Module-Agnostic** | OSINT, Pentest, VulnScan, Cloud, Wireless — same patterns, same data model. |
| **Evidence-Grade** | Every action logged, signed, attributable. Audit-ready by default. |

---

## 2. Layout Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────────┐
│  NANOMITE COMMAND CENTER                                    [Organization ▼] [User ▼] [🔔] [🌙] [⚙️]   │
├─────────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                     │
│  ┌──────────────────┐  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │                  │  │                                                                       │  │
│  │   MODULE NAV     │  │                        MAIN WORKSPACE                                 │  │
│  │   (Collapsible)  │  │                                                                       │  │
│  │                  │  │  ┌─────────────────────────────────────────────────────────────────┐  │  │
│  │  🏠 Overview     │  │  │                      CONTEXT BAR                                 │  │  │
│  │  🎯 Assessments  │  │  │  Project: Acme Corp ▼  |  Assessment: Q4 External Pentest ▼     │  │  │
│  │  🔍 OSINT        │  │  │  Scope: 12 assets  |  Status: RUNNING  |  Agent: PLANNING       │  │  │
│  │  ⚔️ Pentesting   │  │  └─────────────────────────────────────────────────────────────────┘  │  │
│  │  🛡️ Vuln Scanning│  │                                                                       │  │
│  │  ☁️ Cloud        │  │  ┌─────────────────────────┬─────────────────────────────────────┐  │  │
│  │  📡 Wireless     │  │  │                         │                                     │  │  │
│  │  📱 Mobile       │  │  │   PRIMARY PANEL         │      NANOMITE AGENT PANEL           │  │  │
│  │  🔗 API Security │  │  │   (Module Workspace)    │      (Persistent, Resizable)      │  │  │
│  │  🔴 Red Team     │  │  │                         │                                     │  │  │
│  │  🎭 Adversary    │  │  │  • Asset Graph          │  • Chat / Plan / Execute          │  │  │
│  │  🧠 Threat Intel │  │  • Finding Table          │  • Context Actions                │  │  │
│  │  🚨 IR/Forensics │  │  • Scan Results           │  • Skill Browser                  │  │  │
│  │  🎯 Social Eng   │  │  • Attack Paths           │  • Memory Search                  │  │  │
│  │  📊 Reports      │  │  • Compliance             │  • Plan Review                    │  │  │
│  │  ⚙️ Settings     │  │                         │                                     │  │  │
│  │                  │  │                         │                                     │  │  │
│  │  [🔍 Search...]  │  │                         │                                     │  │  │
│  │                  │  │                         │                                     │  │  │
│  │  QUICK ACTIONS   │  │                         │                                     │  │  │
│  │  ➕ New Assessment│  │                         │                                     │  │  │
│  │  ⚡ Quick Scan    │  │                         │                                     │  │  │
│  │  🤖 Ask Nanomite  │  │                         │                                     │  │  │
│  │                  │  │                         │                                     │  │  │
│  └──────────────────┘  └─────────────────────────────────────────────────────────────────┘  │
│                                                                                                     │
│  ┌─────────────────────────────────────────────────────────────────────────────────────────────┐ │
│  │ STATUS BAR:  ● Live  |  3 assessments running  |  47 findings today  |  Agent: Idle  |  ⌨️  │ │
│  └─────────────────────────────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### **Responsive Breakpoints**

| Breakpoint | Layout Change |
|------------|---------------|
| **≥1920px** | Full three-panel: Nav (280px) + Workspace (flex) + Agent (420px) |
| **1440-1919px** | Nav collapsible to 64px (icons only), Agent 380px |
| **1024-1439px** | Nav drawer (hamburger), Agent slides over workspace |
| **<1024px** | Mobile: Bottom sheet for Agent, Nav drawer, stacked workspace |

---

## 3. Module Navigation (Left Rail)

### **Structure**

```typescript
interface ModuleNavItem {
  id: string;
  label: string;
  icon: LucideIcon;
  badge?: number | string;           // Active assessments, critical findings
  status?: 'idle' | 'running' | 'error';
  children?: ModuleNavItem[];        // Sub-modules
  keyboardShortcut?: string;         // e.g., "g o" for OSINT
  requiredRole?: 'admin' | 'analyst' | 'viewer';
}
```

### **Module Groups**

```typescript
const MODULE_GROUPS = [
  {
    label: 'OPERATIONS',
    items: [
      { id: 'overview', label: 'Command Overview', icon: Home, shortcut: 'g o' },
      { id: 'assessments', label: 'Assessments', icon: Target, badge: 3, shortcut: 'g a' },
    ]
  },
  {
    label: 'INTELLIGENCE',
    items: [
      { id: 'osint', label: 'OSINT', icon: Search, badge: '12', status: 'running', shortcut: 'g i' },
      { id: 'threat-intel', label: 'Threat Intel', icon: ShieldAlert, shortcut: 'g t' },
      { id: 'adversary', label: 'Adversary Emulation', icon: UserPen, shortcut: 'g v' },
    ]
  },
  {
    label: 'OFFENSIVE',
    items: [
      { id: 'pentest', label: 'Pentesting', icon: Sword, shortcut: 'g p' },
      { id: 'redteam', label: 'Red Team Infra', icon: ServerCog, shortcut: 'g r' },
      { id: 'social', label: 'Social Engineering', icon: MailWarning, shortcut: 'g s' },
      { id: 'wireless', label: 'Wireless & RF', icon: Wifi, shortcut: 'g w' },
      { id: 'hardware', label: 'Hardware & Firmware', icon: Cpu, shortcut: 'g h' },
    ]
  },
  {
    label: 'DEFENSIVE',
    items: [
      { id: 'vulnscan', label: 'Vulnerability Scanning', icon: ShieldCheck, badge: '5', status: 'running', shortcut: 'g v' },
      { id: 'cloud', label: 'Cloud & Container', icon: Cloud, shortcut: 'g c' },
      { id: 'supply-chain', label: 'Supply Chain & SBOM', icon: PackageCheck, shortcut: 'g u' },
      { id: 'api', label: 'API Security', icon: Link, shortcut: 'g x' },
      { id: 'mobile', label: 'Mobile Security', icon: Smartphone, shortcut: 'g m' },
      { id: 'ir', label: 'IR & Forensics', icon: Activity, shortcut: 'g f' },
    ]
  },
  {
    label: 'GOVERNANCE',
    items: [
      { id: 'devsecops', label: 'DevSecOps Pipeline', icon: GitBranch, shortcut: 'g d' },
      { id: 'compliance', label: 'Compliance', icon: FileCheck, shortcut: 'g l' },
      { id: 'reports', label: 'Reports & Evidence', icon: FileText, shortcut: 'g e' },
      { id: 'training', label: 'Training & Awareness', icon: GraduationCap, shortcut: 'g n' },
    ]
  },
  {
    label: 'AI & AUTOMATION',
    items: [
      { id: 'ai-ml', label: 'AI/ML Model Security', icon: Brain, shortcut: 'g m' },
      { id: 'agent', label: 'Nanomite Agent', icon: Bot, status: 'idle', shortcut: 'g g' },
    ]
  },
  {
    label: 'ADMINISTRATION',
    items: [
      { id: 'settings', label: 'Settings', icon: Settings, requiredRole: 'admin' },
      { id: 'team', label: 'Team & Access', icon: Users, requiredRole: 'admin' },
      { id: 'integrations', label: 'Integrations', icon: Plug, requiredRole: 'admin' },
      { id: 'audit', label: 'Audit Logs', icon: History, requiredRole: 'admin' },
    ]
  }
];
```

### **Keyboard Navigation**

| Shortcut | Action |
|----------|--------|
| `Ctrl/Cmd + K` | Global command palette (search everything) |
| `g + [letter]` | Go to module (e.g., `g o` = Overview, `g a` = Assessments) |
| `Ctrl/Cmd + Shift + A` | New Assessment modal |
| `Ctrl/Cmd + Shift + N` | Ask Nanomite (focus agent panel) |
| `Ctrl/Cmd + B` | Toggle module nav |
| `/` | Focus global search |
| `?` | Show keyboard shortcuts help |

---

## 4. Context Bar (Top of Workspace)

### **Component Spec**

```tsx
interface ContextBarProps {
  project: Project | null;
  assessment: Assessment | null;
  assets: Asset[];
  findings: Finding[];
  agentStatus: AgentStatus;
  onProjectChange: (projectId: string) => void;
  onAssessmentChange: (assessmentId: string) => void;
}
```

### **Visual States**

```
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│ PROJECT: Acme Corporation ▼          ASSESSMENT: Q4 External Pentest ▼        [Create New +]    │
├──────────────────────────────────────────────────────────────────────────────────────────────────┤
│ SCOPE: 12 assets (8 domains, 3 IPs, 1 CIDR)  |  STATUS: ● RUNNING  |  STARTED: 2h 14m ago      │
│ AGENT: 🤖 PLANNING  |  TOOLS ACTIVE: 3 (nuclei, subfinder, amass)  |  FINDINGS: 47 (🔴8 🟠15 🟡19 🟢5) │
│ RESOURCES: CPU 34%  |  RAM 2.1/8 GB  |  NETWORK 12 Mbps ↑ / 45 Mbps ↓  |  STORAGE 12.3 GB used   │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### **Live Indicators**

| Indicator | Source | Update Frequency |
|-----------|--------|------------------|
| Assessment Status | WebSocket (orchestrator) | Real-time |
| Tool Runs | SSE (tool execution) | Real-time |
| Findings Count | WebSocket (finding service) | Real-time |
| Agent State | SSE (nanomite agent) | Real-time |
| System Resources | Prometheus / Node Exporter | 10s |
| Network/Storage | Prometheus | 30s |

---

## 5. Primary Workspace Panels (Per Module)

### **5.1 Command Overview (Landing Page)**

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
│ NANOMITE COMMAND CENTER — OVERVIEW                                                                    │
├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                     │
│  ┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐            │
│  │ ACTIVE ASSESSMENTS│ │ CRITICAL FINDINGS │ │ ASSETS MONITORED │ │ AGENT ACTIONS    │            │
│  │        3          │ │        8          │ │       247        │ │       12/hr      │            │
│  │  ████████░░ 75%   │  │  ██████████░ 92%  │  │  ████████████ 100%│  │  ████████░░ 80%  │            │
│  └──────────────────┘ └──────────────────┘ └──────────────────┘ └──────────────────┘            │
│                                                                                                     │
│  ┌────────────────────────────────────────┐ ┌────────────────────────────────────────┐           │
│  │ RECENT ACTIVITY (Live Feed)            │ │ THREAT LANDSCAPE                        │           │
│  ├────────────────────────────────────────┤ ├────────────────────────────────────────┤           │
│  │ 🔴 2m ago  | nuclei  | CVE-2024-XXXX   │ │  ┌──────────────────────────────────┐  │           │
│  │ 🟠 5m ago  | subfinder| 47 subdomains  │ │  │  🌍 Geographic Attack Heatmap   │  │           │
│  │ 🟡 12m ago | Agent    | Plan approved  │ │  │  (Real-time from threat feeds)  │  │           │
│  │ 🟢 1h ago  | Report   | PDF generated  │ │  └──────────────────────────────────┘  │           │
│  │ 🔵 3h ago  | Sync     | NVD updated    │ │  Top CVEs: CVE-2024-3094, CVE-2024-21413 │           │
│  └────────────────────────────────────────┘ └────────────────────────────────────────┘           │
│                                                                                                     │
│  ┌────────────────────────────────────────┐ ┌────────────────────────────────────────┐           │
│  │ QUICK LAUNCH                           │ │ NANOMITE RECOMMENDATIONS               │           │
│  ├────────────────────────────────────────┤ ├────────────────────────────────────────┤           │
│  │ ⚡ [Quick Recon]    [Full Pentest]      │ │ 🤖 "Based on new CVE-2024-XXXX,      │           │
│  │ 🎯 [Targeted Scan]  [Cloud Audit]       │ │    recommend scanning all assets   │           │
│  │ 📡 [Wireless Audit] [Mobile Test]       │ │    with 'web' tag for this vuln."  │           │
│  │ 🔗 [API Fuzz]       [Supply Chain]      │ │                                    │           │
│  │ 📱 [Social Campaign]  [Red Team]        │ │ 🤖 "Asset 'api.acme.com' has no    │           │
│  └────────────────────────────────────────┘ │    WAF — suggest WAF evasion test."  │           │
│                                             └────────────────────────────────────────┘           │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### **5.2 OSINT Workspace**

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
│ OSINT — RECON WORKSPACE                              [New Recon] [Import] [Export] [🤖 Ask]    │
├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│ TABS: [🌐 Subdomains] [🔌 Ports & Services] [🔧 Technologies] [🔐 SSL/TLS] [📧 Emails] [🌍 DNS]  │
│       [☁️ Cloud Assets] [📦 Git/Code] [🖼️ Metadata] [👥 Social] [📊 Threat Intel] [🗺️ Attack Surface] │
├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                     │
│  SUBDOMAINS TAB                                                                                     │
│  ┌─────────────────────────────────────────────────────────────────────────────────────────────┐  │
│  │ FILTERS: [🔍 Search...] [Status: All ▼] [Source: All ▼] [Tag: ▼] [📅 Last 7d ▼] [Columns ▼]│  │
│  ├─────────────────────────────────────────────────────────────────────────────────────────────┤  │
│  │ ☐  subdomain                 │ Status │ IP(s)          │ Ports    │ Tech      │ Source    │  │
│  │ ─────────────────────────────┼────────┼────────────────┼──────────┼───────────┼───────────┤  │
│  │ 🌐 api.acme.com             │ ● Live │ 1.2.3.4        │ 80,443   │ nginx     │ amass     │  │
│  │ 🌐 admin.acme.com           │ ● Live │ 1.2.3.5        │ 443,8080 │ react     │ subfinder │  │
│  │ 🌐 dev.acme.com             │ ○ Dead │ —              │ —        │ —         │ crt.sh    │  │
│  │ 🌐 staging.acme.com         │ ● Live │ 1.2.3.6        │ 22,80,443│ jenkins   │ amass     │  │
│  │ 🌐 vpn.acme.com             │ ● Live │ 1.2.3.7        │ 443,1194 │ openvpn   │ dnsdb     │  │
│  │ 🌐 mail.acme.com            │ ● Live │ 1.2.3.8        │ 25,143,993│ postfix  │ crt.sh    │  │
│  │ 🌐 *.acme.com (wildcard)    │ ● Live │ 1.2.3.9        │ 443      │ cloudflare│ crt.sh    │  │
│  └─────────────────────────────────────────────────────────────────────────────────────────────┘  │
│  [Selected: 3] [🤖 Scan Ports] [🔍 Tech Detect] [🕷️ Crawl] [📸 Screenshot] [🏷️ Tag] [📤 Export]   │
│                                                                                                     │
│  ┌─────────────────────────────────────────────────────────────────────────────────────────────┐  │
│  │ SUBDOMAIN GRAPH (Force-directed, WebGL)                                                    │  │
│  │  [Clustering: Source ▼] [Layout: Force ▼] [Filter: Live Only] [Export PNG/SVG]            │  │
│  │  ● api.acme.com ── ● admin.acme.com                                                        │  │
│  │       │                  │                                                                   │  │
│  │       └──────────────────┘                                                                   │  │
│  │            │                                                                                │  │
│  │       ● staging.acme.com                                                                    │  │
│  └─────────────────────────────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### **5.3 Pentesting Workspace**

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
│ PENTESTING — ENGAGEMENTS                          [New Engagement] [Templates] [🤖 Ask]        │
├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│ TABS: [🎯 Engagements] [🖥️ Sessions] [🔑 Credentials] [📁 Loot] [🗺️ Attack Paths] [📋 Checklists] │
├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                     │
│  ENGAGEMENTS LIST                                                                                   │
│  ┌─────────────────────────────────────────────────────────────────────────────────────────────┐  │
│  │ ☐  Name                    │ Type          │ Status    │ Targets  │ Findings │ Started    │  │
│  │ ───────────────────────────┼───────────────┼───────────┼──────────┼──────────┼────────────┤  │
│  │ 🎯 Q4 External Pentest     │ Web + Network │ ● Running │ 12       │ 47       │ 2h ago     │  │
│  │ 🎯 Internal Network Assess │ Network       │ ○ Paused  │ 234      │ 12       │ 3d ago     │  │
│  │ 🎯 Wireless Audit          │ Wireless      │ ✓ Done    │ 3 APs    │ 5        │ 1w ago     │  │
│  │ 🎯 Mobile App Test         │ Mobile        │ ⚠ Failed  │ 2 apps   │ 0        │ 2w ago     │  │
│  └─────────────────────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                                     │
│  SESSION DETAIL (when engagement selected)                                                         │
│  ┌────────────────────────────────────────┐ ┌────────────────────────────────────────┐           │
│  │ ACTIVE SESSIONS                        │ │ TOOL TIMELINE                          │           │
│  ├────────────────────────────────────────┤ ├────────────────────────────────────────┤           │
│  │ ▶ msfconsole  (SESSION-001)            │ │ 10:00 ── nuclei ──► 10:15 ── sqlmap ──►│           │
│  │   Target: 1.2.3.4:443                  │ │        (47 findings)     (3 SQLi)      │           │
│  │   User: analyst@acme.com               │ │ 10:30 ── hydra ──────► 10:45 ── bloodhound│           │
│  │   Idle: 2m                             │ │        (2 creds)        (AD mapped)    │           │
│  │   [Interact] [Background] [Kill]       │ │                                            │           │
│  │                                        │ │ [🤖 Agent suggests: kerberoast next]     │           │
│  │ ▶ sliver (SESSION-002)                 │ └────────────────────────────────────────┘           │
│  │   Implant: DEADBEEF                    │                                                      │
│  │   Beacon: 30s                          │ ┌────────────────────────────────────────┐           │
│  │   [Interact] [Tasks] [Screenshot]      │ │ CREDENTIALS HARVESTED                    │           │
│  └────────────────────────────────────────┤ ├────────────────────────────────────────┤           │
│                                            │ │ Type      │ User          │ Domain  │ Source  │
│                                            │ │ ──────────┼───────────────┼─────────┼─────────│
│                                            │ │ NTLM      │ svc_sql       │ ACME    │ bloodhound│
│                                            │ │ Kerberos  │ admin         │ ACME    │ kerberoast│
│                                            │ │ Cleartext │ jdoe          │ ACME    │ mimikatz  │
│                                            │ └────────────────────────────────────────┘           │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### **5.4 Vulnerability Scanning Workspace**

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
│ VULNERABILITY SCANNING                            [New Scan] [Templates] [Schedules] [🤖 Ask]  │
├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│ TABS: [📋 Scans] [🎯 Findings] [📦 Assets] [☁️ Cloud] [📦 Containers] [📦 Dependencies] [📊 Compliance] │
├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                     │
│  SCANS LIST                                                                                         │
│  ┌─────────────────────────────────────────────────────────────────────────────────────────────┐  │
│  │ ☐  Name                    │ Type          │ Status    │ Targets  │ Findings │ Duration   │  │
│  │ ───────────────────────────┼───────────────┼───────────┼──────────┼──────────┼────────────┤  │
│  │ 🛡️ Weekly External Scan    │ Network       │ ● Running │ 47       │ 23       │ 1h 23m     │  │
│  │ 🛡️ Container Image Scan    │ Container     │ ✓ Done    │ 12 imgs  │ 89       │ 4m 32s     │  │
│  │ 🛡️ Kubernetes CIS Bench    │ Cloud         │ ✓ Done    │ 3 clusters│ 34      │ 2m 11s     │  │
│  │ 🛡️ Dependency Scan (CI)    │ Dependencies  │ ⚠ Partial │ 5 repos  │ 12       │ 3m 45s     │  │
│  │ 🛡️ API Schema Fuzz         │ API           │ ○ Queued  │ 8 APIs   │ —        │ —          │  │
│  └─────────────────────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                                     │
│  FINDINGS AGGREGATE VIEW (cross-scan)                                                              │
│  ┌─────────────────────────────────────────────────────────────────────────────────────────────┐  │
│  │ GROUP BY: [Severity ▼] [Asset ▼] [CWE ▼] [Scanner ▼] [Status ▼] [EPSS > 0.5] [KEV Only]   │  │
│  ├─────────────────────────────────────────────────────────────────────────────────────────────┤  │
│  │ 🔴 CRITICAL (8)                                                                            │  │
│  │   ████████████████████ CVE-2024-3094 (xz utils backdoor)  EPSS: 0.94  KEV: ✓  [Fix: Upgrade]│  │
│  │   ████████████████████ CVE-2024-21413 (Outlook RCE)         EPSS: 0.87  KEV: ✓  [Patch]    │  │
│  │ 🟠 HIGH (15)                                                                               │  │
│  │   ████████████████░░░░ CVE-2024-XXXX (nginx path traversal)  EPSS: 0.34  [WAF Rule]       │  │
│  │ 🟡 MEDIUM (19)                                                                             │  │
│  │ 🟢 LOW (5)                                                                                 │  │
│  └─────────────────────────────────────────────────────────────────────────────────────────────┘  │
│  [Selected: 3] [🤖 Generate Remediation Plan] [📋 Create Tickets] [🏷️ Tag] [📤 Export OSCAL]    │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Nanomite Agent Panel (Right Panel — Persistent)

### **6.1 Panel States**

```
┌─────────────────────────────────────────────────────────────────┐
│ NANOMITE AGENT                                    [⛶] [✕] [⚙️] │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  MODE TABS: [💬 Chat] [📋 Plan] [⚡ Execute] [🔧 Skills] [🧠 Memory] │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐ │
│  │ CHAT MODE                                                 │ │
│  ├───────────────────────────────────────────────────────────┤ │
│  │                                                           │ │
│  │ 🤖  Nanomite: I've analyzed the 47 findings from your     │ │
│  │     external scan. 8 critical, 15 high. The xz backdoor  │ │
│  │     (CVE-2024-3094) affects 3 assets. Want me to:        │ │
│  │                                                           │ │
│  │     1. Generate remediation plan for all critical/high   │ │
│  │     2. Launch targeted exploits to validate exploitability│ │
│  │     3. Create OSCAL evidence package for compliance      │ │
│  │     4. Schedule continuous monitoring for these CVEs     │ │
│  │                                                           │ │
│  │ 👤  You: Generate remediation plan and create Jira tickets│ │
│  │                                                           │ │
│  │ 🤖  Nanomite: [Thinking...]                               │ │
│  │     🔧 Tool: jira_create_ticket                          │ │
│  │     🔧 Tool: generate_remediation_plan                   │ │
│  │     ✅ Created 23 tickets (PROJ-1247 to PROJ-1269)       │ │
│  │     ✅ Remediation plan saved to assessment               │ │
│  │                                                           │ │
│  │ 🤖  Nanomite: Done. Remediation plan includes:           │ │
│  │     • Priority matrix (CVSS × EPSS × Asset Criticality)  │ │
│  │     • Specific fix commands per asset/tech stack         │ │
│  │     • Verification steps for each remediation            │ │
│  │     • Rollback procedures                                │ │
│  │     View in Reports → Remediation Plans                  │ │
│  │                                                           │ │
│  └───────────────────────────────────────────────────────────┘ │
│  ┌───────────────────────────────────────────────────────────┐ │
│  │ [🎤 Voice]  Ask Nanomite...                    [Send ↵]   │ │
│  └───────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

### **6.2 Plan Review Mode**

```
┌─────────────────────────────────────────────────────────────────┐
│ NANOMITE AGENT — PLAN REVIEW                     [Approve All] │
├─────────────────────────────────────────────────────────────────┤
│ GOAL: "Validate exploitability of all critical findings"       │
│ SCOPE: Assessment Q4 External Pentest (12 assets, 8 critical)  │
│ CONSTRAINTS: No DoS, business hours only, scope IPs only       │
├─────────────────────────────────────────────────────────────────┤
│ ☑  Step 1: Enumerate attack surface for each critical finding  │
│    Tool: nuclei + custom templates | Est: 5m | Risk: Low       │
│    ▸ Requires approval: No                                     │
│                                                                │
│ ☑  Step 2: Launch targeted exploitation (CVE-2024-3094)      │
│    Tool: metasploit + custom exploit | Est: 15m | Risk: Medium│
│    ▸ Requires approval: YES ⚠️                                 │
│    ▸ Safety check: Read-only verification mode                 │
│                                                                │
│ ☐  Step 3: Post-exploitation enumeration (if Step 2 succeeds) │
│    Tool: meterpreter + post modules | Est: 20m | Risk: High   │
│    ▸ Requires approval: YES ⚠️                                 │
│    ▸ Depends on: Step 2 success                                │
│                                                                │
│ ☐  Step 4: Credential harvesting & lateral movement mapping   │
│    Tool: bloodhound + secretsdump | Est: 10m | Risk: High    │
│    ▸ Requires approval: YES ⚠️                                 │
│                                                                │
│ ☐  Step 5: Evidence collection & attestation                   │
│    Tool: evidence_collector + cosign | Est: 5m | Risk: None   │
│    ▸ Requires approval: No                                     │
│                                                                │
│ [▶ Execute Approved]  [⏸ Pause]  [✏️ Modify]  [🗑️ Discard]   │
└─────────────────────────────────────────────────────────────────┘
```

### **6.3 Skills Browser**

```
┌─────────────────────────────────────────────────────────────────┐
│ NANOMITE SKILLS REGISTRY                          [🔍 Search]  │
├─────────────────────────────────────────────────────────────────┤
│ CATEGORIES: [All] [Recon] [Exploit] [Post-Exploit] [Scan] [Util]│
│ FILTERS: [Installed] [Available] [Favorites] [Recent]           │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  🔍 RECON (12 skills)                                          │
│  ┌───────────────────────────────────────────────────────────┐ │
│  │ subdomain_enum     │ v1.2.0 │ 🟢 Installed │ 47 uses    │ │
│  │   Multi-source subdomain enumeration (amass, subfinder,   │ │
│  │   crt.sh, dnsdb, VT). Recursive, bruteforce, API keys.   │ │
│  │   [★] [📖 Docs] [🗑️ Uninstall] [▶ Run]                    │ │
│  ├───────────────────────────────────────────────────────────┤ │
│  │ port_scan          │ v1.0.0 │ 🟢 Installed │ 128 uses   │ │
  │   Fast TCP/SYN scan (masscan, rustscan, nmap). Service    │ │
│  │   detection, banner grab, script scan.                    │ │
│  │   [★] [📖 Docs] [▶ Run]                                   │ │
│  ├───────────────────────────────────────────────────────────┤ │
│  │ tech_fingerprint   │ v1.1.0 │ 🟡 Available │ —          │ │
│  │   Wappalyzer-based tech detection. Requires: wappalyzer  │ │
│  │   [★] [📖 Docs] [⬇️ Install]                              │ │
│  └───────────────────────────────────────────────────────────┘ │
│                                                                 │
│  ⚔️ EXPLOIT (8 skills)                                         │
│  ┌───────────────────────────────────────────────────────────┐ │
│  │ sql_injection      │ v1.0.0 │ 🟢 Installed │ 23 uses    │ │
│  │   Automated SQLi detection & exploitation (sqlmap).       │ │
│  │   Union, boolean, time-based, stacked queries.            │ │
│  │   [★] [📖 Docs] [▶ Run]                                   │ │
│  ├───────────────────────────────────────────────────────────┤ │
│  │ xss_exploit        │ v1.0.0 │ 🟢 Installed │ 12 uses    │ │
│  │   Reflected, stored, DOM XSS exploitation (xsstrike,      │ │
│  │   dalfox). Payload generation, WAF bypass.                │ │
│  │   [★] [📖 Docs] [▶ Run]                                   │ │
│  └───────────────────────────────────────────────────────────┘ │
│                                                                 │
│  [📦 Install Selected] [⭐ Sync Favorites] [📤 Export Skills]  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 7. Status Bar (Bottom)

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ ● LIVE  |  🟢 3 assessments running  |  🔴 8 critical findings  |  🤖 Agent: PLANNING  |  ⌨️ Shortcuts  │
├─────────────────────────────────────────────────────────────────────────────────────────────────────┤
│  CPU: ████░░░░░░ 34%    MEM: ██████░░░░ 42% (3.2/8 GB)    NET: ↑12 Mbps ↓45 Mbps    DISK: ████████░░ 67% (12.3/18 GB)  │
└─────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### **Status Indicators**

| Indicator | States | Click Action |
|-----------|--------|--------------|
| Connection | ● LIVE / ● RECONNECTING / ○ OFFLINE | Open connection diagnostics |
| Assessments | Count + color | Open Assessments module |
| Findings | Critical count (pulse if >0) | Open Findings with critical filter |
| Agent | IDLE / THINKING / PLANNING / EXECUTING / ERROR | Focus Agent panel |
| Resources | Bar + % | Open System Monitor modal |
| Keyboard | ⌨️ | Show shortcuts help |

---

## 8. Command Palette (Ctrl/Cmd + K)

```
┌────────────────────────────────────────────────────────────────────────────┐
│ 🔍 Command Palette                                    [/] Search...        │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  RECENT                                                                    │
│  ──────────────────────────────────────────────────────────────────────   │
│  🎯 Q4 External Pentest                              Assessment            │
│  🔍 Subdomain Enum: acme.com                         Tool Run              │
│  🎯 Finding: CVE-2024-3094 on api.acme.com          Finding               │
│                                                                             │
│  SUGGESTED                                                                 │
│  ──────────────────────────────────────────────────────────────────────   │
│  ➕ New Assessment                                    Ctrl+Shift+A         │
│  ⚡ Quick Recon Scan                                  Ctrl+Shift+R         │
│  🤖 Ask Nanomite                                      Ctrl+Shift+N         │
│  📊 Generate Report                                   Ctrl+Shift+G         │
│  ⚙️ Settings                                           Ctrl+,              │
│                                                                             │
│  MODULES                                                                   │
│  ──────────────────────────────────────────────────────────────────────   │
│  🏠 Overview                    g o                                          │
│  🎯 Assessments                 g a                                          │
│  🔍 OSINT                       g i                                          │
│  ⚔️ Pentesting                  g p                                          │
│  🛡️ Vulnerability Scanning      g v                                          │
│  📡 Wireless & RF               g w                                          │
│  📱 Mobile Security             g m                                          │
│  🚨 IR & Forensics              g f                                          │
│  📊 Reports                     g e                                          │
│                                                                             │
│  ACTIONS                                                                   │
│  ──────────────────────────────────────────────────────────────────────   │
│  🔄 Sync Threat Intel Feeds                                                        │
│  📥 Import Assets from CSV                                                         │
│  📤 Export Findings to Jira                                                        │
│  🔐 Rotate API Keys                                                                │
│  🧹 Clear Agent Memory                                                             │
│  🛠️ Run Tool: nuclei --template cves/2024/...                                    │
│                                                                             │
└────────────────────────────────────────────────────────────────────────────┘
```

---

## 9. Real-Time Event Types (WebSocket/SSE)

```typescript
type CommandCenterEvent =
  // Assessment lifecycle
  | { type: 'assessment.created'; payload: Assessment }
  | { type: 'assessment.started'; payload: { assessmentId: string } }
  | { type: 'assessment.progress'; payload: { assessmentId: string; progress: number; currentTool: string } }
  | { type: 'assessment.completed'; payload: { assessmentId: string; summary: AssessmentSummary } }
  | { type: 'assessment.failed'; payload: { assessmentId: string; error: string } }

  // Tool execution
  | { type: 'tool.started'; payload: ToolRun }
  | { type: 'tool.stdout'; payload: { runId: string; line: string } }
  | { type: 'tool.stderr'; payload: { runId: string; line: string } }
  | { type: 'tool.completed'; payload: { runId: string; result: ToolResult } }
  | { type: 'tool.failed'; payload: { runId: string; error: string } }

  // Findings
  | { type: 'finding.created'; payload: Finding }
  | { type: 'finding.updated'; payload: Finding }
  | { type: 'finding.bulk_created'; payload: Finding[] }

  // Assets
  | { type: 'asset.discovered'; payload: Asset }
  | { type: 'asset.updated'; payload: Asset }

  // Nanomite Agent
  | { type: 'agent.thinking'; payload: { conversationId: string; content: string } }
  | { type: 'agent.tool_call'; payload: { conversationId: string; tool: string; args: any } }
  | { type: 'agent.tool_result'; payload: { conversationId: string; tool: string; result: any } }
  | { type: 'agent.finding'; payload: { conversationId: string; finding: Finding } }
  | { type: 'agent.plan_generated'; payload: { conversationId: string; plan: Plan } }
  | { type: 'agent.plan_step'; payload: { conversationId: string; step: PlanStep } }
  | { type: 'agent.complete'; payload: { conversationId: string; summary: string } }
  | { type: 'agent.error'; payload: { conversationId: string; error: string } }
  | { type: 'agent.status_changed'; payload: AgentStatus }

  // System
  | { type: 'system.resources'; payload: SystemResources }
  | { type: 'notification'; payload: Notification }
```

---

## 10. Theming & Customization

### **Color System**

```css
:root {
  /* Nanomite Brand */
  --nm-primary: #00d4aa;        /* Teal - primary actions, agent accent */
  --nm-primary-hover: #00e8bb;
  --nm-primary-muted: #00d4aa1a;

  /* Semantic */
  --nm-critical: #ff3366;       /* Critical findings, errors */
  --nm-high: #ff6b35;           /* High severity */
  --nm-medium: #ffc107;         /* Medium severity */
  --nm-low: #00d4aa;            /* Low severity / success */
  --nm-info: #3b82f6;           /* Info */

  /* Surfaces */
  --nm-bg-primary: #0a0f14;     /* Main background */
  --nm-bg-secondary: #111820;   /* Panels, cards */
  --nm-bg-tertiary: #1a2330;    /* Hover states, inputs */
  --nm-border: #2a3548;         /* Borders */
  --nm-border-focus: #00d4aa;   /* Focus rings */

  /* Text */
  --nm-text-primary: #e8edf5;
  --nm-text-secondary: #9aa7bd;
  --nm-text-muted: #6b7a94;

  /* Agent */
  --nm-agent-user: #3b82f6;
  --nm-agent-assistant: #00d4aa;
  --nm-agent-tool: #ffc107;
  --nm-agent-thinking: #9aa7bd;
  --nm-agent-error: #ff3366;
}
```

### **Density Settings**

| Density | Row Height | Font Size | Padding | Use Case |
|---------|------------|-----------|---------|----------|
| **Compact** | 32px | 12px | 4px 8px | Max data, experienced ops |
| **Comfortable** | 40px | 13px | 8px 12px | Default |
| **Spacious** | 48px | 14px | 12px 16px | Presentations, touch |

### **User Preferences (Persisted)**

```typescript
interface UserPreferences {
  theme: 'dark' | 'light' | 'system';
  density: 'compact' | 'comfortable' | 'spacious';
  agentPanelWidth: number;        // 320-600px
  moduleNavCollapsed: boolean;
  realTimeUpdates: boolean;
  autoScrollAgent: boolean;
  showKeyboardHints: boolean;
  timezone: string;
  dateFormat: 'relative' | 'absolute' | 'iso';
  notifications: {
    findings: boolean;
    assessmentComplete: boolean;
    agentComplete: boolean;
    systemAlerts: boolean;
  };
}
```

---

## 11. Accessibility (WCAG 2.1 AA)

| Requirement | Implementation |
|-------------|----------------|
| **Keyboard Navigation** | All interactive elements reachable; focus visible; skip links |
| **Screen Readers** | ARIA labels, live regions for real-time updates, semantic HTML |
| **Color Contrast** | 4.5:1 minimum; status colors have patterns/icons, not color-only |
| **Focus Management** | Trap in modals; restore on close; visible focus indicators |
| **Reduced Motion** | Respect `prefers-reduced-motion`; disable animations |
| **Zoom** | Functional at 200% zoom; no horizontal scroll at 320px width |

---

## 12. Performance Targets

| Metric | Target |
|--------|--------|
| **Initial Load** | <2s (FCP), <3s (TTI) |
| **Module Switch** | <200ms (cached), <500ms (cold) |
| **WebSocket Reconnect** | <1s |
| **Agent Response (streaming)** | First token <500ms |
| **Table Render (10k rows)** | <100ms (virtualized) |
| **Graph Render (1k nodes)** | <500ms (WebGL) |
| **Memory (idle)** | <150MB |
| **Bundle Size (gzipped)** | <500KB initial, <2MB total |

---

## 13. Implementation Roadmap

| Sprint | Deliverable |
|--------|-------------|
| **1** | Layout shell, module nav, context bar, theme system, keyboard nav |
| **2** | Overview dashboard, real-time WebSocket infrastructure, status bar |
| **3** | OSINT workspace (subdomains, ports, tech, graph) |
| **4** | Pentesting workspace (engagements, sessions, credentials) |
| **5** | VulnScan workspace (scans, findings aggregate, compliance) |
| **6** | Nanomite Agent Panel (Chat, Plan, Execute, Skills, Memory) |
| **7** | Remaining modules (Cloud, Wireless, Mobile, IR, etc.) |
| **8** | Command palette, global search, notifications, settings |
| **9** | Accessibility audit, performance optimization, polish |
| **10** | Beta release, documentation, onboarding flow |

---

## 14. File Structure (Frontend)

```
packages/frontend/src/
├── app/
│   ├── layout.tsx                 # Root layout with providers
│   ├── providers.tsx              # QueryClient, Auth, Theme, WS, Agent
│   ├── router.tsx                 # TanStack Router + route tree
│   └── routes/
│       ├── __auth/                # Auth layout
│       ├── command-center/        # Main app layout
│       │   ├── layout.tsx         # Shell with nav, context bar, agent panel
│       │   ├── index.tsx          # Overview dashboard
│       │   ├── assessments/
│       │   ├── osint/
│       │   ├── pentest/
│       │   ├── vulnscan/
│       │   ├── cloud/
│       │   ├── wireless/
│       │   ├── mobile/
│       │   ├── api/
│       │   ├── redteam/
│       │   ├── adversary/
│       │   ├── threat-intel/
│       │   ├── ir/
│       │   ├── social/
│       │   ├── devsecops/
│       │   ├── supply-chain/
│       │   ├── ai-ml/
│       │   ├── hardware/
│       │   ├── training/
│       │   ├── reports/
│       │   ├── compliance/
│       │   └── settings/
│       └── *.tsx                  # Other routes
├── features/
│   ├── command-center/
│   │   ├── components/
│   │   │   ├── ModuleNav.tsx
│   │   │   ├── ContextBar.tsx
│   │   │   ├── AgentPanel.tsx
│   │   │   ├── StatusBar.tsx
│   │   │   ├── CommandPalette.tsx
│   │   │   └── LayoutShell.tsx
│   │   ├── hooks/
│   │   │   ├── useCommandCenter.ts
│   │   │   ├── useWebSocket.ts
│   │   │   ├── useKeyboardShortcuts.ts
│   │   │   └── usePreferences.ts
│   │   └── state/
│   │       ├── contextStore.ts    # Project, assessment, assets, findings
│   │       ├── agentStore.ts      # Agent conversation, plan, skills
│   │       └── uiStore.ts         # Nav collapsed, panel widths, density
│   ├── assessments/
│   ├── osint/
│   ├── pentest/
│   ├── vulnscan/
│   ├── agent/                     # Nanomite-specific components
│   │   ├── ChatMode.tsx
│   │   ├── PlanMode.tsx
│   │   ├── ExecuteMode.tsx
│   │   ├── SkillsBrowser.tsx
│   │   ├── MemoryBrowser.tsx
│   │   └── ContextActions.tsx
│   └── ...
├── components/
│   ├── ui/                        # shadcn/ui components
│   ├── data-display/
│   │   ├── DataTable.tsx          # Virtualized, sortable, filterable
│   │   ├── FindingCard.tsx
│   │   ├── AssetCard.tsx
│   │   ├── ToolRunTimeline.tsx
│   │   ├── AttackGraph.tsx        # WebGL force-directed
│   │   ├── SubdomainGraph.tsx
│   │   └── SeverityDistribution.tsx
│   ├── forms/
│   └── feedback/
├── hooks/
├── lib/
│   ├── api.ts                     # API client + types
│   ├── ws.ts                      # WebSocket manager
│   ├── sse.ts                     # SSE helper
│   ├── utils.ts
│   ├── validations.ts             # Zod schemas
│   └── constants.ts
└── styles/
    ├── globals.css
    ├── theme.css
    └── animations.css
```

---

## 15. Next Steps

1. **Create ADR** for Command Center architecture (decision: React + TanStack Router + shadcn/ui + WebSocket)
2. **Scaffold frontend monorepo** with Vite + React 18 + TypeScript + Tailwind + shadcn/ui
3. **Build LayoutShell** with ModuleNav, ContextBar, AgentPanel, StatusBar
4. **Implement WebSocket/SSE infrastructure** with auto-reconnect, event routing
5. **Build Overview Dashboard** with live metrics, activity feed, quick launch
6. **Implement OSINT Workspace** as first full module (subdomains table + graph)

---

**The NANOMITE COMMAND CENTER is not just a dashboard — it's the operational hub where human expertise meets AI orchestration. Every pixel serves the mission.**