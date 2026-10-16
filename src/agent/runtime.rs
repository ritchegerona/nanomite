use crate::agent::provider::{
    AgentProvider, ChatMessage, DynError, Role, StreamEvent, ToolCall, ToolCallAccumulator, Usage,
};
use crate::memory::{TurnObservation, TurnOutcome};
use crate::tools::document_tools::ReadDocumentArgs;
use crate::tools::exec_tools::{RunCommandArgs, SshRunArgs};
use crate::tools::file_tools::{
    EditFileArgs, ListFilesArgs, ReadFileArgs, SearchArgs, WriteFileArgs,
};
use crate::tools::memory_tools::{
    MemoryDeleteArgs, MemoryGetArgs, MemoryListArgs, MemorySaveArgs, MemorySearchArgs,
    MemoryUpdateArgs,
};
use crate::tools::web_tools::{WebFetchArgs, WebSearchArgs};
use crate::tools::ToolSet;
use serde_json::json;
use std::path::PathBuf;
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
    /// Labels used only for memory provenance - the runtime works with any
    /// provider, so these never shape behaviour.
    pub provider: String,
    pub model: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps: usize::MAX,
            step_timeout_ms: 120_000,
            max_tool_calls: usize::MAX,
            max_message_chars: 200_000,
            provider: String::new(),
            model: String::new(),
        }
    }
}

/// Everything the UI needs to know about a turn, as it happens.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Started,
    /// One agent step is about to start (`index` is the 1-based step number),
    /// so the UI can show live progress on long tool-using turns.
    Step {
        index: usize,
    },
    Delta(String),
    ToolStart {
        name: String,
        detail: String,
        /// Bounded preview of what the call is about to write/run, built from
        /// its arguments, so the transcript can show the code live.
        preview: Option<String>,
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
    /// Workspace root, remembered so memory retrieval can be project-scoped.
    workspace: PathBuf,
    /// Slugified workspace name: the default project key for memory.
    project: Option<String>,
    /// Stable per-session identifier for memory checkpoints.
    session: String,
    /// Remembered so `with_memory` can rebuild the tool set with memory attached.
    max_file_bytes: usize,
    /// Lowercased tool names disabled by configuration ("tool library"):
    /// filtered out of the schema list and refused at dispatch, so a
    /// hallucinated call to a disabled tool can never run.
    disabled_tools: Arc<Vec<String>>,
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
        let project = crate::memory::model::slugify(
            &workspace
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        Self {
            provider,
            tools,
            tools_schema: Arc::new(crate::agent::openai::tool_schemas()),
            system_prompt: crate::agent::openai::system_prompt(&workspace.display().to_string()),
            config: config.unwrap_or_default(),
            workspace: workspace.clone(),
            project: (!project.is_empty()).then_some(project),
            session: crate::memory::security::new_id(
                crate::memory::model::now(),
                0,
                &format!("session:{}", workspace.display()),
            ),
            max_file_bytes,
            disabled_tools: Arc::new(Vec::new()),
        }
    }

    /// Attaches the persistent memory engine: memory tools become available,
    /// every turn retrieves bounded context before the request, and finished
    /// turns feed the post-task capture policy.
    ///
    /// Memory is provider-independent by construction - nothing here mentions a
    /// model, an endpoint or a prompt format.
    pub fn with_memory(mut self, engine: Arc<crate::memory::MemoryEngine>) -> Self {
        let mut tools = ToolSet::new(self.workspace.clone(), self.max_file_bytes);
        // Both builders rebuild the tool set, so each preserves the other's
        // attachment: call order never silently drops a subsystem.
        tools.improve = self.tools.improve.clone();
        tools.improve_roots = self.tools.improve_roots.clone();
        tools.memory = Some(Arc::clone(&engine));
        self.tools = Arc::new(tools);
        let mut schemas = (*self.tools_schema).clone();
        schemas.extend(crate::tools::memory_tools::memory_tool_schemas());
        self.tools_schema = Arc::new(filter_disabled_schemas(schemas, &self.disabled_tools));
        self.system_prompt.push_str(
            "\n\nPersistent memory is available through the memory_* tools. Search it when \
             context from earlier sessions would help, and save durable facts, decisions \
             and preferences the user states - never credentials or secrets.",
        );
        self
    }

    /// Attaches the self-improvement engine: `propose_self_edit` and
    /// `list_self_edits` become available schemas, validated against the
    /// caller-supplied roots.
    ///
    /// There is deliberately no agent-callable apply. The model can *stage* a
    /// change; a human approves it in the `/improve` overlay, and only the UI's
    /// explicit approve keypress ever calls [`crate::improve::ImproveEngine::apply`].
    pub fn with_improve(
        mut self,
        engine: Arc<crate::improve::ImproveEngine>,
        roots: crate::improve::Roots,
    ) -> Self {
        let mut tools = ToolSet::new(self.workspace.clone(), self.max_file_bytes);
        tools.memory = self.tools.memory.clone();
        tools.improve = Some(Arc::clone(&engine));
        tools.improve_roots = Some(roots.clone());
        self.tools = Arc::new(tools);
        let mut schemas = (*self.tools_schema).clone();
        schemas.extend(crate::tools::improve_tools::improve_tool_schemas());
        self.tools_schema = Arc::new(filter_disabled_schemas(schemas, &self.disabled_tools));
        self.system_prompt.push_str(
            "\n\nYou may propose changes to your own config, prompt file or a workspace \
             source file with propose_self_edit (list_self_edits shows what is staged). \
             A proposal is staged for user review only: the user approves it in /improve, \
             nothing is applied automatically, and you have no tool that applies one.",
        );
        self
    }

    /// Appends the user's custom prompt file to the system prompt. Call after
    /// `with_memory` so the base prompt, the memory notice and then the custom
    /// instructions reach the model in that order.
    ///
    /// The file the agent edits with `propose_self_edit { target: "prompt" }` is
    /// this file: hand-written and agent-proposed instructions are the same loop.
    pub fn with_prompt_file(mut self, text: String) -> Self {
        let text = text.trim();
        if text.is_empty() {
            return self;
        }
        self.system_prompt
            .push_str("\n\n# User instructions (prompt.md)\n\n");
        self.system_prompt.push_str(text);
        self
    }

    /// Configures the disabled-tool list (the "tool library" toggle). Disabled
    /// tools disappear from the schema list the provider sees and are refused
    /// at dispatch, so the model can neither see nor run them. Call in any
    /// order relative to [`Self::with_memory`]: both schema assembly points
    /// re-apply the filter.
    pub fn with_disabled_tools(mut self, disabled: Vec<String>) -> Self {
        self.disabled_tools = Arc::new(crate::config::parse_disabled_tools(&disabled.join(",")));
        self.tools_schema = Arc::new(filter_disabled_schemas(
            (*self.tools_schema).clone(),
            &self.disabled_tools,
        ));
        self
    }

    pub fn provider(&self) -> &Arc<dyn AgentProvider> {
        &self.provider
    }

    /// The live tool set, shared with the approval-path tests so they can drive
    /// a tool call exactly as the turn loop does.
    #[cfg(test)]
    pub(crate) fn tool_set(&self) -> std::sync::Arc<ToolSet> {
        std::sync::Arc::clone(&self.tools)
    }

    /// Appends bounded retrieved memory to the trusted system message, keeping
    /// one system message (widest provider compatibility).
    fn inject_memory_context(&self, messages: &mut [ChatMessage], input: &str) {
        let Some(engine) = &self.tools.memory else {
            return;
        };
        let context = engine.context(input, self.project.as_deref());
        if context.text.is_empty() {
            return;
        }
        if let Some(system) = messages.first_mut() {
            if system.role == Role::System {
                system.content.push_str("\n\n");
                system.content.push_str(&context.text);
            }
        }
    }

    /// Feeds the finished turn to the post-task capture policy. Fire-and-forget:
    /// observation is best-effort by contract and must never delay or fail the
    /// turn it observes.
    fn observe_turn(&self, outcome: TurnOutcome, input: &str, tools_used: &[String]) {
        let Some(engine) = &self.tools.memory else {
            return;
        };
        let observation = TurnObservation {
            user_input: input.chars().take(2_000).collect(),
            outcome,
            tools: tools_used.to_vec(),
            workspace: self.workspace.display().to_string(),
            project: self.project.clone(),
            provider: self.config.provider.clone(),
            model: self.config.model.clone(),
            session: self.session.clone(),
        };
        let engine = Arc::clone(engine);
        tokio::task::spawn_blocking(move || engine.observe(&observation));
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
        messages.push(ChatMessage::user(user_input.clone()));

        // Retrieve bounded memory context for this request. Retrieval never fails
        // the turn, never grants tool permissions, and is framed by the engine as
        // untrusted reference data - distinct from this trusted system prompt.
        self.inject_memory_context(&mut messages, &user_input);

        let usage = std::cell::Cell::new(Usage::default());
        let mut tool_calls_made = 0usize;
        let mut steps = 0usize;
        // Tool names this turn used, in first-use order, for the capture policy.
        let mut tools_used: Vec<String> = Vec::new();

        loop {
            if cancel.is_cancelled() {
                self.observe_turn(TurnOutcome::Interrupted, &user_input, &tools_used);
                let _ = tx.send(AgentEvent::Interrupted);
                return None;
            }
            if steps >= self.config.max_steps || tool_calls_made >= self.config.max_tool_calls {
                let note = if steps >= self.config.max_steps {
                    format!("(stopped after {steps} steps)")
                } else {
                    format!("(stopped after {tool_calls_made} tool calls)")
                };
                self.observe_turn(TurnOutcome::Interrupted, &user_input, &tools_used);
                let _ = tx.send(AgentEvent::Finished {
                    text: note,
                    usage: usage.take(),
                    elapsed: started.elapsed(),
                });
                return None;
            }
            steps += 1;
            let _ = tx.send(AgentEvent::Step { index: steps });

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
                    self.observe_turn(TurnOutcome::Failed, &user_input, &tools_used);
                    let _ = tx.send(AgentEvent::Failed(e.to_string()));
                    return None;
                }
                StepOutcome::Elapsed => {
                    self.observe_turn(TurnOutcome::Failed, &user_input, &tools_used);
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
                self.observe_turn(TurnOutcome::Interrupted, &user_input, &tools_used);
                let _ = tx.send(AgentEvent::Interrupted);
                return None;
            }
            if let Some(err) = failure {
                self.observe_turn(TurnOutcome::Failed, &user_input, &tools_used);
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
                self.observe_turn(TurnOutcome::Completed, &user_input, &tools_used);
                let _ = tx.send(AgentEvent::Finished {
                    text,
                    usage: usage.take(),
                    elapsed: started.elapsed(),
                });
                return None;
            }

            for call in calls {
                if cancel.is_cancelled() {
                    self.observe_turn(TurnOutcome::Interrupted, &user_input, &tools_used);
                    let _ = tx.send(AgentEvent::Interrupted);
                    return None;
                }
                tool_calls_made += 1;
                if !tools_used.iter().any(|name| name == &call.name) {
                    tools_used.push(call.name.clone());
                }
                let detail = describe_args(&call);
                let preview = preview_args(&call);
                if tx
                    .send(AgentEvent::ToolStart {
                        name: call.name.clone(),
                        detail,
                        preview,
                    })
                    .is_err()
                {
                    return None;
                }

                // File IO is blocking: keep it off the async runtime's worker threads.
                let tools = Arc::clone(&self.tools);
                let disabled = Arc::clone(&self.disabled_tools);
                let call_for_task = call.clone();
                let outcome = tokio::task::spawn_blocking(move || {
                    run_tool(&tools, &disabled, &call_for_task)
                })
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

/// Removes configuration-disabled tools from a schema list. Both sides are
/// normalized (trim + lowercase), so the helper is safe to call with any
/// user-supplied list, not just pre-parsed names.
fn filter_disabled_schemas(
    schemas: Vec<crate::agent::provider::Tool>,
    disabled: &[String],
) -> Vec<crate::agent::provider::Tool> {
    if disabled.is_empty() {
        return schemas;
    }
    schemas
        .into_iter()
        .filter(|tool| {
            let name = tool.function.name.to_ascii_lowercase();
            !disabled
                .iter()
                .any(|d| d.trim().to_ascii_lowercase() == name)
        })
        .collect()
}

/// Executes a tool call and returns `(payload for the model, human summary for the UI)`.
/// `disabled` carries the configuration-disabled tool names (lowercased): a
/// disabled tool is refused even though the schema list should already have
/// hidden it - the model can still hallucinate a name.
pub(crate) fn run_tool(
    tools: &ToolSet,
    disabled: &[String],
    call: &ToolCall,
) -> Result<(String, String), String> {
    let requested = call.name.trim().to_ascii_lowercase();
    if disabled.contains(&requested) {
        return Err(
            "{\"error\":\"tool disabled by configuration\",\"code\":\"tool_disabled\"}".to_string(),
        );
    }
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
                .files
                .list_files(parsed)
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
                .files
                .read_file(parsed)
                .map_err(|e| format!("read_file failed: {e}"))?;
            let suffix = if result.trimmed { " (truncated)" } else { "" };
            let summary = format!("{} lines{suffix}", result.lines);
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · {summary}", shorten(&path, 48))))
        }
        "search" => {
            let parsed: SearchArgs =
                serde_json::from_value(args).map_err(|e| format!("bad search arguments: {e}"))?;
            let query = parsed.query.clone();
            let result = tools
                .files
                .search(parsed)
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
                .files
                .write_file(parsed)
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
                .files
                .edit_file(parsed)
                .map_err(|e| format!("edit_file failed: {e}"))?;
            let summary = format!("{} replacements made", result.replacements_made);
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · {summary}", shorten(&path, 48))))
        }
        "read_document" => {
            let parsed: ReadDocumentArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad read_document arguments: {e}"))?;
            let path = parsed.path.clone();
            let result = tools
                .documents
                .read_document(parsed)
                .map_err(|e| format!("read_document failed: {e}"))?;
            let summary = format!(
                "{} · {} · {} {}{}",
                shorten(&path, 48),
                result.format,
                result.sections,
                if result.sections == 1 {
                    "section"
                } else {
                    "sections"
                },
                if result.truncated { " (truncated)" } else { "" }
            );
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, summary))
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
            let parsed: SshRunArgs =
                serde_json::from_value(args).map_err(|e| format!("bad ssh_run arguments: {e}"))?;
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
            Ok((
                payload,
                format!("{} · {}", shorten(&url, 40), summary.trim()),
            ))
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
        // Persistent memory. Every operation authorizes inside the engine via
        // MemoryPolicy; these arms only adapt JSON and keep payloads bounded.
        "memory_search" => {
            let parsed: MemorySearchArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad memory_search arguments: {e}"))?;
            let result = memory_error(tools.memory.as_ref(), |memory| memory.search(parsed))?;
            let summary = if result.results.is_empty() {
                "no matching memories".to_string()
            } else {
                format!("{} of {} memories", result.results.len(), result.total)
            };
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, summary))
        }
        "memory_save" => {
            let parsed: MemorySaveArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad memory_save arguments: {e}"))?;
            let content = parsed.content.clone();
            let result = memory_error(tools.memory.as_ref(), |memory| memory.save(parsed))?;
            let summary = if result.duplicate {
                "already stored".to_string()
            } else {
                format!("saved as {}", result.id)
            };
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · {}", shorten(&content, 40), summary)))
        }
        "memory_get" => {
            let parsed: MemoryGetArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad memory_get arguments: {e}"))?;
            let id = parsed.id.clone();
            let result = memory_error(tools.memory.as_ref(), |memory| memory.get(parsed))?;
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · retrieved", shorten(&id, 20))))
        }
        "memory_update" => {
            let parsed: MemoryUpdateArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad memory_update arguments: {e}"))?;
            let id = parsed.id.clone();
            let result = memory_error(tools.memory.as_ref(), |memory| memory.update(parsed))?;
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · updated", shorten(&id, 20))))
        }
        "memory_delete" => {
            let parsed: MemoryDeleteArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad memory_delete arguments: {e}"))?;
            let id = parsed.id.clone();
            let result = memory_error(tools.memory.as_ref(), |memory| memory.delete(parsed))?;
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, format!("{} · deleted", shorten(&id, 20))))
        }
        "memory_list" => {
            let parsed: MemoryListArgs = serde_json::from_value(args)
                .map_err(|e| format!("bad memory_list arguments: {e}"))?;
            let result = memory_error(tools.memory.as_ref(), |memory| memory.list(parsed))?;
            let summary = if result.results.is_empty() {
                "no memories stored".to_string()
            } else {
                format!("{} of {} memories", result.results.len(), result.total)
            };
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, summary))
        }
        // Self-improvement. Staging only: these arms never call
        // `ImproveEngine::apply`, which is reachable exclusively from the
        // `/improve` overlay's explicit approve keypress.
        "propose_self_edit" => {
            let parsed: crate::tools::improve_tools::ProposeSelfEditArgs =
                serde_json::from_value(args)
                    .map_err(|e| format!("bad propose_self_edit arguments: {e}"))?;
            let path = parsed.path.clone();
            let result = improve_error(
                tools.improve.as_ref(),
                tools.improve_roots.as_ref(),
                |engine, roots| crate::tools::improve_tools::propose(engine, parsed, roots),
            )?;
            let summary = format!(
                "{} · {} · +{} −{}",
                shorten(&result.id, 20),
                shorten(&path, 40),
                result.added,
                result.removed
            );
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, summary))
        }
        "list_self_edits" => {
            let result = improve_error(
                tools.improve.as_ref(),
                tools.improve_roots.as_ref(),
                |engine, _roots| crate::tools::improve_tools::list_pending(engine),
            )?;
            let summary = if result.proposals.is_empty() {
                "no pending proposals".to_string()
            } else {
                format!("{} pending", result.total)
            };
            let payload = serde_json::to_string(&result).unwrap_or_default();
            Ok((payload, summary))
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}

/// Runs a memory tool when the engine is attached; reports a structured refusal
/// when it is not. Tool failures are data for the model, never a crash.
fn memory_error<T, F>(memory: Option<&Arc<crate::memory::MemoryEngine>>, f: F) -> Result<T, String>
where
    F: FnOnce(
        &crate::tools::memory_tools::MemoryTools,
    ) -> Result<T, crate::memory::model::MemoryError>,
{
    let Some(engine) = memory else {
        return Err("memory is not available in this session".to_string());
    };
    let tools = crate::tools::memory_tools::MemoryTools::new(Arc::clone(engine));
    f(&tools).map_err(|e| {
        // Structured, machine-readable refusal: the model can adapt instead of
        // retrying blindly.
        format!(
            "{{\"error\":{},\"code\":\"{}\"}}",
            serde_json::to_string(&e.to_string()).unwrap_or_else(|_| "\"memory error\"".into()),
            e.code()
        )
    })
}

/// Runs a self-improvement tool when the engine is attached; reports a
/// structured refusal otherwise. A safety denial is data for the model, never a
/// crash, so the engine's own message is passed through verbatim.
fn improve_error<T, F>(
    engine: Option<&Arc<crate::improve::ImproveEngine>>,
    roots: Option<&crate::improve::Roots>,
    f: F,
) -> Result<T, String>
where
    F: FnOnce(
        &crate::improve::ImproveEngine,
        &crate::improve::Roots,
    ) -> Result<T, crate::improve::ImproveError>,
{
    let (Some(engine), Some(roots)) = (engine, roots) else {
        return Err("self-improvement is not available in this session".to_string());
    };
    f(engine, roots).map_err(|e| {
        format!(
            "{{\"error\":{},\"code\":\"{}\"}}",
            serde_json::to_string(&e.to_string()).unwrap_or_else(|_| "\"improve error\"".into()),
            match e {
                crate::improve::ImproveError::Policy(_) => "improve_policy",
                crate::improve::ImproveError::Invalid(_) => "improve_invalid",
                _ => "improve_error",
            }
        )
    })
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
        .or_else(|| {
            args.get("id")
                .and_then(|i| i.as_str())
                .map(|i| shorten(i, 20))
        })
        .unwrap_or_default()
}

/// Shared with the tool layer: shortens a string to `max` characters with an
/// ellipsis, used for one-line UI summaries and bounded payload previews.
pub(crate) fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

/// Caps for the live tool preview shown under a transcript tool row.
const PREVIEW_MAX_LINES: usize = 12;
const PREVIEW_MAX_CHARS: usize = 1_600;

/// Bounded preview of what a tool call is about to do, built from its
/// arguments at start time so the transcript shows the code being written or
/// edited while it happens: `write_file` shows the content, `edit_file` a
/// compact `-old`/`+new` diff, `run_command` the command line. Bounded so a
/// huge file write can never flood the event channel or the transcript.
fn preview_args(call: &ToolCall) -> Option<String> {
    let args = serde_json::from_str::<serde_json::Value>(&call.arguments).ok()?;
    match call.name.as_str() {
        "write_file" => {
            let content = args.get("content").and_then(|v| v.as_str())?;
            if content.trim().is_empty() {
                return None;
            }
            Some(bounded_preview(
                content,
                PREVIEW_MAX_LINES,
                PREVIEW_MAX_CHARS,
            ))
        }
        "edit_file" => {
            let old = args.get("old_string").and_then(|v| v.as_str())?;
            let new = args.get("new_string").and_then(|v| v.as_str())?;
            if old.trim().is_empty() && new.trim().is_empty() {
                return None;
            }
            let half_lines = PREVIEW_MAX_LINES / 2;
            let half_chars = PREVIEW_MAX_CHARS / 2;
            let removed = old
                .lines()
                .map(|l| format!("- {l}"))
                .collect::<Vec<_>>()
                .join("\n");
            let added = new
                .lines()
                .map(|l| format!("+ {l}"))
                .collect::<Vec<_>>()
                .join("\n");
            Some(format!(
                "{}\n{}",
                bounded_preview(&removed, half_lines, half_chars),
                bounded_preview(&added, half_lines, half_chars),
            ))
        }
        "run_command" => {
            let cmd = args.get("command").and_then(|v| v.as_str())?;
            if cmd.trim().is_empty() {
                return None;
            }
            Some(shorten(cmd, 120))
        }
        // read_document previews from arguments alone (no IO here - the loop
        // runs previews on the async thread): the target format comes from the
        // extension, the section titles only exist after extraction.
        "read_document" => {
            let path = args.get("path").and_then(|v| v.as_str())?;
            if path.trim().is_empty() {
                return None;
            }
            let format = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .and_then(crate::extract::Format::from_extension)
                .map(|f| f.as_str())
                .unwrap_or("document");
            Some(shorten(&format!("[{format}] {path}"), 120))
        }
        "propose_self_edit" => {
            // The rationale is the "why" a reviewer wants to see first; the
            // preview is built from arguments alone, like every other preview.
            let rationale = args.get("rationale").and_then(|v| v.as_str())?;
            if rationale.trim().is_empty() {
                return None;
            }
            Some(bounded_preview(
                rationale,
                PREVIEW_MAX_LINES,
                PREVIEW_MAX_CHARS,
            ))
        }
        _ => None,
    }
}

/// Trim a block to at most `max_lines` lines and `max_chars` characters,
/// appending a `… +N more lines` marker when anything was cut.
fn bounded_preview(text: &str, max_lines: usize, max_chars: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    let mut emitted = 0usize;
    for line in lines.iter().take(max_lines) {
        if out.len() >= max_chars {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        let room = max_chars - out.len();
        if line.len() + 1 > room {
            out.push_str(&shorten(line, room.saturating_sub(1)));
        } else {
            out.push_str(line);
        }
        emitted += 1;
    }
    let hidden = lines.len().saturating_sub(emitted);
    if hidden > 0 {
        out.push_str(&format!("\n… +{hidden} more lines"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::{BoxStream, StreamEvent, Tool};
    use crate::memory::model::{MemoryCategory, NewMemory, SearchQuery};
    use crate::memory::policy::MemoryPolicy;
    use crate::memory::MemoryEngine;
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// Provider that records the messages it is handed, so tests can assert on
    /// what the runtime actually sent.
    struct RecordingProvider {
        seen: Mutex<Vec<Vec<ChatMessage>>>,
    }

    impl RecordingProvider {
        fn new() -> Self {
            Self {
                seen: Mutex::new(Vec::new()),
            }
        }

        fn last_system(&self) -> String {
            self.seen
                .lock()
                .unwrap()
                .last()
                .and_then(|messages| messages.first())
                .map(|m| m.content.clone())
                .unwrap_or_default()
        }
    }

    #[async_trait]
    impl AgentProvider for RecordingProvider {
        async fn stream_chat(
            &self,
            messages: Vec<ChatMessage>,
            _tools: Option<Vec<Tool>>,
        ) -> Result<BoxStream, DynError> {
            self.seen.lock().unwrap().push(messages);
            let stream = futures_util::stream::iter(vec![
                Ok(StreamEvent::Delta("done".into())),
                Ok(StreamEvent::Done),
            ]);
            Ok(Box::pin(stream))
        }
    }

    fn memory_engine() -> Arc<MemoryEngine> {
        let policy = MemoryPolicy {
            mirror_markdown: false,
            ..MemoryPolicy::default()
        };
        Arc::new(MemoryEngine::open_in_memory(policy).expect("engine"))
    }

    fn runtime_with(
        provider: Arc<dyn AgentProvider>,
        memory: Option<Arc<MemoryEngine>>,
    ) -> AgentRuntime {
        let runtime = AgentRuntime::new(
            provider,
            std::env::temp_dir().join("nanomite-runtime-test-workspace"),
            64 * 1024,
            None,
        );
        match memory {
            Some(engine) => runtime.with_memory(engine),
            None => runtime,
        }
    }

    async fn run_turn(runtime: &AgentRuntime) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        runtime
            .run(
                "what storage does this project use".into(),
                &[],
                Cancel::new(),
                tx,
            )
            .await;
        while rx.try_recv().is_ok() {}
    }

    #[tokio::test]
    async fn retrieved_memory_is_injected_into_the_system_message() {
        let memory = memory_engine();
        memory
            .save(&NewMemory::new(
                MemoryCategory::Decision,
                "nanomite stores memory in rusqlite",
            ))
            .unwrap();

        let provider = Arc::new(RecordingProvider::new());
        let runtime = runtime_with(
            Arc::clone(&provider) as Arc<dyn AgentProvider>,
            Some(Arc::clone(&memory)),
        );
        run_turn(&runtime).await;
        let system = provider.last_system();
        assert!(
            system.contains(crate::memory::CONTEXT_OPEN),
            "memory block must be framed as data:\n{system}"
        );
        assert!(
            system.contains("rusqlite"),
            "retrieved fact must be present:\n{system}"
        );
    }

    #[tokio::test]
    async fn turns_without_memory_send_a_plain_system_prompt() {
        let provider = Arc::new(RecordingProvider::new());
        let runtime = runtime_with(Arc::clone(&provider) as Arc<dyn AgentProvider>, None);
        run_turn(&runtime).await;
        assert!(!provider.last_system().contains(crate::memory::CONTEXT_OPEN));
    }

    #[tokio::test]
    async fn finished_turns_feed_the_capture_policy() {
        let memory = memory_engine();
        let runtime = runtime_with(
            Arc::new(RecordingProvider::new()),
            Some(Arc::clone(&memory)),
        );
        run_turn(&runtime).await;
        // The capture policy runs detached; poll briefly for the checkpoint.
        for _ in 0..50 {
            if memory
                .search(&SearchQuery::text("checkpoint"), false)
                .map(|(list, _)| list.total)
                .unwrap_or(0)
                > 0
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let (list, _) = memory
            .search(&SearchQuery::text("checkpoint"), false)
            .unwrap();
        assert!(list.total >= 1, "a session checkpoint must be captured");
    }

    #[test]
    fn memory_tools_dispatch_through_the_tool_layer() {
        let memory = memory_engine();
        let runtime = runtime_with(
            Arc::new(RecordingProvider::new()),
            Some(Arc::clone(&memory)),
        );
        let tools = runtime.tools.clone();

        let (payload, _) = run_tool(
            &tools,
            &[],
            &ToolCall {
                id: "t1".into(),
                name: "memory_save".into(),
                arguments: serde_json::json!({
                    "category": "core",
                    "content": "the release gate is cargo test"
                })
                .to_string(),
                index: 0,
            },
        )
        .expect("memory_save via run_tool");
        assert!(payload.contains("\"id\""));

        let (payload, _) = run_tool(
            &tools,
            &[],
            &ToolCall {
                id: "t2".into(),
                name: "memory_search".into(),
                arguments: serde_json::json!({ "query": "release gate" }).to_string(),
                index: 0,
            },
        )
        .expect("memory_search via run_tool");
        assert!(payload.contains("cargo test"), "{payload}");

        // Deletion without confirmation is refused; the refusal reaches the model
        // as a structured error string on the tool-result path.
        let refusal = run_tool(
            &tools,
            &[],
            &ToolCall {
                id: "t3".into(),
                name: "memory_delete".into(),
                arguments: serde_json::json!({ "id": "mem_x", "confirm": false }).to_string(),
                index: 0,
            },
        )
        .expect_err("the refusal reaches the model as data");
        assert!(refusal.starts_with("{\"error\""), "{refusal}");
    }

    #[test]
    fn memory_tools_are_absent_without_the_engine() {
        let runtime = runtime_with(Arc::new(RecordingProvider::new()), None);
        let outcome = run_tool(
            &runtime.tools,
            &[],
            &ToolCall {
                id: "t1".into(),
                name: "memory_list".into(),
                arguments: "{}".into(),
                index: 0,
            },
        );
        assert!(outcome.is_err(), "no engine means a clean refusal");
    }

    #[test]
    fn preview_args_shows_written_code() {
        let call = ToolCall {
            id: "t1".into(),
            name: "write_file".into(),
            arguments: r#"{"path":"c.py","content":"def add(a, b):\n    return a + b"}"#
                .to_string(),
            index: 0,
        };
        let p = preview_args(&call).expect("write_file gets a preview");
        assert!(p.contains("def add(a, b):"), "{p}");
        assert!(p.contains("return a + b"), "{p}");
    }

    #[test]
    fn preview_args_edit_file_is_a_mini_diff() {
        let call = ToolCall {
            id: "t2".into(),
            name: "edit_file".into(),
            arguments: r#"{"path":"c.py","old_string":"return a - b","new_string":"return a + b"}"#
                .to_string(),
            index: 0,
        };
        let p = preview_args(&call).expect("edit_file gets a preview");
        assert!(p.contains("- return a - b"), "{p}");
        assert!(p.contains("+ return a + b"), "{p}");
    }

    #[test]
    fn preview_args_run_command_shows_the_command() {
        let call = ToolCall {
            id: "t3".into(),
            name: "run_command".into(),
            arguments: r#"{"command":"python3 calculator.py"}"#.to_string(),
            index: 0,
        };
        assert_eq!(
            preview_args(&call).as_deref(),
            Some("python3 calculator.py")
        );
    }

    #[test]
    fn preview_args_other_tools_and_empty_args_get_none() {
        let none = ToolCall {
            id: "t4".into(),
            name: "list_files".into(),
            arguments: "{}".into(),
            index: 0,
        };
        assert!(preview_args(&none).is_none());

        let empty = ToolCall {
            id: "t5".into(),
            name: "write_file".into(),
            arguments: r#"{"path":"c.py","content":""}"#.to_string(),
            index: 0,
        };
        assert!(preview_args(&empty).is_none());

        let junk = ToolCall {
            id: "t6".into(),
            name: "write_file".into(),
            arguments: "not json".into(),
            index: 0,
        };
        assert!(preview_args(&junk).is_none());
    }

    #[test]
    fn bounded_preview_caps_lines_and_marks_the_rest() {
        let text = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let p = bounded_preview(&text, 5, 1_600);
        assert_eq!(p.lines().count(), 6, "5 kept + marker: {p}");
        assert!(p.contains("… +35 more lines"), "{p}");
    }

    #[test]
    fn bounded_preview_caps_chars() {
        let text = "x".repeat(10_000);
        let p = bounded_preview(&text, 100, 200);
        assert!(p.len() <= 210, "char cap leaked: {}", p.len());
    }

    #[test]
    fn preview_args_read_document_shows_format_and_path() {
        let call = ToolCall {
            id: "t7".into(),
            name: "read_document".into(),
            arguments: r#"{"path":"report.pdf"}"#.to_string(),
            index: 0,
        };
        let p = preview_args(&call).expect("read_document gets a preview");
        assert!(p.contains("pdf"), "{p}");
        assert!(p.contains("report.pdf"), "{p}");
        assert!(p.chars().count() <= 160, "preview must stay bounded: {p}");

        let none = ToolCall {
            id: "t8".into(),
            name: "read_document".into(),
            arguments: "{}".into(),
            index: 0,
        };
        assert!(preview_args(&none).is_none());
    }

    #[test]
    fn filter_disabled_schemas_removes_only_disabled_names() {
        let schemas = crate::agent::openai::tool_schemas();
        let disabled = vec!["read_file".to_string(), "WEB_FETCH".to_string()];
        let kept = filter_disabled_schemas(schemas, &disabled);
        let names: Vec<&str> = kept.iter().map(|t| t.function.name.as_str()).collect();
        assert!(!names.contains(&"read_file"));
        assert!(!names.contains(&"web_fetch"));
        assert!(names.contains(&"list_files"));

        // Empty disabled list is the identity.
        let schemas = crate::agent::openai::tool_schemas();
        assert_eq!(filter_disabled_schemas(schemas, &[]).len(), 10);
    }

    #[test]
    fn disabled_tools_are_hidden_from_schemas_and_refused_at_dispatch() {
        let provider: Arc<dyn AgentProvider> = Arc::new(RecordingProvider::new());
        let runtime = runtime_with(provider, Some(memory_engine()))
            .with_disabled_tools(vec!["read_file".into(), "memory_save".into()]);
        let names: Vec<String> = runtime
            .tools_schema
            .iter()
            .map(|t| t.function.name.clone())
            .collect();
        assert!(!names.iter().any(|n| n == "read_file"), "{names:?}");
        assert!(
            !names.iter().any(|n| n == "memory_save"),
            "memory schemas must be filtered too: {names:?}"
        );
        assert!(names.iter().any(|n| n == "list_files"));
        assert!(names.iter().any(|n| n == "memory_search"));
        assert!(names.iter().any(|n| n == "read_document"));

        // Defense in depth: a hallucinated call to a disabled tool is refused
        // with a structured error payload, never executed.
        let tools = ToolSet::new(
            std::env::temp_dir().join("nanomite-runtime-test-workspace"),
            64 * 1024,
        );
        let call = ToolCall {
            id: "t9".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"Cargo.toml"}"#.to_string(),
            index: 0,
        };
        let err = run_tool(&tools, &runtime.disabled_tools, &call).unwrap_err();
        assert!(err.contains("tool disabled by configuration"), "{err}");

        // A tool that is merely unknown is still reported as unknown, not
        // conflated with the disabled refusal.
        let unknown = ToolCall {
            id: "t10".into(),
            name: "no_such_tool".into(),
            arguments: "{}".into(),
            index: 0,
        };
        let err = run_tool(&tools, &runtime.disabled_tools, &unknown).unwrap_err();
        assert!(err.contains("unknown tool"), "{err}");
    }

    #[tokio::test]
    async fn read_document_dispatch_returns_payload_and_summary() {
        let ws = std::env::temp_dir().join(format!("nanomite-runtime-doc-{}", std::process::id()));
        std::fs::create_dir_all(&ws).expect("temp workspace");
        std::fs::write(ws.join("doc.md"), "# Title\n\nBody text.\n").expect("fixture");

        let provider: Arc<dyn AgentProvider> = Arc::new(RecordingProvider::new());
        let runtime = runtime_with(provider, None);
        // Point the runtime's tool set at the fixture workspace.
        let runtime = AgentRuntime {
            tools: Arc::new(ToolSet::new(ws.clone(), 64 * 1024)),
            ..runtime
        };

        let call = ToolCall {
            id: "t11".into(),
            name: "read_document".into(),
            arguments: r#"{"path":"doc.md"}"#.to_string(),
            index: 0,
        };
        let disabled: Vec<String> = Vec::new();
        let (payload, summary) = run_tool(&runtime.tools, &disabled, &call).expect("extract");
        assert!(payload.contains("\"format\":\"markdown\""), "{payload}");
        assert!(payload.contains("Body text."), "{payload}");
        assert!(summary.contains("markdown"), "{summary}");
        assert!(
            summary.contains("sections") || summary.contains("section"),
            "{summary}"
        );

        let _ = std::fs::remove_dir_all(&ws);
    }

    /// Self-improvement: two schemas, no apply, and the truth in the prompt.
    fn improve_runtime(dir: &std::path::Path) -> AgentRuntime {
        let roots = crate::improve::Roots {
            config_path: dir.join("config.toml"),
            prompt_path: dir.join("prompt.md"),
            repo_root: dir.to_path_buf(),
        };
        let engine = Arc::new(crate::improve::ImproveEngine::new(dir.join(".improve")));
        runtime_with(Arc::new(RecordingProvider::new()), None).with_improve(engine, roots)
    }

    #[tokio::test]
    async fn the_custom_prompt_file_reaches_the_system_message() {
        let provider = Arc::new(RecordingProvider::new());
        let runtime = runtime_with(Arc::clone(&provider) as Arc<dyn AgentProvider>, None)
            .with_prompt_file("  Always answer in metric units.  ".to_string());
        run_turn(&runtime).await;
        let system = provider.last_system();
        assert!(
            system.contains("Always answer in metric units."),
            "prompt file content missing:\n{system}"
        );
        // It is appended after the base prompt, never replacing it.
        assert!(system.contains("You are nanomite"), "{system}");
    }

    #[tokio::test]
    async fn an_empty_prompt_file_changes_nothing() {
        let provider = Arc::new(RecordingProvider::new());
        let base = runtime_with(Arc::clone(&provider) as Arc<dyn AgentProvider>, None);
        run_turn(&base).await;
        let plain = provider.last_system();
        let provider = Arc::new(RecordingProvider::new());
        let runtime = runtime_with(Arc::clone(&provider) as Arc<dyn AgentProvider>, None)
            .with_prompt_file("   \n  ".to_string());
        run_turn(&runtime).await;
        assert_eq!(provider.last_system(), plain);
    }

    #[test]
    fn improve_attaches_exactly_two_schemas_and_no_apply() {
        let dir = std::env::temp_dir().join(format!("nanomite-improve-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let runtime = improve_runtime(&dir);
        let names: Vec<&str> = runtime
            .tools_schema
            .iter()
            .map(|t| t.function.name.as_str())
            .collect();
        assert!(names.contains(&"propose_self_edit"), "{names:?}");
        assert!(names.contains(&"list_self_edits"), "{names:?}");
        // Approval is a human action: there is deliberately no apply tool.
        assert!(
            !names.iter().any(|n| n.contains("apply")),
            "no agent-callable apply may exist: {names:?}"
        );
        let system = runtime.system_prompt.clone();
        assert!(
            system.contains("staged for user review"),
            "the prompt must say proposals need the user: {system}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn self_edit_tools_are_refused_without_the_engine() {
        let runtime = runtime_with(Arc::new(RecordingProvider::new()), None);
        let tools = runtime.tools.clone();
        let err = run_tool(
            &tools,
            &[],
            &ToolCall {
                id: "t1".into(),
                name: "list_self_edits".into(),
                arguments: "{}".to_string(),
                index: 0,
            },
        )
        .expect_err("no engine, no self-edit tools");
        assert!(err.contains("not available"), "{err}");
    }

    #[test]
    fn propose_dispatches_through_the_tool_layer_and_never_applies() {
        let dir =
            std::env::temp_dir().join(format!("nanomite-improve-disp-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).expect("temp dir");
        std::fs::write(dir.join("src/lib.rs"), "pub fn a() {}\n").expect("fixture");
        let runtime = improve_runtime(&dir);
        let tools = runtime.tools.clone();

        let (payload, summary) = run_tool(
            &tools,
            &[],
            &ToolCall {
                id: "t1".into(),
                name: "propose_self_edit".into(),
                arguments: r#"{"target":"source","path":"src/lib.rs","new_content":"pub fn b() {}\n","rationale":"rename"}"#.to_string(),
                index: 0,
            },
        )
        .expect("propose via run_tool");
        assert!(payload.contains("\"status\":\"pending\""), "{payload}");
        assert!(summary.contains("+1"), "{summary}");

        // Staging left the file exactly as it was.
        assert_eq!(
            std::fs::read_to_string(dir.join("src/lib.rs")).unwrap(),
            "pub fn a() {}\n"
        );

        let (payload, summary) = run_tool(
            &tools,
            &[],
            &ToolCall {
                id: "t2".into(),
                name: "list_self_edits".into(),
                arguments: "{}".to_string(),
                index: 0,
            },
        )
        .expect("list via run_tool");
        assert!(payload.contains("\"total\":1"), "{payload}");
        assert!(payload.contains("rename"), "{payload}");
        assert_eq!(summary, "1 pending");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_args_shows_the_proposal_rationale() {
        let preview = preview_args(&ToolCall {
            id: "t1".into(),
            name: "propose_self_edit".into(),
            arguments: r#"{"target":"source","path":"src/lib.rs","new_content":"x","rationale":"speed up the parser"}"#.to_string(),
            index: 0,
        });
        assert_eq!(preview.as_deref(), Some("speed up the parser"));
        // No rationale means no preview rather than an empty row.
        assert_eq!(
            preview_args(&ToolCall {
                id: "t2".into(),
                name: "propose_self_edit".into(),
                arguments: r#"{"target":"source","rationale":"  "}"#.to_string(),
                index: 0,
            }),
            None
        );
    }
}
