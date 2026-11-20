//! Tool chaining/pipelines - output of one tool feeds into another.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub tool: String,
    pub args: serde_json::Value,
    pub input_mapping: serde_json::Value,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: uuid::Uuid,
    pub name: String,
    pub steps: Vec<PipelineStep>,
    pub on_failure: String,
}

pub struct PipelineExecutor {
    executor: super::executor::ToolExecutor,
}

impl PipelineExecutor {
    pub fn new() -> Self {
        Self {
            executor: super::executor::ToolExecutor::new(),
        }
    }

    pub async fn execute(&self, pipeline: Pipeline) -> anyhow::Result<Vec<super::executor::ToolExecutionResult>> {
        let mut results = Vec::new();
        let mut previous_output: Option<serde_json::Value> = None;

        for step in &pipeline.steps {
            let mut args = step.args.clone();

            if let Some(ref prev) = previous_output {
                let mut merged = serde_json::json!({ "input": prev });
                if let serde_json::Value::Object(ref mut map) = merged {
                    if let serde_json::Value::Object(ref args_map) = args {
                        for (k, v) in args_map {
                            map.insert(k.clone(), v.clone());
                        }
                    }
                }
                args = merged;
            }

            let request = super::executor::ToolExecutionRequest {
                run_id: uuid::Uuid::new_v4(),
                tool_name: step.tool.clone(),
                module: "pipeline".to_string(),
                team: "shared".to_string(),
                command: step.tool.clone(),
                args: args.as_array().unwrap_or(&vec![]).iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect(),
                env: HashMap::new(),
                working_dir: None,
                timeout_secs: 300,
                max_memory_mb: 1024,
                max_cpu_percent: 50,
            };

            let result = self.executor.execute(request).await?;
            results.push(result.clone());

            previous_output = Some(serde_json::json!({
                "status": result.status,
                "stdout": result.stdout,
                "exit_code": result.exit_code
            }));

            if result.status == "failed" && pipeline.on_failure == "stop" {
                break;
            }
        }

        Ok(results)
    }
}