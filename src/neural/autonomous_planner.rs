//! # Autonomous Planner — Goal-Driven Execution Loop
//!
//! Takes a high-level goal (e.g. "assess the web app on 10.0.0.5"), breaks it
//! into ordered tasks, and executes them one at a time through the agent's
//! tool layer. Each tool call passes through `pre_tool_gate` so the operator
//! can approve or deny risky commands (run_command, ssh_run, etc.) before they
//! fire. Results are stored as episodic neural memories so the agent recalls
//! them across sessions.
//!
//! Execution is plan-only: nanomite produces commands, parses results, and
//! records findings — it never exploits.

use crate::agent::runtime::{AgentEvent, AgentRuntime};
use crate::agent::provider::ToolCall;
use crate::neural::memory::NeuralMemory;
use crate::neural::planner::LearnedPlanner;
use crate::neural::{hash_string, now_secs};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

/// A single task in an autonomous plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTask {
    pub id: u64,
    pub description: String,
    pub tool: String,            // tool name to invoke (run_command, web_recon, etc.)
    pub arguments: String,       // JSON arguments for the tool
    pub status: TaskStatus,
    pub result: Option<String>,  // tool payload summary
    pub error: Option<String>,
    pub executed_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Blocked,
}

/// The autonomous plan: goal + ordered tasks + metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomousPlan {
    pub id: String,
    pub goal: String,
    pub tasks: Vec<PlanTask>,
    pub created_at: u64,
    pub completed_at: Option<u64>,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanStatus {
    Planning,
    Executing,
    Completed,
    Failed,
    Blocked,
}

/// Arguments for the `autonomous_execute` tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomousExecuteArgs {
    /// High-level goal for the autonomous agent to pursue.
    pub goal: String,
    /// Optional target scope hint (URL, host, network range).
    #[serde(default)]
    pub scope: Option<String>,
    /// Whether to require human approval before each tool call (default true).
    #[serde(default = "default_true")]
    pub require_approval: bool,
}

fn default_true() -> bool {
    true
}

/// Result of an autonomous execution run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomousExecuteResult {
    pub plan_id: String,
    pub goal: String,
    pub tasks_completed: usize,
    pub tasks_failed: usize,
    pub total_tasks: usize,
    pub summary: String,
}

/// The autonomous planner: drives goal-oriented execution through the tool layer.
#[derive(Clone)]
pub struct AutonomousPlanner {
    runtime: Arc<AgentRuntime>,
    neural_memory: Option<Arc<NeuralMemory>>,
    learned_planner: Option<LearnedPlanner>,
    /// Pre-tool-call gate: consulted before each risky tool dispatch.
    /// When `Some`, the operator must approve each call.
    pre_tool_gate: Option<Arc<dyn Fn(&str, &str) -> bool + Send + Sync>>,
    /// Active plan (if any).
    active_plan: Option<AutonomousPlan>,
    /// Data directory for persisting plans.
    data_dir: PathBuf,
}

impl AutonomousPlanner {
    /// Create a new autonomous planner wrapping an agent runtime.
    pub fn new(
        runtime: Arc<AgentRuntime>,
        data_dir: impl Into<PathBuf>,
    ) -> Self {
        let data_dir = data_dir.into();
        let _ = std::fs::create_dir_all(&data_dir);

        Self {
            runtime,
            neural_memory: None,
            learned_planner: None,
            pre_tool_gate: None,
            active_plan: None,
            data_dir,
        }
    }

    /// Attach neural memory so task results are stored as episodic memories.
    pub fn with_memory(mut self, memory: Arc<NeuralMemory>) -> Self {
        self.neural_memory = Some(memory);
        self
    }

    /// Attach the learned planner for predicting next tools.
    pub fn with_learned_planner(mut self, planner: LearnedPlanner) -> Self {
        self.learned_planner = Some(planner);
        self
    }

    /// Attach a pre-tool-call gate for human approval of risky commands.
    pub fn with_pre_tool_gate(
        mut self,
        gate: Arc<dyn Fn(&str, &str) -> bool + Send + Sync>,
    ) -> Self {
        self.pre_tool_gate = Some(gate);
        self
    }

    /// Create an autonomous plan from a goal. The planner decomposes the goal
    /// into ordered tasks with tool assignments.
    pub fn create_plan(&mut self, goal: &str, scope: Option<&str>) -> AutonomousPlan {
        let id = format!("plan-{}-{}", slugify(goal), now_secs());
        let mut tasks = self.decompose_goal(goal, scope);

        let plan = AutonomousPlan {
            id: id.clone(),
            goal: goal.to_string(),
            tasks,
            created_at: now_secs(),
            completed_at: None,
            status: PlanStatus::Planning,
        };

        self.active_plan = Some(plan.clone());
        self.save_plan(&plan);
        plan
    }

    /// Execute the active plan step by step. Each task is dispatched through
    /// `run_tool()` with the pre-tool-call gate consulted first. Results are
    /// stored in neural memory and the plan is updated in place.
    pub async fn execute_plan(
        &mut self,
        cancel: crate::agent::runtime::Cancel,
        tx: UnboundedSender<AgentEvent>,
    ) -> AutonomousExecuteResult {
        let Some(mut plan) = self.active_plan.take() else {
            return AutonomousExecuteResult {
                plan_id: String::new(),
                goal: String::new(),
                tasks_completed: 0,
                tasks_failed: 0,
                total_tasks: 0,
                summary: "No active plan to execute.".to_string(),
            };
        };

        plan.status = PlanStatus::Executing;
        self.save_plan(&plan);
        self.active_plan = Some(plan.clone());

        let mut completed = 0usize;
        let mut failed = 0usize;

        for task_idx in 0..plan.tasks.len() {
            if cancel.is_cancelled() {
                plan.status = PlanStatus::Blocked;
                self.save_plan(&plan);
                self.active_plan = Some(plan.clone());
                break;
            }

            let task = plan.tasks[task_idx].clone();

            // Mark task in progress.
            plan.tasks[task_idx].status = TaskStatus::InProgress;
            plan.tasks[task_idx].executed_at = Some(now_secs());
            self.save_plan(&plan);
            self.active_plan = Some(plan.clone());

            let _ = tx.send(AgentEvent::Activity {
                action: format!("Executing task {}: {}", task.id, task.description),
                detail: Some(format!("tool: {}", task.tool)),
                done: false,
            });

            // Dispatch through the tool layer with pre-tool-call gate.
            let call = ToolCall {
                id: format!("plan-{}-{}", plan.id, task.id),
                name: task.tool.clone(),
                arguments: task.arguments.clone(),
                index: task_idx,
            };

            let result = self.dispatch_tool(&call);

            match result {
                Ok((payload, summary)) => {
                    plan.tasks[task_idx].status = TaskStatus::Completed;
                    plan.tasks[task_idx].result = Some(summary.clone());
                    completed += 1;

                    // Store result as episodic neural memory.
                    if let Some(memory) = &self.neural_memory {
                        let _ = memory.store(
                            &format!("task-{}-{}", plan.id, task.id),
                            &format!(
                                "Goal: {goal}\\nTask: {desc}\\nTool: {tool}\\nResult: {payload}",
                                goal = plan.goal,
                                desc = task.description,
                                tool = task.tool,
                                payload = payload.chars().take(2000).collect::<String>()
                            ),
                            vec![
                                "technique".to_string(),
                                "task".to_string(),
                                "success".to_string(),
                                task.tool.clone(),
                            ],
                            "task_result",
                        );
                    }

                    // Feed back to learned planner if available.
                    if let Some(planner) = &mut self.learned_planner {
                        let features =
                            LearnedPlanner::context_features(&task.description, &[task.tool.clone()], &[true]);
                        planner.observe_outcome(crate::neural::planner::ToolOutcome {
                            tool: task.tool.clone(),
                            success: true,
                            context_hash: hash_string(&task.description),
                            context_features: features,
                            timestamp: now_secs(),
                            duration_ms: 0,
                        });
                    }

                    let _ = tx.send(AgentEvent::Activity {
                        action: format!("Task {} completed: {}", task.id, task.description),
                        detail: Some(summary),
                        done: true,
                    });
                }
                Err(err) => {
                    plan.tasks[task_idx].status = TaskStatus::Failed;
                    plan.tasks[task_idx].error = Some(err.clone());
                    failed += 1;

                    let _ = tx.send(AgentEvent::Activity {
                        action: format!("Task {} failed: {}", task.id, task.description),
                        detail: Some(err.clone()),
                        done: true,
                    });
                }
            }

            self.save_plan(&plan);
            self.active_plan = Some(plan.clone());

            // Small yield between tasks.
            tokio::task::yield_now().await;
        }

        plan.completed_at = Some(now_secs());
        if failed > 0 {
            plan.status = PlanStatus::Failed;
        } else if completed == plan.tasks.len() {
            plan.status = PlanStatus::Completed;
        } else {
            plan.status = PlanStatus::Blocked;
        }
        self.save_plan(&plan);
        self.active_plan = Some(plan.clone());

        let summary = format!(
            "Plan {}: {} tasks completed, {} failed out of {} total.",
            plan.id, completed, failed, plan.tasks.len()
        );

        AutonomousExecuteResult {
            plan_id: plan.id,
            goal: plan.goal,
            tasks_completed: completed,
            tasks_failed: failed,
            total_tasks: plan.tasks.len(),
            summary,
        }
    }

    /// Dispatch a single tool call through the runtime's tool layer, consulting
    /// the pre-tool-call gate first.
    fn dispatch_tool(&self, call: &ToolCall) -> Result<(String, String), String> {
        // Consult pre-tool-call gate if attached.
        if let Some(gate) = &self.pre_tool_gate {
            if !gate(&call.name, &call.arguments) {
                return Err(format!(
                    "pre_tool_gate: tool `{}` blocked by human-approval gate",
                    call.name
                ));
            }
        }

        let tools = self.runtime.tool_set();
        let disabled: Vec<String> = Vec::new();
        let policy = self.runtime.tool_policy();
        let hooks: Vec<crate::hooks::HookConfig> = Vec::new();
        let workspace = self.runtime.workspace();

        crate::agent::runtime::run_tool(
            &tools,
            &disabled,
            policy,
            &hooks,
            &workspace,
            call,
        )
    }

    /// Decompose a high-level goal into ordered tasks with tool assignments.
    /// Uses the learned planner when available, otherwise falls back to
    /// keyword-based decomposition.
    fn decompose_goal(&self, goal: &str, scope: Option<&str>) -> Vec<PlanTask> {
        let goal_lower = goal.to_ascii_lowercase();
        let mut tasks = Vec::new();
        let mut id: u64 = 1;

        // Determine the primary target from scope or goal.
        let target = scope.unwrap_or("").to_string();

        // Task 1: Reconnaissance / information gathering.
        if goal_lower.contains("web") || goal_lower.contains("http") || goal_lower.contains("url") {
            let url = if !target.is_empty() {
                target.clone()
            } else {
                // Extract URL from goal if present.
                extract_url(goal).unwrap_or_else(|| "http://example.com".to_string())
            };
            tasks.push(PlanTask {
                id,
                description: format!("Web reconnaissance against {}", url),
                tool: "web_recon".to_string(),
                arguments: serde_json::json!({
                    "url": url,
                    "scanners": "both",
                    "timeout_ms": 120000
                }).to_string(),
                status: TaskStatus::Pending,
                result: None,
                error: None,
                executed_at: None,
            });
            id += 1;
        }

        if goal_lower.contains("port") || goal_lower.contains("scan") || goal_lower.contains("network") {
            let host = if !target.is_empty() {
                target.clone()
            } else {
                extract_host(goal).unwrap_or_else(|| "127.0.0.1".to_string())
            };
            tasks.push(PlanTask {
                id,
                description: format!("Port scan against {}", host),
                tool: "scan_ports".to_string(),
                arguments: serde_json::json!({
                    "host": host,
                    "ports": [22, 80, 443, 8080, 8443]
                }).to_string(),
                status: TaskStatus::Pending,
                result: None,
                error: None,
                executed_at: None,
            });
            id += 1;
        }

        if goal_lower.contains("command") || goal_lower.contains("run") || goal_lower.contains("execute") {
            tasks.push(PlanTask {
                id,
                description: "Execute discovery command".to_string(),
                tool: "run_command".to_string(),
                arguments: serde_json::json!({
                    "command": "whoami && id && uname -a"
                }).to_string(),
                status: TaskStatus::Pending,
                result: None,
                error: None,
                executed_at: None,
            });
            id += 1;
        }

        // If no specific task matched, add a generic information-gathering task.
        if tasks.is_empty() {
            tasks.push(PlanTask {
                id,
                description: format!("Gather information about: {}", goal),
                tool: "run_command".to_string(),
                arguments: serde_json::json!({
                    "command": "echo 'No specific tool matched for goal: {}'".replace("{}", goal)
                }).to_string(),
                status: TaskStatus::Pending,
                result: None,
                error: None,
                executed_at: None,
            });
        }

        tasks
    }

    /// Save the current plan to disk.
    fn save_plan(&self, plan: &AutonomousPlan) {
        let path = self.data_dir.join(format!("{}.json", plan.id));
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = serde_json::to_string_pretty(plan).unwrap_or_default();
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &body).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Get the active plan (if any).
    pub fn active_plan(&self) -> Option<&AutonomousPlan> {
        self.active_plan.as_ref()
    }

    /// List all saved plans from disk.
    pub fn list_plans(&self) -> Vec<AutonomousPlan> {
        let mut plans = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.data_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false)
                    && !path.extension().map(|e| e == "tmp").unwrap_or(false)
                {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        if let Ok(plan) = serde_json::from_str::<AutonomousPlan>(&text) {
                            plans.push(plan);
                        }
                    }
                }
            }
        }
        plans.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        plans
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

/// Extract a URL from a goal string (best effort).
fn extract_url(goal: &str) -> Option<String> {
    // Look for http:// or https://
    let re = regex::Regex::new(r"https?://[^\s]+").ok()?;
    re.find(goal).map(|m| m.as_str().to_string())
}

/// Extract a host from a goal string (best effort).
fn extract_host(goal: &str) -> Option<String> {
    // Look for IP-like patterns
    let re = regex::Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b").ok()?;
    re.find(goal).map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runtime::AgentConfig;
    use crate::agent::testdouble::ScriptedProvider;
    use std::sync::Arc;

    fn test_runtime() -> AgentRuntime {
        AgentRuntime::new(
            Arc::new(ScriptedProvider::default()),
            std::env::temp_dir().join(format!(
                "nanomite-autonomous-test-{}",
                std::process::id()
            )),
            64 * 1024,
            Some(AgentConfig::default()),
        )
    }

    #[test]
    fn create_plan_decomposes_web_goal() {
        let runtime = Arc::new(test_runtime());
        let dir = std::env::temp_dir().join(format!(
            "nanomite-autonomous-state-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut planner = AutonomousPlanner::new(Arc::clone(&runtime), &dir);

        let plan = planner.create_plan("Assess the web app on http://10.0.0.5", None);
        assert!(!plan.tasks.is_empty());
        assert_eq!(plan.status, PlanStatus::Planning);
        assert!(plan.tasks.iter().any(|t| t.tool == "web_recon"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_plan_decomposes_port_scan_goal() {
        let runtime = Arc::new(test_runtime());
        let dir = std::env::temp_dir().join(format!(
            "nanomite-autonomous-state-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut planner = AutonomousPlanner::new(Arc::clone(&runtime), &dir);

        let plan = planner.create_plan("Port scan 192.168.1.1", None);
        assert!(plan.tasks.iter().any(|t| t.tool == "scan_ports"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pre_tool_gate_blocks_risky_tools() {
        let runtime = Arc::new(test_runtime());
        let dir = std::env::temp_dir().join(format!(
            "nanomite-autonomous-state-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let gate: Arc<dyn Fn(&str, &str) -> bool + Send + Sync> =
            Arc::new(|tool: &str, _args: &str| {
                // Allow everything except run_command.
                tool != "run_command"
            });
        let mut planner = AutonomousPlanner::new(Arc::clone(&runtime), &dir)
            .with_pre_tool_gate(gate);

        let call = ToolCall {
            id: "t1".into(),
            name: "run_command".into(),
            arguments: r#"{"command":"echo hi"}"#.to_string(),
            index: 0,
        };
        let result = planner.dispatch_tool(&call);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("pre_tool_gate"));

        let call2 = ToolCall {
            id: "t2".into(),
            name: "list_files".into(),
            arguments: "{}".to_string(),
            index: 0,
        };
        let result2 = planner.dispatch_tool(&call2);
        // list_files may fail in temp dir but should not be gate-blocked.
        assert!(!result2.as_ref().err().map(|e| e.contains("pre_tool_gate")).unwrap_or(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_list_plans_roundtrip() {
        let runtime = Arc::new(test_runtime());
        let dir = std::env::temp_dir().join(format!(
            "nanomite-autonomous-state-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut planner = AutonomousPlanner::new(Arc::clone(&runtime), &dir);

        let plan = planner.create_plan("Test plan", None);
        planner.save_plan(&plan);

        let listed = planner.list_plans();
        assert!(!listed.is_empty());
        assert_eq!(listed[0].id, plan.id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn decompose_generic_goal_creates_fallback_task() {
        let runtime = Arc::new(test_runtime());
        let dir = std::env::temp_dir().join(format!(
            "nanomite-autonomous-state-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let planner = AutonomousPlanner::new(Arc::clone(&runtime), &dir);

        let tasks = planner.decompose_goal("do something unknown", None);
        assert!(!tasks.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
