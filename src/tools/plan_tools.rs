//! Plan/task list tools for structured agent work.
//!
//! Provides a persistent task list the agent can create, update, and track
//! progress against. Integrates with the ToolPolicy: in Plan mode the agent
//! can only read and must use these tools to report its plan before executing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const MAX_DESC_LEN: usize = 500;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    #[default]
    Pending,
    InProgress,
    Done,
    Blocked,
    Cancelled,
}

impl TaskStatus {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pending" => Some(TaskStatus::Pending),
            "in_progress" | "in-progress" | "running" => Some(TaskStatus::InProgress),
            "done" | "completed" | "complete" => Some(TaskStatus::Done),
            "blocked" => Some(TaskStatus::Blocked),
            "cancelled" | "canceled" => Some(TaskStatus::Cancelled),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Done => "done",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: usize,
    pub description: String,
    pub status: TaskStatus,
    pub created_at: String,
    pub updated_at: String,
    pub depends_on: Vec<usize>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub name: String,
    pub description: String,
    pub tasks: Vec<Task>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePlanArgs {
    pub name: String,
    pub description: String,
    pub tasks: Vec<CreateTaskArgs>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskArgs {
    pub description: String,
    pub depends_on: Vec<usize>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePlanResult {
    pub plan: Plan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPlanArgs {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPlanResult {
    pub plan: Option<Plan>,
    pub plans: Vec<Plan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTaskArgs {
    pub task_id: usize,
    pub status: Option<String>,
    pub description: Option<String>,
    pub depends_on: Option<Vec<usize>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTaskResult {
    pub task: Task,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePlanArgs {
    pub name: String,
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePlanResult {
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPlansResult {
    pub plans: Vec<PlanSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSummary {
    pub name: String,
    pub description: String,
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub in_progress_tasks: usize,
    pub updated_at: String,
}

#[derive(Clone)]
pub struct PlanTools {
    root: PathBuf,
    plans: Arc<Mutex<HashMap<String, Plan>>>,
}

impl PlanTools {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            plans: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn cache_dir(&self) -> PathBuf {
        self.root.join(".nanomite").join("plans")
    }

    fn ensure_cache_dir(&self) -> Result<(), String> {
        std::fs::create_dir_all(self.cache_dir())
            .map_err(|e| format!("failed to create plans cache dir: {e}"))
    }

    fn plan_file(&self, name: &str) -> PathBuf {
        self.cache_dir()
            .join(format!("{}.json", sanitize_filename(name)))
    }

    fn load_all_plans(&self) -> Result<(), String> {
        self.ensure_cache_dir()?;
        let mut plans = HashMap::new();
        for entry in std::fs::read_dir(self.cache_dir())
            .map_err(|e| format!("read plans dir: {e}"))?
            .filter_map(|e| e.ok())
        {
            if entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(plan) = serde_json::from_str::<Plan>(&content) {
                        plans.insert(plan.name.clone(), plan);
                    }
                }
            }
        }
        *self.plans.lock().unwrap() = plans;
        Ok(())
    }

    fn save_plan(&self, plan: &Plan) -> Result<(), String> {
        self.ensure_cache_dir()?;
        let path = self.plan_file(&plan.name);
        let data =
            serde_json::to_string_pretty(plan).map_err(|e| format!("serialization failed: {e}"))?;
        std::fs::write(&path, data).map_err(|e| format!("write failed: {e}"))?;
        Ok(())
    }

    fn delete_plan_file(&self, name: &str) -> Result<(), String> {
        let path = self.plan_file(name);
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| format!("delete failed: {e}"))?;
        }
        Ok(())
    }

    fn now_iso() -> String {
        chrono::Utc::now().to_rfc3339()
    }

    pub fn create_plan(&self, args: CreatePlanArgs) -> Result<CreatePlanResult, String> {
        self.load_all_plans()?;

        let mut plans = self.plans.lock().unwrap();
        if plans.contains_key(&args.name) {
            return Err(format!("plan '{}' already exists", args.name));
        }

        let now = Self::now_iso();
        let mut tasks = Vec::new();
        for (i, t) in args.tasks.into_iter().enumerate() {
            if t.description.len() > MAX_DESC_LEN {
                return Err(format!(
                    "task description too long (max {MAX_DESC_LEN} chars)"
                ));
            }
            tasks.push(Task {
                id: i + 1,
                description: t.description,
                status: TaskStatus::Pending,
                created_at: now.clone(),
                updated_at: now.clone(),
                depends_on: t.depends_on,
                tags: t.tags,
            });
        }

        let plan = Plan {
            name: args.name.clone(),
            description: args.description,
            tasks,
            created_at: now.clone(),
            updated_at: now,
        };

        self.save_plan(&plan)?;
        plans.insert(args.name.clone(), plan.clone());

        Ok(CreatePlanResult { plan })
    }

    pub fn get_plan(&self, args: GetPlanArgs) -> Result<GetPlanResult, String> {
        self.load_all_plans()?;
        let plans = self.plans.lock().unwrap();

        let plan = if let Some(name) = args.name {
            plans.get(&name).cloned()
        } else {
            plans.values().max_by_key(|p| p.updated_at.clone()).cloned()
        };

        let all_plans: Vec<Plan> = plans.values().cloned().collect();

        Ok(GetPlanResult {
            plan,
            plans: all_plans,
        })
    }

    pub fn update_task(&self, args: UpdateTaskArgs) -> Result<UpdateTaskResult, String> {
        self.load_all_plans()?;
        let mut plans = self.plans.lock().unwrap();

        // Find the plan containing this task
        for plan in plans.values_mut() {
            if let Some(task) = plan.tasks.iter_mut().find(|t| t.id == args.task_id) {
                if let Some(status_str) = args.status {
                    if let Some(status) = TaskStatus::parse(&status_str) {
                        task.status = status;
                    } else {
                        return Err(format!("invalid status: {}", status_str));
                    }
                }
                if let Some(desc) = args.description {
                    if desc.len() > MAX_DESC_LEN {
                        return Err(format!("description too long (max {MAX_DESC_LEN} chars)"));
                    }
                    task.description = desc;
                }
                if let Some(deps) = args.depends_on {
                    task.depends_on = deps;
                }
                if let Some(tags) = args.tags {
                    task.tags = tags;
                }
                task.updated_at = Self::now_iso();
                plan.updated_at = Self::now_iso();

                let result = task.clone();
                self.save_plan(plan)?;
                return Ok(UpdateTaskResult { task: result });
            }
        }

        Err(format!("task {} not found in any plan", args.task_id))
    }

    pub fn delete_plan(&self, args: DeletePlanArgs) -> Result<DeletePlanResult, String> {
        if !args.confirm {
            return Err("deletion requires confirm=true".into());
        }
        self.load_all_plans()?;
        let mut plans = self.plans.lock().unwrap();

        if plans.remove(&args.name).is_none() {
            return Err(format!("plan '{}' not found", args.name));
        }

        self.delete_plan_file(&args.name)?;
        Ok(DeletePlanResult { deleted: true })
    }

    pub fn list_plans(&self) -> Result<ListPlansResult, String> {
        self.load_all_plans()?;
        let plans = self.plans.lock().unwrap();

        let summaries: Vec<PlanSummary> = plans
            .values()
            .map(|p| {
                let total = p.tasks.len();
                let completed = p
                    .tasks
                    .iter()
                    .filter(|t| t.status == TaskStatus::Done)
                    .count();
                let in_progress = p
                    .tasks
                    .iter()
                    .filter(|t| t.status == TaskStatus::InProgress)
                    .count();
                PlanSummary {
                    name: p.name.clone(),
                    description: p.description.clone(),
                    total_tasks: total,
                    completed_tasks: completed,
                    in_progress_tasks: in_progress,
                    updated_at: p.updated_at.clone(),
                }
            })
            .collect();

        Ok(ListPlansResult { plans: summaries })
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_plan_tools() -> (PathBuf, PlanTools) {
        // Per-test unique dir: the tests write real plan files, and a shared
        // dir races under parallel test execution (list/get see other tests'
        // plans, update_task finds another plan's task id first).
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "nanomite-plan-test-{}-{}-{}",
            std::process::id(),
            id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let tools = PlanTools::new(&dir);
        (dir, tools)
    }

    #[test]
    fn create_plan_works() {
        let (dir, tools) = init_plan_tools();
        let result = tools
            .create_plan(CreatePlanArgs {
                name: "feature-x".into(),
                description: "Add feature X".into(),
                tasks: vec![
                    CreateTaskArgs {
                        description: "Design API".into(),
                        depends_on: vec![],
                        tags: vec!["api".into()],
                    },
                    CreateTaskArgs {
                        description: "Implement backend".into(),
                        depends_on: vec![1],
                        tags: vec!["backend".into()],
                    },
                    CreateTaskArgs {
                        description: "Write tests".into(),
                        depends_on: vec![2],
                        tags: vec!["test".into()],
                    },
                ],
            })
            .unwrap();

        assert_eq!(result.plan.name, "feature-x");
        assert_eq!(result.plan.tasks.len(), 3);
        assert_eq!(result.plan.tasks[0].status, TaskStatus::Pending);
        assert_eq!(result.plan.tasks[1].depends_on, vec![1]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn get_plan_returns_latest_when_no_name() {
        let (dir, tools) = init_plan_tools();
        tools
            .create_plan(CreatePlanArgs {
                name: "plan-a".into(),
                description: "A".into(),
                tasks: vec![],
            })
            .unwrap();
        tools
            .create_plan(CreatePlanArgs {
                name: "plan-b".into(),
                description: "B".into(),
                tasks: vec![],
            })
            .unwrap();

        let result = tools.get_plan(GetPlanArgs { name: None }).unwrap();
        assert!(result.plan.is_some());
        assert_eq!(result.plan.unwrap().name, "plan-b");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_task_status() {
        let (dir, tools) = init_plan_tools();
        tools
            .create_plan(CreatePlanArgs {
                name: "test".into(),
                description: "Test".into(),
                tasks: vec![CreateTaskArgs {
                    description: "Task 1".into(),
                    depends_on: vec![],
                    tags: vec![],
                }],
            })
            .unwrap();

        let result = tools
            .update_task(UpdateTaskArgs {
                task_id: 1,
                status: Some("in_progress".into()),
                description: None,
                depends_on: None,
                tags: None,
            })
            .unwrap();

        assert_eq!(result.task.status, TaskStatus::InProgress);
        // updated_at should be recent (we can't easily test exact time)
        assert!(!result.task.updated_at.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_plans_shows_summary() {
        let (dir, tools) = init_plan_tools();
        tools
            .create_plan(CreatePlanArgs {
                name: "plan-1".into(),
                description: "First".into(),
                tasks: vec![
                    CreateTaskArgs {
                        description: "Done task".into(),
                        depends_on: vec![],
                        tags: vec![],
                    },
                    CreateTaskArgs {
                        description: "Pending task".into(),
                        depends_on: vec![],
                        tags: vec![],
                    },
                ],
            })
            .unwrap();

        // Update one to done
        tools
            .update_task(UpdateTaskArgs {
                task_id: 1,
                status: Some("done".into()),
                description: None,
                depends_on: None,
                tags: None,
            })
            .unwrap();

        let result = tools.list_plans().unwrap();
        assert_eq!(result.plans.len(), 1);
        assert_eq!(result.plans[0].total_tasks, 2);
        assert_eq!(result.plans[0].completed_tasks, 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_plan_requires_confirm() {
        let (dir, tools) = init_plan_tools();
        tools
            .create_plan(CreatePlanArgs {
                name: "to-delete".into(),
                description: "Will be deleted".into(),
                tasks: vec![],
            })
            .unwrap();

        let err = tools
            .delete_plan(DeletePlanArgs {
                name: "to-delete".into(),
                confirm: false,
            })
            .unwrap_err();
        assert!(err.contains("confirm"));

        let result = tools
            .delete_plan(DeletePlanArgs {
                name: "to-delete".into(),
                confirm: true,
            })
            .unwrap();
        assert!(result.deleted);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_plan_rejected() {
        let (dir, tools) = init_plan_tools();
        tools
            .create_plan(CreatePlanArgs {
                name: "dup".into(),
                description: "First".into(),
                tasks: vec![],
            })
            .unwrap();

        let err = tools
            .create_plan(CreatePlanArgs {
                name: "dup".into(),
                description: "Second".into(),
                tasks: vec![],
            })
            .unwrap_err();

        assert!(err.contains("already exists"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn task_dependencies_preserved() {
        let (dir, tools) = init_plan_tools();
        let result = tools
            .create_plan(CreatePlanArgs {
                name: "deps".into(),
                description: "Test deps".into(),
                tasks: vec![
                    CreateTaskArgs {
                        description: "A".into(),
                        depends_on: vec![],
                        tags: vec![],
                    },
                    CreateTaskArgs {
                        description: "B".into(),
                        depends_on: vec![1],
                        tags: vec![],
                    },
                    CreateTaskArgs {
                        description: "C".into(),
                        depends_on: vec![1, 2],
                        tags: vec![],
                    },
                ],
            })
            .unwrap();

        assert_eq!(result.plan.tasks[0].depends_on, Vec::<usize>::new());
        assert_eq!(result.plan.tasks[1].depends_on, vec![1]);
        assert_eq!(result.plan.tasks[2].depends_on, vec![1, 2]);
        let _ = fs::remove_dir_all(&dir);
    }
}
