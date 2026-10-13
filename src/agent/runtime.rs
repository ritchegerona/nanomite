use crate::agent::provider::{
    AgentProvider, ChatMessage, DynError, Role, StreamEvent, ToolCall, ToolCallAccumulator, Usage,
};
use crate::tools::exec_tools::{RunCommandArgs, SshRunArgs};
use crate::tools::file_tools::{
    EditFileArgs, ListFilesArgs, ReadFileArgs, SearchArgs, WriteFileArgs,
};
use crate::tools::web_tools::{WebFetchArgs, WebSearchArgs};
use crate::tools::ToolSet;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc::UnboundedSender, Notify};

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub max_steps: usize,
    pub step_timeout_ms: u64,
    pub max_tool_calls: usize,
    pub max_message_chars: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps: 12,
            step_timeout_ms: 120_000,
            max_tool_calls: 24,
            max_message_chars: 200_000,
        }
    }
}

/// Everything the UI needs to know about a turn, as it happens.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Started,
    Delta(String),
    ToolStart {
        name: String,
        detail: String,
    },
    ToolEnd {
        name: String,
        detail: String,
        ok: bool,
    },
    Finished {
        text: String,
        usage: Usage,
        elapsed: Duration,
    },
    Interrupted,
    Failed(String),
}

/// Cooperative cancellation shared between the UI thread and the running agent.
#[derive(Clone, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    pub async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

#[derive(Clone)]
pub struct AgentRuntime {
    provider: Arc<dyn AgentProvider>,
    tools: Arc<ToolSet>,
    tools_schema: Arc<Vec<crate::agent::provider::Tool>>,
    system_prompt: String,
    config: AgentConfig,
}

impl AgentRuntime {
    pub fn new(
        provider: Arc<dyn AgentProvider>,
        workspace: impl Into<std::path::PathBuf>,
        max_file_bytes: usize,
        config: Option<AgentConfig>,
    ) -> Self {
        let workspace: std::path::PathBuf = workspace.into();
        let tools = Arc::new(ToolSet::new(workspace.clone(), max_file_bytes));
        Self {
            provider,
            tools,
            tools_schema: Arc::new(crate::agent::openai::tool_schemas()),
            system_prompt: crate::agent::openai::system_prompt(&workspace.display().to_string()),
            config: config.unwrap_or_default(),
        }
    }

    pub fn provider(&self) -> &Arc<dyn AgentProvider> {
        &self.provider
    }

    /// Runs one conversational turn, streaming progress to `tx`.
    pub async fn run(
        &self,
        user_input: String,
        history: &[ChatMessage],
        cancel: Cancel,
        tx: UnboundedSender<AgentEvent>,
    ) -> Option<Usage> {
        let started = Instant::now();
        let _ = tx.send(AgentEvent::Started);

        let mut messages = vec![ChatMessage::system(self.system_prompt.clone())];
        messages.extend_from_slice(history);
        messages.push(ChatMessage::user(user_input));

        let usage = std::cell::Cell::new(Usage::default());
        let mut tool_calls_made = 0usize;
        let mut steps = 0usize;

        loop {
            if cancel.is_cancelled() {
                let _ = tx.send(AgentEvent::Interrupted);
                return None;
            }
            if steps >= self.config.max_steps || tool_calls_made >= self.config.max_tool_calls {
                let note = if steps >= self.config.max_steps {
                    format!("(stopped after {steps} steps)")
                } else {
                    format!("(stopped after {tool_calls_made} tool calls)")
                };
                let _ = tx.send(AgentEvent::Finished {
                    text: note,
                    usage: usage.take(),
                    elapsed: started.elapsed(),
                });
                return None;
            }
            steps += 1;

            let stream = match tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    let _ = tx.send(AgentEvent::Interrupted);
                    return None;
                }
                outcome = with_timeout(
                    self.provider.stream_chat(messages.clone(), Some((*self.tools_schema).clone())),
                    self.config.step_timeout_ms,
                ) => outcome,
            } {
                StepOutcome::Value(stream) => stream,
                StepOutcome::ProviderError(e) => {
                    let _ = tx.send(AgentEvent::Failed(e.to_string()));
                    return None;
                }
                StepOutcome::Elapsed => {
                    let _ = tx.send(AgentEvent::Failed(format!(
                        "step timed out after {}s",
                        self.config.step_timeout_ms / 1000
                    )));
                    return None;
                }
            };

            let mut content = String::new();
            let mut accumulator = ToolCallAccumulator::default();
            let mut failure: Option<String> = None;

            {
                use futures_util::StreamExt;
                let mut stream = stream;
                loop {
                    let next = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => None,
                        item = stream.next() => item,
                    };
                    let Some(item) = next else { break };
                    match item {
                        Ok(StreamEvent::Delta(d)) => {
                            if content.len() < self.config.max_message_chars {
                                content.push_str(&d);
                                if tx.send(AgentEvent::Delta(d)).is_err() {
                                    break;
                                }
                            }
                        }
                        Ok(StreamEvent::ToolCall(fragment)) => accumulator.push(fragment),
                        Ok(StreamEvent::Usage(u)) => {
                            let mut acc = usage.take();
                            acc.add(&u);
                            usage.set(acc);
                        }
                        Ok(StreamEvent::Done) => break,
                        Err(e) => {
                            failure = Some(e.to_string());
                            break;
                        }
                    }
                }
            }

            if cancel.is_cancelled() {
                let _ = tx.send(AgentEvent::Interrupted);
                return None;
            }
            if let Some(err) = failure {
                let _ = tx.send(AgentEvent::Failed(err));
                return None;
            }

            let calls = accumulator.finish();
            let assistant = ChatMessage {
                role: Role::Assistant,
                content: content.clone(),
                tool_calls: (!calls.is_empty()).then(|| calls.clone()),
                tool_call_id: None,
            };
            messages.push(assistant.clone());

            if calls.is_empty() {
                let text = if content.trim().is_empty() {
                    "(empty response)".to_string()
                } else {
                    content
                };
                let _ = tx.send(AgentEvent::Finished {
                    text,
                    usage: usage.take(),
                    elapsed: started.elapsed(),
                });
                return None;
            }

            for call in calls {
                if cancel.is_cancelled() {
                    let _ = tx.send(AgentEvent::Interrupted);
                    return None;
                }
                tool_calls_made += 1;
                let detail = describe_args(&call);
                if tx
                    .send(AgentEvent::ToolStart {
                        name: call.name.clone(),
                        detail,
                    })
                    .is_err()
                {
                    return None;
                }

                // File IO is blocking: keep it off the async runtime's worker threads.
                let tools = Arc::clone(&self.tools);
                let call_for_task = call.clone();
                let outcome = tokio::task::spawn_blocking(move || run_tool(&tools, &call_for_task))
                    .await
                    .unwrap_or_else(|e| Err(format!("tool panicked: {e}")));

                match outcome {
                    Ok((payload, summary)) => {
                        let ok = !payload.starts_with("{\"error\"");
                        let _ = tx.send(AgentEvent::ToolEnd {
                            name: call.name.clone(),
                            detail: summary,
                            ok,
                        });
                        messages.push(ChatMessage::tool_result(call.id.clone(), payload));
                    }
                    Err(e) => {
                        let _ = tx.send(AgentEvent::ToolEnd {
                            name: call.name.clone(),
                            detail: e.clone(),
                            ok: false,
                        });
                        messages.push(ChatMessage::tool_result(call.id.clone(), e));
                    }
                }
            }

            // Give the UI a beat to settle before the next step.
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = tx.send(AgentEvent::Interrupted);
                    return None;
                }
                _ = tokio::time::sleep(Duration::from_millis(120)) => {}
            }
        }
    }
}

enum StepOutcome<T> {
    Value(T),
    ProviderError(DynError),
    Elapsed,
}

async fn with_timeout<F, T>(fut: F, ms: u64) -> StepOutcome<T>
where
    F: std::future::Future<Output = Result<T, DynError>>,
{
    match tokio::time::timeout(Duration::from_millis(ms), fut).await {
        Ok(Ok(v)) => StepOutcome::Value(v),
        Ok(Err(e)) => StepOutcome::ProviderError(e),
        Err(_) => StepOutcome::Elapsed,
    }
}

/// Executes a tool call and returns `(payload for the model, human summary for the UI)`.
fn run_tool(tools: &ToolSet, call: &ToolCall) -> Result<(String, String), String> {
    let args: serde_json::Value = if call.arguments.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&call.arguments)
            .map_err(|e| format!("invalid JSON arguments for {}: {e}", call.name))?
    };

    match call.name.as_str() {
        "list_files" => {
            let parsed: ListFilesArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad list_files arguments: {e}"))?;
            let where_ = parsed.path.clone().unwrap_or_else(|| ".".to_string());
            let result = tools
                .files.list_files(parsed)
                .map_err(|e| format!("list_files failed: {e}"))?;
            let summary = format!(
                "{} {} + {} dirs",
                result.total(),
                if result.total() == 1 { "file" } else { "files" },
                result.directories.len()
            );
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{where_} · {summary}")))
        }
        "read_file" => {
            let parsed: ReadFileArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad read_file arguments: {e}"))?;
            let path = parsed.path.clone();
            let result = tools
                .files.read_file(parsed)
                .map_err(|e| format!("read_file failed: {e}"))?;
            let suffix = if result.trimmed { " (truncated)" } else { "" };
            let summary = format!("{} lines{suffix}", result.lines);
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · {summary}", shorten(&path, 48))))
        }
        "search" => {
            let parsed: SearchArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad search arguments: {e}"))?;
            let query = parsed.query.clone();
            let result = tools
                .files.search(parsed)
                .map_err(|e| format!("search failed: {e}"))?;
            let summary = if result.hits.is_empty() {
                format!("no matches in {} files", result.files_scanned)
            } else {
                format!(
                    "{} hits / {} files",
                    result.hits.len(),
                    result.files_scanned
                )
            };
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · {summary}", shorten(&query, 32))))
        }
        "write_file" => {
            let parsed: WriteFileArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad write_file arguments: {e}"))?;
            let path = parsed.path.clone();
            let result = tools
                .files.write_file(parsed)
                .map_err(|e| format!("write_file failed: {e}"))?;
            let summary = format!("{} bytes written", result.bytes_written);
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · {summary}", shorten(&path, 48))))
        }
        "edit_file" => {
            let parsed: EditFileArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad edit_file arguments: {e}"))?;
            let path = parsed.path.clone();
            let result = tools
                .files.edit_file(parsed)
                .map_err(|e| format!("edit_file failed: {e}"))?;
            let summary = format!("{} replacements made", result.replacements_made);
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · {summary}", shorten(&path, 48))))
        }
        "run_command" => {
            let parsed: RunCommandArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad run_command arguments: {e}"))?;
            let cmd = parsed.command.clone();
            let result = tools
                .exec
                .run_command(parsed)
                .map_err(|e| format!("run_command failed: {e}"))?;
            let summary = if result.timed_out {
                "timed out".to_string()
            } else {
                format!("exit {}", result.exit_code)
            };
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · {summary}", shorten(&cmd, 40))))
        }
        "ssh_run" => {
            let parsed: SshRunArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad ssh_run arguments: {e}"))?;
            let host = parsed.host.clone();
            let cmd = parsed.command.clone();
            let result = tools
                .exec
                .ssh_run(parsed)
                .map_err(|e| format!("ssh_run failed: {e}"))?;
            let summary = if result.timed_out {
                "timed out".to_string()
            } else {
                format!("exit {}", result.exit_code)
            };
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((
                payload,
                format!("{} @ {} · {summary}", shorten(&cmd, 32), shorten(&host, 24)),
            ))
        }
        "web_fetch" => {
            let parsed: WebFetchArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad web_fetch arguments: {e}"))?;
            let url = parsed.url.clone();
            let result = tools
                .web
                .fetch(parsed)
                .map_err(|e| format!("web_fetch failed: {e}"))?;
            let summary = format!(
                "{} {}{}",
                result.status,
                result.title.clone().unwrap_or_default(),
                if result.truncated { " (truncated)" } else { "" }
            );
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · {}", shorten(&url, 40), summary.trim())))
        }
        "web_search" => {
            let parsed: WebSearchArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad web_search arguments: {e}"))?;
            let query = parsed.query.clone();
            let result = tools
                .web
                .search(parsed)
                .map_err(|e| format!("web_search failed: {e}"))?;
            let summary = format!("{} results", result.results.len());
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · {}", shorten(&query, 32), summary)))
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn describe_args(call: &ToolCall) -> String {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(&call.arguments) else {
        return String::new();
    };
    args.get("path")
        .and_then(|p| p.as_str())
        .map(|p| shorten(p, 48))
        .or_else(|| {
            args.get("query")
                .and_then(|q| q.as_str())
                .map(|q| format!("\"{}\"", shorten(q, 32)))
        })
        .unwrap_or_default()
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}
