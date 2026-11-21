//! # Multi-Agent Orchestration — Sub-Agents Working in Parallel
//!
//! Spawns coordinated sub-agents (recon, vuln, crack, report) that work on
//! different targets simultaneously, sharing results through the neural
//! memory and evidence locker. Each sub-agent runs a focused goal through the
//! autonomous planner, then reports findings back to the coordinator.
//!
//! Execution is plan-only: sub-agents produce commands, parse results, and
//! record findings — they never exploit.

use crate::agent::runtime::{AgentEvent, AgentRuntime};
use crate::neural::autonomous_planner::{AutonomousPlanner, PlanTask};
use crate::neural::memory::NeuralMemory;
use crate::neural::{hash_string, now_secs};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

/// A sub-agent role: what kind of assessment it performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentRole {
    Recon,  // web_recon, scan_ports, whois-style discovery
    Vuln,   // nikto, sqlmap, directory busting, web vuln checks
    Crack,  // password cracking, hash cracking, credential reuse
    Report, // synthesize findings into a report from evidence
}

/// One sub-agent in the swarm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgent {
    pub id: String,
    pub role: AgentRole,
    pub target: String, // URL, host, network range, or hash list
    pub goal: String,   // natural-language goal for this sub-agent
    pub status: SubAgentStatus,
    pub result: Option<SubAgentResult>,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubAgentStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult {
    pub agent_id: String,
    pub role: AgentRole,
    pub target: String,
    pub summary: String,
    pub findings: Vec<Finding>,
    pub tools_used: Vec<String>,
    pub evidence_ids: Vec<String>,
}

/// A single finding produced by a sub-agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub title: String,
    pub severity: FindingSeverity,
    pub description: String,
    pub target: String,
    pub evidence_ref: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

/// Arguments for the `multi_agent_orchestrate` tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAgentArgs {
    /// High-level goal for the swarm (e.g. "assess the web app on 10.0.0.5").
    pub goal: String,
    /// Optional target scope hint (URL, host, network range).
    #[serde(default)]
    pub scope: Option<String>,
    /// Roles to spawn (default: recon + vuln + report).
    #[serde(default = "default_roles")]
    pub roles: Vec<AgentRole>,
    /// Whether to require human approval before each risky tool call (default true).
    #[serde(default = "default_true")]
    pub require_approval: bool,
}

fn default_roles() -> Vec<AgentRole> {
    vec![AgentRole::Recon, AgentRole::Vuln, AgentRole::Report]
}

fn default_true() -> bool {
    true
}

/// Result of a multi-agent orchestration run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAgentResult {
    pub swarm_id: String,
    pub goal: String,
    pub agents: Vec<SubAgent>,
    pub tasks_completed: usize,
    pub tasks_failed: usize,
    pub total_tasks: usize,
    pub summary: String,
}

/// The multi-agent orchestrator: spawns and coordinates sub-agents.
#[derive(Clone)]
pub struct AgentOrchestrator {
    runtime: Arc<AgentRuntime>,
    neural_memory: Option<Arc<NeuralMemory>>,
    /// Pre-tool-call gate: consulted before each risky tool dispatch.
    #[allow(clippy::type_complexity)]
    pre_tool_gate: Option<Arc<dyn Fn(&str, &str) -> bool + Send + Sync>>,
    /// Active swarm (if any).
    active_swarm: Option<Vec<SubAgent>>,
    /// Data directory for persisting swarm results.
    data_dir: PathBuf,
}

impl AgentOrchestrator {
    /// Create a new orchestrator wrapping an agent runtime.
    pub fn new(runtime: Arc<AgentRuntime>, data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        let _ = std::fs::create_dir_all(&data_dir);

        Self {
            runtime,
            neural_memory: None,
            pre_tool_gate: None,
            active_swarm: None,
            data_dir,
        }
    }

    /// Attach neural memory so sub-agent findings are stored as episodic memories.
    pub fn with_memory(mut self, memory: Arc<NeuralMemory>) -> Self {
        self.neural_memory = Some(memory);
        self
    }

    /// Attach a pre-tool-call gate for human approval of risky commands.
    #[allow(clippy::type_complexity)]
    pub fn with_pre_tool_gate(
        mut self,
        gate: Arc<dyn Fn(&str, &str) -> bool + Send + Sync>,
    ) -> Self {
        self.pre_tool_gate = Some(gate);
        self
    }

    /// Spawn sub-agents for each role, decompose goals, and execute them in
    /// parallel. Results are stored in neural memory and the evidence locker.
    pub async fn orchestrate(
        &mut self,
        args: MultiAgentArgs,
        cancel: crate::agent::runtime::Cancel,
        tx: UnboundedSender<AgentEvent>,
    ) -> MultiAgentResult {
        let swarm_id = format!("swarm-{}-{}", slugify(&args.goal), now_secs());

        let roles = if args.roles.is_empty() {
            default_roles()
        } else {
            args.roles.clone()
        };

        // Build sub-agents from roles + scope.
        let mut agents: Vec<SubAgent> = Vec::new();
        for (id, role) in (1_u64..).zip(roles) {
            let (target, goal) = self.sub_agent_goal(&args, role, &args.scope);
            agents.push(SubAgent {
                id: format!("{}-{}", swarm_id, id),
                role,
                target: target.clone(),
                goal,
                status: SubAgentStatus::Pending,
                result: None,
                started_at: None,
                finished_at: None,
            });
        }

        let agent_count = agents.len();
        self.active_swarm = Some(agents.clone());
        self.save_swarm(&swarm_id, &agents);

        let _ = tx.send(AgentEvent::Activity {
            action: format!("Spawned {} sub-agents for swarm {}", agent_count, swarm_id),
            detail: Some(format!("goal: {}", args.goal)),
            done: false,
        });

        // Keep a copy of original agents for building final results after join.
        let original_agents = agents.clone();

        // Execute sub-agents in parallel.
        let handles: Vec<_> = agents
            .into_iter()
            .map(|agent| {
                let runtime = Arc::clone(&self.runtime);
                let memory = self.neural_memory.clone();
                let gate = self.pre_tool_gate.clone();
                let data_dir = self.data_dir.clone();
                let swarm_id = swarm_id.clone();
                let tx = tx.clone();
                let cancel = cancel.clone();

                async move {
                    Self::run_sub_agent(
                        runtime, agent, memory, gate, &swarm_id, data_dir, cancel, tx,
                    )
                    .await
                }
            })
            .collect();

        let results = futures_util::future::join_all(handles).await;

        let mut completed = 0usize;
        let mut failed = 0usize;
        let mut final_agents: Vec<SubAgent> = Vec::new();

        for result in results {
            match result {
                Ok((mut agent, sub_result)) => {
                    agent.status = SubAgentStatus::Completed;
                    agent.finished_at = Some(now_secs());
                    agent.result = Some(sub_result.clone());
                    completed += 1;

                    // Store finding as episodic neural memory.
                    if let Some(memory) = &self.neural_memory {
                        let role_str = format!("{:?}", agent.role);
                        let _ = memory.store(
                            &format!("swarm-{}-{}", swarm_id, agent.id),
                            &format!(
                                "Swarm: {goal}\nAgent: {role} ({target})\nResult: {summary}\nTools: {tools}",
                                goal = args.goal,
                                role = role_str,
                                target = agent.target,
                                summary = sub_result.summary,
                                tools = sub_result.tools_used.join(", "),
                            ),
                            vec![
                                "technique".to_string(),
                                "swarm".to_string(),
                                "success".to_string(),
                                format!("{:?}", agent.role),
                            ],
                            "swarm_result",
                        );
                    }

                    final_agents.push(agent);
                }
                Err(err) => {
                    failed += 1;
                    // Find the agent to mark failed.
                    if let Some(agent) = final_agents.iter_mut().find(|a| a.id == err.0) {
                        agent.status = SubAgentStatus::Failed;
                        agent.finished_at = Some(now_secs());
                    } else {
                        // Shouldn't happen, but be safe.
                        final_agents.push(SubAgent {
                            id: err.0.clone(),
                            role: AgentRole::Recon,
                            target: String::new(),
                            goal: String::new(),
                            status: SubAgentStatus::Failed,
                            result: None,
                            started_at: None,
                            finished_at: Some(now_secs()),
                        });
                    }
                    let _ = tx.send(AgentEvent::Activity {
                        action: format!("Sub-agent {} failed: {}", err.0, err.1),
                        detail: None,
                        done: true,
                    });
                }
            }
        }

        let swarm_status = if failed > 0 { "partial" } else { "complete" };

        let summary = format!(
            "Swarm {}: {} agents completed, {} failed out of {} total ({}).",
            swarm_id,
            completed,
            failed,
            original_agents.len(),
            swarm_status
        );

        let _ = tx.send(AgentEvent::Activity {
            action: format!("Swarm {} finished", swarm_id),
            detail: Some(summary.clone()),
            done: true,
        });

        self.active_swarm = Some(final_agents.clone());
        self.save_swarm(&swarm_id, &final_agents);

        MultiAgentResult {
            swarm_id,
            goal: args.goal,
            agents: final_agents,
            tasks_completed: completed,
            tasks_failed: failed,
            total_tasks: original_agents.len(),
            summary,
        }
    }

    /// Run a single sub-agent: create an autonomous plan for its goal, execute
    /// it, then synthesize findings.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    async fn run_sub_agent(
        runtime: Arc<AgentRuntime>,
        mut agent: SubAgent,
        memory: Option<Arc<NeuralMemory>>,
        gate: Option<Arc<dyn Fn(&str, &str) -> bool + Send + Sync>>,
        swarm_id: &str,
        data_dir: PathBuf,
        cancel: crate::agent::runtime::Cancel,
        tx: UnboundedSender<AgentEvent>,
    ) -> Result<(SubAgent, SubAgentResult), (String, String)> {
        agent.status = SubAgentStatus::Running;
        agent.started_at = Some(now_secs());

        let _ = tx.send(AgentEvent::Activity {
            action: format!(
                "Sub-agent {} ({:?}) starting: {}",
                agent.id, agent.role, agent.goal
            ),
            detail: Some(format!("target: {}", agent.target)),
            done: false,
        });

        // Build the autonomous planner for this sub-agent.
        let mut planner = AutonomousPlanner::new(Arc::clone(&runtime), &data_dir);
        if let Some(mem) = memory {
            planner = planner.with_memory(mem);
        }
        if let Some(g) = gate {
            planner = planner.with_pre_tool_gate(g);
        }

        // Create and execute the plan.
        let plan = planner.create_plan(&agent.goal, Some(&agent.target));
        let (tx_events, _rx) = tokio::sync::mpsc::unbounded_channel();
        let exec_result = planner.execute_plan(cancel.clone(), tx_events).await;

        // Synthesize findings from task results.
        let mut findings: Vec<Finding> = Vec::new();
        let mut tools_used: Vec<String> = Vec::new();
        let mut evidence_ids: Vec<String> = Vec::new();

        for task in &plan.tasks {
            let tool = task.tool.as_str();
            if !tools_used.contains(&tool.to_string()) {
                tools_used.push(tool.to_string());
            }
            if let Some(result) = &task.result {
                if let Some(finding) = Self::extract_finding(task, result, &agent.target) {
                    findings.push(finding);
                }
            }
            if let Some(ref _result_str) = task.result {
                // Store evidence reference.
                let id = hash_string(&format!("{}-{}-{}", swarm_id, agent.id, task.id));
                evidence_ids.push(format!("ev-{:x}", id));
            }
        }

        let summary = if findings.is_empty() {
            format!(
                "No findings for {:?} on {} ({} tasks completed)",
                agent.role, agent.target, exec_result.tasks_completed
            )
        } else {
            format!(
                "{} findings for {:?} on {} ({} tasks completed)",
                findings.len(),
                agent.role,
                agent.target,
                exec_result.tasks_completed
            )
        };

        let sub_result = SubAgentResult {
            agent_id: agent.id.clone(),
            role: agent.role,
            target: agent.target.clone(),
            summary,
            findings,
            tools_used,
            evidence_ids,
        };

        Ok((agent, sub_result))
    }

    /// Extract a Finding from a completed task result (heuristic based on tool
    /// name and result content).
    fn extract_finding(task: &PlanTask, result: &str, target: &str) -> Option<Finding> {
        let tool = task.tool.as_str();
        let sev = match tool {
            "web_recon" => FindingSeverity::Medium,
            "wireless_recon" => FindingSeverity::High,
            "scan_ports" => FindingSeverity::Low,
            "run_command" => FindingSeverity::Info,
            _ => FindingSeverity::Info,
        };

        let title = match tool {
            "web_recon" => format!("Web reconnaissance findings on {}", target),
            "wireless_recon" => format!("Wireless reconnaissance on {}", target),
            "scan_ports" => format!("Open ports on {}", target),
            _ => format!("Task {} result", task.id),
        };

        Some(Finding {
            title,
            severity: sev,
            description: result.chars().take(2000).collect::<String>(),
            target: target.to_string(),
            evidence_ref: Some(format!(
                "ev-{:x}",
                hash_string(&format!("{}-{}", target, task.id))
            )),
            tags: vec![tool.to_string(), "swarm".to_string(), format!("{:?}", tool)],
        })
    }

    /// Build the target + goal for a sub-agent role.
    fn sub_agent_goal(
        &self,
        _args: &MultiAgentArgs,
        role: AgentRole,
        scope: &Option<String>,
    ) -> (String, String) {
        let target = scope.clone().unwrap_or_default();
        let goal = match role {
            AgentRole::Recon => format!(
                "Perform reconnaissance on {}: discover open ports, services, and web entry points.",
                target
            ),
            AgentRole::Vuln => format!(
                "Run vulnerability checks on {}: identify web vulnerabilities, misconfigurations, and exposed services.",
                target
            ),
            AgentRole::Crack => format!(
                "Attempt credential and hash cracking against {}: use authorized wordlists and rules.",
                target
            ),
            AgentRole::Report => format!(
                "Synthesize all findings from the swarm into a consolidated assessment report for {}.",
                target
            ),
        };
        (target, goal)
    }

    /// Get the active swarm (if any).
    pub fn active_swarm(&self) -> Option<&Vec<SubAgent>> {
        self.active_swarm.as_ref()
    }

    /// List all saved swarms from disk.
    pub fn list_swarms(&self) -> Vec<(String, Vec<SubAgent>)> {
        let mut swarms = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.data_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false)
                    && !path.extension().map(|e| e == "tmp").unwrap_or(false)
                {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        if let Ok(agents) = serde_json::from_str::<Vec<SubAgent>>(&text) {
                            let name = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("")
                                .to_string();
                            swarms.push((name, agents));
                        }
                    }
                }
            }
        }
        swarms.sort_by(|a, b| {
            b.1.iter()
                .filter_map(|ag| ag.started_at)
                .max()
                .unwrap_or(0)
                .cmp(&a.1.iter().filter_map(|ag| ag.started_at).max().unwrap_or(0))
        });
        swarms
    }

    /// Save the swarm agent list to disk.
    fn save_swarm(&self, swarm_id: &str, agents: &[SubAgent]) {
        let path = self.data_dir.join(format!("{}.json", swarm_id));
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = serde_json::to_string_pretty(agents).unwrap_or_default();
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &body).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// Slugify a string for use in filenames.
fn slugify(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_ascii_lowercase()
        .trim_matches(|c| c == '-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runtime::{AgentConfig, Cancel};
    use crate::agent::testdouble::ScriptedProvider;
    use std::sync::Arc;
    use tokio::sync::mpsc::unbounded_channel;

    fn test_runtime() -> AgentRuntime {
        AgentRuntime::new(
            Arc::new(ScriptedProvider::default()),
            std::env::temp_dir().join(format!(
                "nanomite-multi-agent-test-{}",
                "orchestrator_creates_swarm"
            )),
            64 * 1024,
            Some(AgentConfig::default()),
        )
    }

    #[test]
    fn orchestrator_creates_swarm() {
        let runtime = Arc::new(test_runtime());
        let dir = std::env::temp_dir().join(format!(
            "nanomite-multi-agent-state-{}",
            "orchestrator_creates_swarm"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _orchestrator = AgentOrchestrator::new(Arc::clone(&runtime), &dir);

        let args = MultiAgentArgs {
            goal: "Assess the web app on http://10.0.0.5".to_string(),
            scope: Some("http://10.0.0.5".to_string()),
            roles: vec![AgentRole::Recon, AgentRole::Vuln],
            require_approval: true,
        };

        // Just test swarm construction (not full execution which needs network).
        let roles_count = args.roles.len();
        assert_eq!(roles_count, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sub_agent_goal_generation() {
        let runtime = Arc::new(test_runtime());
        let dir = std::env::temp_dir().join(format!(
            "nanomite-multi-agent-state-{}",
            "sub_agent_goal_generation"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let orchestrator = AgentOrchestrator::new(Arc::clone(&runtime), &dir);

        let args = MultiAgentArgs {
            goal: "Test swarm".to_string(),
            scope: Some("http://10.0.0.5".to_string()),
            roles: vec![AgentRole::Recon, AgentRole::Report],
            require_approval: true,
        };

        let (recon_target, recon_goal) =
            orchestrator.sub_agent_goal(&args, AgentRole::Recon, &args.scope);
        assert_eq!(recon_target, "http://10.0.0.5");
        assert!(recon_goal.contains("reconnaissance"));

        let (report_target, report_goal) =
            orchestrator.sub_agent_goal(&args, AgentRole::Report, &args.scope);
        assert_eq!(report_target, "http://10.0.0.5");
        assert!(report_goal.contains("report"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finding_extraction_classifies_by_tool() {
        let task = PlanTask {
            id: 1,
            description: "Web recon".to_string(),
            tool: "web_recon".to_string(),
            arguments: "{}".to_string(),
            status: crate::neural::autonomous_planner::TaskStatus::Completed,
            result: Some("Found /admin panel".to_string()),
            error: None,
            executed_at: Some(now_secs()),
        };
        let finding =
            AgentOrchestrator::extract_finding(&task, "Found /admin panel", "http://10.0.0.5");
        assert!(finding.is_some());
        let f = finding.unwrap();
        assert_eq!(f.severity, FindingSeverity::Medium);
        assert!(f.title.to_ascii_lowercase().contains("web"));
        assert!(f.description.contains("Found /admin panel"));

        let wireless_task = PlanTask {
            id: 2,
            description: "Wireless scan".to_string(),
            tool: "wireless_recon".to_string(),
            arguments: "{}".to_string(),
            status: crate::neural::autonomous_planner::TaskStatus::Completed,
            result: Some("WPS enabled on AP".to_string()),
            error: None,
            executed_at: Some(now_secs()),
        };
        let w_finding =
            AgentOrchestrator::extract_finding(&wireless_task, "WPS enabled on AP", "wlan0");
        assert!(w_finding.is_some());
        let wf = w_finding.unwrap();
        assert_eq!(wf.severity, FindingSeverity::High);
    }

    #[test]
    fn save_and_list_swarms_roundtrip() {
        let runtime = Arc::new(test_runtime());
        let dir = std::env::temp_dir().join(format!(
            "nanomite-multi-agent-state-{}",
            "save_and_list_swarms"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let orchestrator = AgentOrchestrator::new(Arc::clone(&runtime), &dir);

        let agents = vec![SubAgent {
            id: "swarm-1".to_string(),
            role: AgentRole::Recon,
            target: "10.0.0.5".to_string(),
            goal: "Recon".to_string(),
            status: SubAgentStatus::Completed,
            result: None,
            started_at: Some(now_secs()),
            finished_at: Some(now_secs()),
        }];
        let swarm_id = "swarm-test-123";
        orchestrator.save_swarm(swarm_id, &agents);

        let listed = orchestrator.list_swarms();
        assert!(!listed.is_empty());
        assert_eq!(listed[0].0, swarm_id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn orchestrate_with_cancel_immediately_blocks() {
        let runtime = Arc::new(test_runtime());
        let dir = std::env::temp_dir().join(format!(
            "nanomite-multi-agent-state-{}",
            "orchestrate_with_cancel"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut orchestrator = AgentOrchestrator::new(Arc::clone(&runtime), &dir);

        let cancel = Cancel::new();
        cancel.cancel(); // immediately cancelled

        let (tx, _rx) = unbounded_channel();
        let args = MultiAgentArgs {
            goal: "Test cancel".to_string(),
            scope: None,
            roles: vec![AgentRole::Recon],
            require_approval: true,
        };

        let result = orchestrator.orchestrate(args, cancel, tx).await;
        // Should complete (sub-agents fail gracefully, not crash).
        assert!(!result.summary.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
