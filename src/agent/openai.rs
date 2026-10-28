use super::provider::{
    AgentProvider, BoxStream, ChatMessage, DynError, Role, StreamEvent, Tool, ToolCall, Usage,
};
use crate::config::Config;
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

/// Client for any OpenAI-compatible `/chat/completions` endpoint with SSE streaming.
pub struct OpenAiProvider {
    client: reqwest::Client,
    endpoint: String,
    base_url: String,
    api_key: Option<String>,
    model: String,
    reasoning: Option<String>,
}

impl OpenAiProvider {
    pub fn new(cfg: &Config) -> Result<Self, DynError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .pool_idle_timeout(Duration::from_secs(90))
            .user_agent(concat!("nanomite/", env!("CARGO_PKG_VERSION")))
            .build()?;

        let reasoning = cfg
            .supports_reasoning_effort()
            .then(|| cfg.reasoning.as_str().to_string());
        Ok(Self {
            client,
            endpoint: cfg.endpoint(),
            base_url: cfg.base_url.clone(),
            api_key: cfg.api_key.clone(),
            model: cfg.model.clone(),
            reasoning,
        })
    }

    /// OpenAI wire format for a chat message. Tool calls must be nested under a
    /// `function` object with `id` + `type`, and tool results must carry
    /// `tool_call_id` - serde's derived shape is flat and providers reject it.
    fn message_to_wire(m: &ChatMessage) -> Value {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
            Role::System => "system",
            Role::Unknown => "user",
        };
        let mut obj = serde_json::Map::new();
        obj.insert("role".into(), json!(role));
        match m.role {
            Role::Tool => {
                obj.insert("content".into(), json!(m.content));
                if let Some(id) = &m.tool_call_id {
                    obj.insert("tool_call_id".into(), json!(id));
                }
            }
            _ => {
                if let Some(calls) = &m.tool_calls {
                    obj.insert(
                        "content".into(),
                        if m.content.is_empty() {
                            Value::Null
                        } else {
                            json!(m.content)
                        },
                    );
                    let arr: Vec<Value> = calls
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            let id = if c.id.is_empty() {
                                format!("call_{i}")
                            } else {
                                c.id.clone()
                            };
                            json!({
                                "id": id,
                                "type": "function",
                                "function": { "name": c.name, "arguments": c.arguments }
                            })
                        })
                        .collect();
                    obj.insert("tool_calls".into(), Value::Array(arr));
                } else {
                    obj.insert("content".into(), json!(m.content));
                }
            }
        }
        Value::Object(obj)
    }

    fn build_body(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        stream: bool,
    ) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": messages.iter().map(Self::message_to_wire).collect::<Vec<_>>(),
            "stream": stream,
        });
        if stream {
            body["stream_options"] = json!({ "include_usage": true });
        }
        if let Some(effort) = &self.reasoning {
            body["reasoning_effort"] = json!(effort);
        }
        if let Some(tools) = tools.filter(|t| !t.is_empty()) {
            body["tools"] = serde_json::to_value(tools).unwrap_or(Value::Null);
        }
        body
    }
}

#[async_trait]
impl AgentProvider for OpenAiProvider {
    async fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Result<BoxStream, DynError> {
        let mut req = self
            .client
            .post(&self.endpoint)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&self.build_body(messages, tools, true));
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req.send().await.map_err(|e| {
            DynError::from(format!(
                "request to {} failed: {e} (is the base URL correct and reachable?)",
                self.endpoint
            ))
        })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(describe_http_error(status.as_u16(), &body).into());
        }

        let sse = resp.bytes_stream().map(|res| res.map(|b| b.to_vec()));
        Ok(Box::pin(decode_sse(sse)))
    }

    /// `GET {base}/models`; understands both the OpenAI shape and Ollama's native one.
    async fn list_models(&self) -> Result<Vec<String>, DynError> {
        let url = format!("{}/models", self.base_url);
        let mut req = self.client.get(&url).header("accept", "application/json");
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("could not reach {url}: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(describe_http_error(status.as_u16(), &body).into());
        }

        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("malformed model list: {e}"))?;
        let mut models: Vec<String> = value
            .get("data")
            .or_else(|| value.get("models"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        item.get("id")
                            .or_else(|| item.get("name"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default();

        models.sort();
        models.dedup();
        Ok(models)
    }
}

/// Incremental SSE decoder: text deltas, tool-call fragments and usage.
fn decode_sse<S>(
    byte_stream: S,
) -> impl futures_util::Stream<Item = Result<StreamEvent, DynError>> + Send
where
    S: futures_util::Stream<Item = Result<Vec<u8>, reqwest::Error>> + Send + 'static,
{
    let state = SseState {
        stream: Box::pin(byte_stream),
        buffer: Vec::with_capacity(8 * 1024),
    };
    futures_util::stream::unfold(state, |mut st| async move {
        loop {
            if st.buffer.len() > 8 * 1024 * 1024 {
                st.buffer.clear();
            }
            if let Some(end) = find_event_boundary(&st.buffer) {
                let block = st.take(end);
                if let Some(event) = parse_event(&block) {
                    return Some((event, st));
                }
                continue;
            }
            match st.stream.next().await {
                Some(Ok(bytes)) => st.buffer.extend_from_slice(&bytes),
                Some(Err(e)) => {
                    let err: DynError = format!("stream interrupted: {e}").into();
                    return Some((Err(err), st));
                }
                None => {
                    let block = std::mem::take(&mut st.buffer);
                    if block.is_empty() {
                        return None;
                    }
                    let block = String::from_utf8_lossy(&block).into_owned();
                    return parse_event(&block).map(|event| (event, st));
                }
            }
        }
    })
}

struct SseState<S> {
    stream: std::pin::Pin<Box<S>>,
    buffer: Vec<u8>,
}

impl<S> SseState<S> {
    /// Removes the first `block_len` bytes. The boundary already includes the
    /// blank separator line, so nothing further is consumed.
    fn take(&mut self, block_len: usize) -> String {
        let block: Vec<u8> = self.buffer.drain(..block_len).collect();
        String::from_utf8_lossy(&block).into_owned()
    }
}

/// Returns the byte offset where the first complete event block ends (including its blank line).
fn find_event_boundary(buf: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < buf.len() {
        let rel = buf[i..].iter().position(|&b| b == b'\n')?;
        let end = i + rel + 1;
        if buf[i..end].iter().all(|b| b.is_ascii_whitespace()) {
            return Some(end);
        }
        i = end;
    }
    None
}

fn parse_event(block: &str) -> Option<Result<StreamEvent, DynError>> {
    let mut data = String::new();
    for line in block.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(payload) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(payload.trim_start());
        }
    }
    if data.is_empty() {
        return None;
    }
    if data.trim() == "[DONE]" {
        return Some(Ok(StreamEvent::Done));
    }

    let value: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => return Some(Err(format!("malformed stream chunk: {e}").into())),
    };

    if let Some(err) = value.get("error") {
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("stream error");
        return Some(Err(message.to_string().into()));
    }

    if let Some(usage) = value.get("usage").filter(|u| !u.is_null()) {
        if let Ok(usage) = serde_json::from_value::<UsageWire>(usage.clone()) {
            return Some(Ok(StreamEvent::Usage(usage.into())));
        }
    }

    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())?;
    let delta = choice.get("delta").or_else(|| choice.get("message"));

    if let Some(calls) = delta
        .and_then(|d| d.get("tool_calls"))
        .and_then(Value::as_array)
    {
        let mut out = None;
        for call in calls {
            let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let id = call.get("id").and_then(Value::as_str).unwrap_or_default();
            let func = call.get("function");
            let name = func
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = func
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            out = Some(StreamEvent::ToolCall(ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: args.to_string(),
                index,
            }));
        }
        return out.map(Ok);
    }

    // The answer text first; reasoning models (nemotron, deepseek-r1, …)
    // stream their thoughts in a separate field, which surfaces as its own
    // event so the wait stays visible in the transcript.
    if let Some(s) = delta
        .and_then(|d| d.get("content"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        return Some(Ok(StreamEvent::Delta(s.to_string())));
    }
    delta
        .and_then(|d| d.get("reasoning_content"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| Ok(StreamEvent::ReasoningDelta(s.to_string())))
}

#[derive(Deserialize)]
struct UsageWire {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

impl From<UsageWire> for Usage {
    fn from(w: UsageWire) -> Self {
        let total = if w.total_tokens == 0 {
            w.prompt_tokens + w.completion_tokens
        } else {
            w.total_tokens
        };
        Usage {
            prompt_tokens: w.prompt_tokens,
            completion_tokens: w.completion_tokens,
            total_tokens: total,
        }
    }
}

fn describe_http_error(status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(240).collect::<String>());

    match status {
        401 | 403 => format!("{status} unauthorized - check NANOMITE_API_KEY"),
        404 => format!("{status} not found - check NANOMITE_BASE_URL and NANOMITE_MODEL"),
        429 => format!("{status} rate limited - slow down or switch model"),
        400..=499 => format!("{status} {detail}"),
        _ => format!("{status} {detail}"),
    }
}

/// Tool catalogue exposed to the model.
pub fn tool_schemas() -> Vec<Tool> {
    vec![
        Tool::new(
            "list_files",
            "List files and directories at a path inside the workspace.",
            json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path (defaults to the workspace root)"
                    }
                },
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "read_file",
            "Read a UTF-8 text file from the workspace.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative file path" },
                    "limit": { "type": "integer", "description": "Max bytes to read" },
                    "offset": { "type": "integer", "description": "Byte offset to start from" }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "search",
            "Search workspace text files for a case-insensitive substring and return matching lines with paths and line numbers.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Text to search for (case-insensitive)" },
                    "path": { "type": "string", "description": "Workspace-relative directory to search (defaults to the workspace root)" }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "write_file",
            "Write content to a file inside the workspace.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative file path" },
                    "content": { "type": "string", "description": "Content to write to the file" },
                    "append": { "type": "boolean", "description": "Whether to append to the file (default: false)" }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "edit_file",
            "Edit a file by replacing occurrences of a string with another string.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative file path" },
                    "old_string": { "type": "string", "description": "String to replace" },
                    "new_string": { "type": "string", "description": "String to replace with" },
                    "replace_all": { "type": "boolean", "description": "Whether to replace all occurrences (default: false)" }
                },
                "required": ["path", "old_string", "new_string"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "read_document",
            "Extract text from a document in the workspace (pdf, docx, xlsx, odt, ods, rtf, csv, plus text/markdown/json/yaml/xml/html). Returns the detected format, section count and bounded text.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative document path" }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "run_command",
            "Run a shell command inside the workspace (sh -c). Use for builds, git, package managers, process inspection and any Linux tooling.",
            json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command line, e.g. cargo test --quiet" },
                    "cwd": { "type": "string", "description": "Workspace-relative working directory (defaults to the workspace root)" },
                    "timeout_ms": { "type": "integer", "description": "Hard cap in milliseconds (max 120000, default 60000)" },
                    "sudo_password": { "type": "string", "description": "Optional sudo password for commands requiring elevation. If provided, command runs via sudo -S with password piped to stdin. Never logged or stored." }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "ssh_run",
            "Run a command on a remote host over SSH (key-based auth only; BatchMode).",
            json!({
                "type": "object",
                "properties": {
                    "host": { "type": "string", "description": "user@host or an ssh-config alias" },
                    "command": { "type": "string", "description": "Command line to run on the remote host" },
                    "timeout_ms": { "type": "integer", "description": "Hard cap in milliseconds (max 120000, default 30000)" }
                },
                "required": ["host", "command"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "web_fetch",
            "Fetch an http(s) URL and return its readable text content (HTML stripped).",
            json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Absolute http(s) URL" }
                },
                "required": ["url"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "web_search",
            "Search the web (DuckDuckGo) and return titles, URLs and snippets.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "browse",
            "Open a URL in a headless browser (JavaScript rendered) and return the page's readable text.",
            json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Absolute http(s) URL" },
                    "wait_ms": { "type": "integer", "description": "Optional virtual-time budget in ms for JavaScript rendering (200-15000, default 3000)" }
                },
                "required": ["url"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "browse_screenshot",
            "Open a URL in a headless browser and save a full-page screenshot PNG in the workspace.",
            json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Absolute http(s) URL" },
                    "output": { "type": "string", "description": "Workspace-relative PNG path (defaults to screenshot-<unix time>.png)" }
                },
                "required": ["url"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "git_status",
            "Show git status: current branch, ahead/behind counts, staged, unstaged and untracked files.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative path to a file or directory (defaults to workspace root)" }
                },
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "git_diff",
            "Show git diff of working tree or staged changes, optionally for a specific path.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative file or directory to diff (defaults to all changes)" },
                    "staged": { "type": "boolean", "description": "Show staged (index) changes instead of working tree (default: false)" },
                    "include_untracked": { "type": "boolean", "description": "Include untracked files as new content (default: false)" }
                },
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "git_log",
            "Show recent git commit history with optional diffs.",
            json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Number of commits to show (default 20, max 200)" },
                    "path": { "type": "string", "description": "Optional path to limit history to" },
                    "show_diff": { "type": "boolean", "description": "Include full diff per commit (default: false)" }
                },
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "git_commit",
            "Create a git commit with the given message, optionally staging specific files first.",
            json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Commit message" },
                    "files": { "type": "array", "items": { "type": "string" }, "description": "Files to stage before commit (defaults to all tracked changes)" },
                    "allow_empty": { "type": "boolean", "description": "Allow empty commit (default: false)" }
                },
                "required": ["message"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "git_branch",
            "List, create, or delete git branches.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the new branch (omit to list all branches)" },
                    "start_point": { "type": "string", "description": "Starting commit-ish (defaults to HEAD)" },
                    "delete": { "type": "boolean", "description": "Delete the branch instead of creating (default: false)" },
                    "force": { "type": "boolean", "description": "Force delete (required if branch has unmerged changes, default: false)" }
                },
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "run_tests",
            "Run project tests using cargo, npm, pytest, jest, go test, or a custom command. Returns structured pass/fail results with per-test details.",
            json!({
                "type": "object",
                "properties": {
                    "runner": { "type": "string", "description": "Test runner: cargo, npm, pytest, jest, go, or custom" },
                    "args": { "type": "array", "items": { "type": "string" }, "description": "Additional arguments for the runner" },
                    "cwd": { "type": "string", "description": "Workspace-relative working directory (defaults to workspace root)" },
                    "timeout_ms": { "type": "integer", "description": "Hard cap in milliseconds (max 300000, default 120000)" },
                    "command": { "type": "string", "description": "For custom runner: full command line to execute" }
                },
                "required": ["runner"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "index_symbols",
            "Build or rebuild the project symbol index for fast code navigation. Uses ctags when available, falls back to regex-based indexing.",
            json!({
                "type": "object",
                "properties": {
                    "force_rebuild": { "type": "boolean", "description": "Force rebuild the index (default: false, uses cached index)" },
                    "languages": { "type": "array", "items": { "type": "string" }, "description": "Specific languages to index (default: all detected)" }
                },
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "find_symbol",
            "Search the symbol index for definitions matching a pattern. Supports wildcards (*) and filters by kind/language.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Symbol name or pattern to search for (supports * wildcard)" },
                    "kind": { "type": "string", "description": "Filter by symbol kind: function, struct, enum, trait, const, mod, etc." },
                    "language": { "type": "string", "description": "Filter by language" },
                    "limit": { "type": "integer", "description": "Max results (default 50, max 200)", "default": 50 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "find_references",
            "Find all references to a symbol across the project.",
            json!({
                "type": "object",
                "properties": {
                    "symbol": { "type": "string", "description": "Symbol name to find references for" },
                    "from_file": { "type": "string", "description": "Optional file to start search from (for context)" },
                    "limit": { "type": "integer", "description": "Max results (default 50, max 200)", "default": 50 }
                },
                "required": ["symbol"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "create_plan",
            "Create a new task plan with structured tasks, dependencies, and tags. Use this to break down complex work into trackable steps.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Unique plan name" },
                    "description": { "type": "string", "description": "Plan description" },
                    "tasks": { "type": "array", "items": { "type": "object", "properties": { "description": { "type": "string" }, "depends_on": { "type": "array", "items": { "type": "integer" } }, "tags": { "type": "array", "items": { "type": "string" } } }, "required": ["description"] } }
                },
                "required": ["name", "description", "tasks"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "get_plan",
            "Retrieve a plan by name, or the most recently updated plan if name is omitted.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Plan name (optional; defaults to most recent)" }
                },
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "update_task",
            "Update a task's status, description, dependencies, or tags.",
            json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "integer", "description": "Task ID to update" },
                    "status": { "type": "string", "description": "New status: pending, in_progress, done, blocked, cancelled" },
                    "description": { "type": "string", "description": "New description" },
                    "depends_on": { "type": "array", "items": { "type": "integer" }, "description": "New dependency task IDs" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "New tags" }
                },
                "required": ["task_id"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "list_plans",
            "List all saved plans with completion summaries.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "delete_plan",
            "Delete a plan by name (requires confirmation).",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Plan name to delete" },
                    "confirm": { "type": "boolean", "description": "Must be true to confirm deletion" }
                },
                "required": ["name", "confirm"],
                "additionalProperties": false
            }),
        ),
    ]
}

pub fn system_prompt(workspace: &str) -> String {
    format!(
        "You are nanomite, a terse terminal coding agent working in {workspace}.\n\
         You have tools: file access (list_files, read_file, write_file, edit_file, search),\n\
         a workspace shell (run_command), SSH to remote hosts (ssh_run) and web access\n\
         (web_fetch, web_search), a headless browser for JavaScript-rendered pages\n\
         (browse, browse_screenshot), git (git_status, git_diff, git_log, git_commit, git_branch),\n         and test runner (run_tests), symbol index (index_symbols, find_symbol, find_references), plan (create_plan, get_plan, update_task, list_plans, delete_plan).\n\
         Prefer reading files before answering. Cite paths as `path:line`.\n\
         Keep answers short: no preamble, no restating the request."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_content_deltas_surface_as_events() {
        let block = r#"data: {"choices":[{"delta":{"reasoning_content":"thinking hard"}}]}"#;
        let event = parse_event(block).expect("event").expect("ok");
        assert!(
            matches!(event, StreamEvent::ReasoningDelta(ref d) if d == "thinking hard"),
            "wrong event"
        );
    }

    #[test]
    fn answer_content_wins_over_reasoning_in_one_delta() {
        let block = r#"data: {"choices":[{"delta":{"content":"ans","reasoning_content":"th"}}]}"#;
        let event = parse_event(block).expect("event").expect("ok");
        assert!(
            matches!(event, StreamEvent::Delta(ref d) if d == "ans"),
            "wrong event"
        );
    }
}
