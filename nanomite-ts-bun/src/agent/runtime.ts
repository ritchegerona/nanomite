/* Agent Runtime - ported from src/agent/runtime.rs
 * Core turn loop with policy gating and tool dispatch.
 * Imports tool schemas from the catalog module.
 */

import { Config, ToolPolicy, Risk, StreamEventType, ChatMessage, Usage, StreamEvent } from "./";
import { toolSchema, toolRisk, ALL_TOOL_SCHEMAS } from "./tools/catalog";
import * as fs from "fs";
import * as path from "path";
import * as readline from "readline";

// ---------------------------------------------------------------------------
// Cancel: cooperative two-stage interrupt
// ---------------------------------------------------------------------------

export class Cancel {
  cancelled = false;
  cancel() { this.cancelled = true; }
  force_stop() { this.cancel(); }
  check() { return this.cancelled; }
}

// ---------------------------------------------------------------------------
// AgentConfig
// ---------------------------------------------------------------------------

export interface AgentConfig {
  max_steps: number;
  step_timeout_ms: number;
  max_tool_calls: number;
  max_message_chars: number;
  offline: boolean;
}

// ---------------------------------------------------------------------------
// AgentEvent types
// ---------------------------------------------------------------------------

export enum AgentEventType {
  Started, Step, Delta, ReasoningDelta,
  ToolStart, ToolOutput, ToolEnd,
  Snapshot, FileCreated, FileModified, FileDeleted, DiffUpdate,
  Activity, Finished, Interrupted, Failed,
}

export interface AgentEvent {
  type: AgentEventType;
  index?: number; content?: string; reasoning_content?: string;
  name?: string; detail?: string; preview?: string;
  stdout?: string; stderr?: string; ok?: boolean;
  path?: string; previous?: any; diff?: string | null;
  action?: string; detail?: string; done?: boolean;
  text?: string; usage?: Usage; elapsed?: number;
  message?: string;
}

// ---------------------------------------------------------------------------
// Tool authorization
// ---------------------------------------------------------------------------

export interface AuthResult {
  allowed: boolean;
  mode: string;
  error_message: string;
}

export function authorizeTool(policy: ToolPolicy, name: string): AuthResult {
  const trimmed = name.trim().toLowerCase();
  if (policy === ToolPolicy.Hacker) return { allowed: true, mode: "hacker", error_message: "" };
  if (policy === ToolPolicy.Build) {
    const build = ["write_file", "edit_file", "index_symbols", "propose_self_edit",
      "memory_save", "memory_update", "run_command", "git_commit", "git_branch", "delete_plan"];
    if (build.some(t => t === trimmed)) return { allowed: true, mode: "build", error_message: "" };
    const risk = toolRisk(trimmed);
    if (risk === Risk.Safe || risk === Risk.Write) return { allowed: true, mode: "build", error_message: "" };
    return { allowed: false, mode: "build",
      error_message: `{"error":"build mode: remote (SSH) access is disabled. Everything local works - ask the user to press Shift+Tab for hacker mode to run remote commands.","code":"build_mode"}` };
  }
  if (policy === ToolPolicy.Coder) {
    const coder = ["write_file", "edit_file", "index_symbols", "propose_self_edit",
      "memory_save", "memory_update"];
    if (coder.some(t => t === trimmed)) return { allowed: true, mode: "coder", error_message: "" };
    return { allowed: false, mode: "coder",
      error_message: `{"error":"coder mode: command execution and git history changes are disabled. Edit files freely; ask the user to press Shift+Tab for build mode to run commands or commit.","code":"coder_mode"}` };
  }
  if (policy === ToolPolicy.Plan) {
    const plan = ["create_plan", "get_plan", "update_task", "list_plans"];
    if (plan.some(t => t === trimmed)) return { allowed: true, mode: "plan", error_message: "" };
    const risk = toolRisk(trimmed);
    if (risk === Risk.Safe) return { allowed: true, mode: "plan", error_message: "" };
    return { allowed: false, mode: "plan",
      error_message: `{"error":"plan mode is active: mutating tools are disabled. Create a plan first with create_plan (allowed in this mode), describe each step and what you would run, then wait for the user to review it and switch back with Shift+Tab.","code":"plan_mode"}` };
  }
  return { allowed: false, mode: "plan", error_message: "unknown mode" };
}

function getRisk(name: string): Risk {
  const external = ["run_command", "ssh_run", "memory_delete", "browse", "browse_screenshot",
    "run_tests", "vpn_control", "tor_control"];
  const write = ["write_file", "edit_file", "memory_save", "memory_update", "propose_self_edit",
    "git_commit", "git_branch", "index_symbols", "create_plan", "update_task",
    "delete_plan", "read_document"];
  if (external.some(t => t === name)) return Risk.External;
  if (write.some(t => t === name)) return Risk.Write;
  return Risk.Safe;
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

export async function runTool(
  toolName: string,
  argumentsJson: string,
  policy: ToolPolicy,
  disabledTools: string[],
  workspace: string
): Promise<{ payload: string; summary: string }> {
  const requested = toolName.trim().toLowerCase();
  if (disabledTools.includes(requested)) {
    throw new Error(`{"error":"tool disabled by configuration","code":"tool_disabled"}`);
  }
  const auth = authorizeTool(policy, toolName);
  if (!auth.allowed) throw new Error(auth.error_message);

  switch (toolName) {
    case "list_files": {
      const args = JSON.parse(argumentsJson);
      const where_ = args.path || ".";
      try {
        const files = fs.readdirSync(path.resolve(workspace, where_));
        const result = files.map(f => ({ name: f, path: path.join(where_, f) }));
        return { payload: JSON.stringify(result), summary: `${result.length} files + dirs` };
      } catch { throw new Error("list_files failed"); }
    }
    case "read_file": {
      const args = JSON.parse(argumentsJson);
      const path_ = args.path || "";
      if (!path_ || path_.startsWith("..") || path_.startsWith("/")) throw new Error("bad read_file arguments");
      try {
        const content = fs.readFileSync(path.resolve(workspace, path_), "utf-8");
        return { payload: content, summary: `${Math.min(content.split("\n").length, 1000)} lines` };
      } catch { throw new Error("read_file failed"); }
    }
    case "write_file": {
      const args = JSON.parse(argumentsJson);
      const path_ = args.path || "";
      const content = args.content || "";
      if (!path_ || path_.startsWith("..") || path_.startsWith("/")) throw new Error("bad write_file arguments");
      try {
        const dir = path.dirname(path.resolve(workspace, path_));
        if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
        fs.writeFileSync(path.resolve(workspace, path_), content, "utf-8");
        return { payload: content, summary: `${content.length} bytes written` };
      } catch { throw new Error("write_file failed"); }
    }
    case "edit_file": {
      const args = JSON.parse(argumentsJson);
      const path_ = args.path || "";
      const old_s = args.old_string || "";
      const new_s = args.new_string || "";
      if (!path_ || path_.startsWith("..") || path_.startsWith("/")) throw new Error("bad edit_file arguments");
      try {
        if (!fs.existsSync(path.resolve(workspace, path_))) throw new Error("file not found");
        const content = fs.readFileSync(path.resolve(workspace, path_), "utf-8");
        fs.writeFileSync(path.resolve(workspace, path_), content.replace(old_s, new_s), "utf-8");
        return { payload: content, summary: "1 replacement(s) made" };
      } catch { throw new Error("edit_file failed"); }
    }
    case "search": {
      const args = JSON.parse(argumentsJson);
      const query = args.query || "";
      const path_ = args.path || workspace;
      try {
        const files = fs.readdirSync(path.resolve(workspace, path_));
        const matches: any[] = [];
        for (const f of files) {
          const ff = path.resolve(workspace, path_, f);
          if (fs.lstatSync(ff).isFile()) {
            const lines = fs.readFileSync(ff, "utf-8").split("\n");
            for (let i = 0; i < lines.length; i++) {
              if (lines[i].toLowerCase().includes(query.toLowerCase())) matches.push({ file: f, line: i + 1, content: lines[i] });
            }
          }
        }
        return { payload: JSON.stringify({ hits: matches.length, files_scanned: files.length, hits: matches }), summary: `${matches.length} hits / ${files.length} files` };
      } catch { throw new Error("search failed"); }
    }
    case "run_command": {
      const args = JSON.parse(argumentsJson);
      const command = args.command || "";
      const cwd = args.cwd || workspace;
      const timeoutMs = args.timeout_ms ?? 60000;
      if (!command) throw new Error("bad run_command arguments");
      try {
        const { execSync } = await import("child_process");
        const result = execSync(command, { cwd: path.resolve(cwd), timeout: timeoutMs, encoding: "utf-8" });
        return { payload: JSON.stringify({ exit_code: result.status, stdout: result.stdout, stderr: result.stderr }), summary: result.status === 0 ? "success" : "exit " + result.status };
      } catch (e: any) { throw new Error(`run_command failed: ${e.message || String(e)}`); }
    }
    case "git_status": {
      try { const { execSync } = await import("child_process"); const r = execSync("git status --porcelain", { cwd: workspace, encoding: "utf-8" }); return { payload: JSON.stringify({ status: r.trim() }), summary: "git status" }; } catch { throw new Error("git_status failed"); }
    }
    case "git_diff": {
      try { const { execSync } = await import("child_process"); const r = execSync("git diff", { cwd: workspace, encoding: "utf-8" }); return { payload: JSON.stringify({ diff: r }), summary: "git diff" }; } catch { throw new Error("git_diff failed"); }
    }
    case "git_commit": {
      const args = JSON.parse(argumentsJson);
      try { const { execSync } = await import("child_process"); const r = execSync(`git commit -m "${args.message}"`, { cwd: workspace, encoding: "utf-8" }); return { payload: JSON.stringify({ commit: r.stdout }), summary: "1 file(s) committed" }; } catch { throw new Error("git_commit failed"); }
    }
    case "index_symbols": {
      try { const { execSync } = await import("child_process"); const r = execSync("ctags -R --exclude='*.git' --languages=javascript,typescript 2>/dev/null || echo done", { cwd: workspace, encoding: "utf-8" }); return { payload: JSON.stringify({ indexed: r.trim() }), summary: "symbols indexed" }; } catch { throw new Error("index_symbols failed"); }
    }
    case "find_symbol": {
      const args = JSON.parse(argumentsJson);
      try { const { execSync } = await import("child_process"); const r = execSync(`grep -r "${args.query}" --include="*.ts" --include="*.js" ${workspace} 2>/dev/null | head -20`, { cwd: workspace, encoding: "utf-8" }); return { payload: JSON.stringify({ references: r.trim() }), summary: `${r.trim().split("\n").length} results` }; } catch { throw new Error("find_symbol failed"); }
    }
    case "find_references": {
      const args = JSON.parse(argumentsJson);
      try { const { execSync } = await import("child_process"); const r = execSync(`grep -rn "${args.symbol}" ${workspace} 2>/dev/null | head -30`, { cwd: workspace, encoding: "utf-8" }); return { payload: JSON.stringify({ references: r.trim() }), summary: `${r.trim().split("\n").length} results` }; } catch { throw new Error("find_references failed"); }
    }
    case "memory_save": {
      const args = JSON.parse(argumentsJson);
      const summary = `memory saved: ${args.key} -> ${args.value?.substring(0, 50)}${args.value?.length > 50 ? "..." : ""}`;
      return { payload: JSON.stringify({ key: args.key, value: args.value }), summary };
    }
    case "memory_get": {
      const args = JSON.parse(argumentsJson);
      const summary = `memory retrieved: ${args.key}`;
      return { payload: JSON.stringify({ key: args.key, value: "dummy value" }), summary };
    }
    case "memory_list": {
      const summary = "memory list: (empty)";
      return { payload: JSON.stringify({ entries: [] }), summary };
    }
    case "propose_self_edit": {
      const args = JSON.parse(argumentsJson);
      const summary = `propose_self_edit staged for: ${args.target}`;
      return { payload: JSON.stringify({ target: args.target, new_value: args.new_value }), summary };
    }
    case "create_plan": {
      const args = JSON.parse(argumentsJson);
      const summary = `plan created: ${args.name} with ${args.tasks?.length || 0} task(s)`;
      return { payload: JSON.stringify({ name: args.name, description: args.description, tasks: args.tasks }), summary };
    }
    case "get_plan": {
      const summary = "plan retrieved: (placeholder)";
      return { payload: JSON.stringify({ plans: [] }), summary };
    }
    case "update_task": {
      const args = JSON.parse(argumentsJson);
      const summary = `task ${args.task_id ?? 0} updated to ${args.status || "pending"}`;
      return { payload: { task_id: args.task_id ?? 0, status: args.status || "pending" }, summary };
    }
    case "list_plans": {
      const summary = "plans listed: (no plans loaded)";
      return { payload: JSON.stringify({ plans: [] }), summary };
    }
    case "delete_plan": {
      const args = JSON.parse(argumentsJson);
      const summary = `plan deletion proposed: ${args.name}`;
      return { payload: { name: args.name, confirmed: false }, summary };
    }
    case "read_document": {
      const args = JSON.parse(argumentsJson);
      const path_ = args.path || "";
      if (!path_ || path_.startsWith("..") || path_.startsWith("/")) throw new Error("bad read_document arguments");
      try {
        const fp = path.resolve(workspace, path_);
        if (fp.toLowerCase().endsWith(".png") || fp.toLowerCase().endsWith(".jpg") ||
            fp.toLowerCase().endsWith(".jpeg") || fp.toLowerCase().endsWith(".tif") ||
            fp.toLowerCase().endsWith(".bmp") || fp.toLowerCase().endsWith(".webp")) {
          return { payload: { format: "image", chars: 0, truncated: false, language: "eng", path: fp }, summary: "ocr not implemented" };
        }
        const content = fs.readFileSync(fp, "utf-8");
        return { payload: { format: "text", chars: content.length, truncated: false, language: "eng", path: fp, content }, summary: `${content.split("\n").length} lines · text` };
      } catch { throw new Error("read_document failed"); }
    }
    case "ocr_image": {
      const args = JSON.parse(argumentsJson);
      return { payload: { text: "OCR result would appear here", language: args.lang || "eng", chars: 0, truncated: false, path: args.path || "" }, summary: "ocr: tesseract not available" };
    }
    case "web_fetch": {
      const args = JSON.parse(argumentsJson);
      const url = args.url || "";
      if (!url) throw new Error("bad web_fetch arguments");
      return { payload: { url, text: "", status: 0 }, summary: `web_fetch: ${url} not implemented` };
    }
    case "web_search": {
      const args = JSON.parse(argumentsJson);
      const summary = `web_search: ${args.query} not implemented`;
      return { payload: { query: args.query, results: [] }, summary };
    }
    case "ip_lookup": {
      const args = JSON.parse(argumentsJson);
      const ip = args.ip || "";
      const summary = ip === "me" || ip === "" ? "ip_lookup: your public IP" : `ip_lookup: ${ip} not implemented`;
      return { payload: { ip, location: "unknown", isp: "unknown", timezone: "unknown" }, summary };
    }
    case "browse": {
      const args = JSON.parse(argumentsJson);
      return { payload: { url: args.url || "", text: "" }, summary: "browse: headless Chromium not implemented" };
    }
    case "browse_screenshot": {
      const args = JSON.parse(argumentsJson);
      return { payload: { url: args.url || "", output: `screenshot-${Date.now()}.png`, path: `screenshot-${Date.now()}.png` }, summary: "browse_screenshot not implemented" };
    }
    case "git_branch": {
      const args = JSON.parse(argumentsJson);
      try { const { execSync } = await import("child_process"); const r = execSync("git branch", { cwd: workspace, encoding: "utf-8" }); return { payload: JSON.stringify({ branches: r.trim().split("\n") }), summary: "git branch listed" }; } catch { throw new Error("git_branch failed"); }
      const summary = `git_branch: ${args.name}`;
      return { payload: { name: args.name, deleted: false }, summary };
    }
    case "run_tests": {
      const args = JSON.parse(argumentsJson);
      const runner = args.runner || "custom";
      const cwd = args.cwd || workspace;
      const timeoutMs = args.timeout_ms ?? 120000;
      let output = "";
      let exitCode = 0;
      try { const { execSync } = await import("child_process"); output = execTimeout(runner, cwd, timeoutMs, args.command); exitCode = 0; } catch (e) { output = ((e as any)?.stdout || (e as any)?.stderr || (e as any)?.message || "unknown"); exitCode = 1; }
      return { payload: { output, exit_code: exitCode }, summary: exitCode === 0 ? "tests ran" : `tests failed: ${output.substring(0, 100)}${output.length > 100 ? "..." : ""}` };
    }
    default: throw new Error(`unknown tool: ${toolName}`);
  }
}

function execTimeout(runner: string, cwd: string, timeoutMs: number, command: string): string {
  const { execSync } = await import("child_process");
  switch (runner) {
    case "cargo": return execSync("cargo test --quiet", { cwd: path.resolve(cwd), timeout: timeoutMs, encoding: "utf-8" }).toString();
    case "npm": return execSync("npm test", { cwd: path.resolve(cwd), timeout: timeoutMs, encoding: "utf-8" }).toString();
    case "pytest": return execSync("pytest --quiet", { cwd: path.resolve(cwd), timeout: timeoutMs, encoding: "utf-8" }).toString();
    case "jest": return execSync("npx jest --quiet", { cwd: path.resolve(cwd), timeout: timeoutMs, encoding: "utf-8" }).toString();
    case "go": return execSync("go test -quiet", { cwd: path.resolve(cwd), timeout: timeoutMs, encoding: "utf-8" }).toString();
    default: return execSync(command || "bash -c '" + command + "'", { cwd: path.resolve(cwd), timeout: timeoutMs, encoding: "utf-8" }).toString();
  }
}

// ---------------------------------------------------------------------------
// AgentRuntime
// ---------------------------------------------------------------------------

export class AgentRuntime {
  private config: AgentConfig;
  private policy: ToolPolicy;
  private disabledTools: Set<string>;
  private workspace: string;
  private onEvent: ((event: any) => void) | null = null;

  constructor(config: AgentConfig, policy: ToolPolicy, disabledTools: string[], workspace: string) {
    this.config = config;
    this.policy = policy;
    this.disabledTools = new Set(disabledTools.map(t => t.trim().toLowerCase()));
    this.workspace = workspace;
  }

  setOnEvent(onEvent: (event: any) => void): void { this.onEvent = onEvent; }
  private emit(e: any) { this.onEvent?.(e); }

  private step(i: number) { this.emit({ type: 0, index: i }); }
  private delta(c: string) { this.emit({ type: 1, content: c }); }
  private reasoningDelta(c: string) { this.emit({ type: 2, reasoning_content: c }); }
  private toolStart(n: string, d: string, p: string | null) { this.emit({ type: 3, name: n, detail: d, preview: p }); }
  private toolOutput(n: string, s: string, err: string) { this.emit({ type: 4, name: n, stdout: s, stderr: err }); }
  private toolEnd(n: string, d: string, ok: boolean) { this.emit({ type: 5, name: n, detail: d, ok }); }
  private snapshot(p: string, prev: any) { this.emit({ type: 6, path: p, previous: prev }); }
  private fileCreated(p: string) { this.emit({ type: 7, path: p }); }
  private fileModified(p: string, d: string | null) { this.emit({ type: 8, path: p, diff: d }); }
  private fileDeleted(p: string) { this.emit({ type: 9, path: p }); }
  private diffUpdate(p: string, d: string) { this.emit({ type: 10, path: p, diff: d }); }
  private activity(a: string, d: string | null, done: boolean) { this.emit({ type: 11, action: a, detail: d, done }); }
  private finished(t: string, u: any, el: number) { this.emit({ type: 12, text: t, usage: u, elapsed: el }); }
  private interrupted() { this.emit({ type: 13 }); }
  private failed(m: string) { this.emit({ type: 14, message: m }); }

  private buildSystemPrompt(): string {
    const preset = Config.findPreset(this.config.provider);
    const reasonStr = this.config.reasoning !== undefined ? (this.config.reasoning as any) || (preset.reasoning || "minimal") : (preset.reasoning || "minimal");
    let prompt = `You are an AI assistant with access to tools. You have ${this.config.max_tool_calls} tool calls available across ${this.config.max_steps} steps.

Available tools and their risk classifications:

`;
    const filtered = ALL_TOOL_SCHEMAS.filter(t => !this.disabledTools.has(t.name.toLowerCase()));
    prompt += filtered.map(t => `- ${t.name} (${t.risk}: ${t.description})`).join("\n");
    prompt += `
Tool policy mode: ${this.policy.as_str()}
Use Shift+Tab to cycle modes: Plan ⊂ Coder ⊂ Build ⊂ Hacker.
Think step by step inside <thoughts>...</thoughts> tags, then give your final answer directly after the closing tag.
Be direct and detailed: give comprehensive, informative answers with specific details, step-by-step instructions, examples, and technical depth.
`;
    return prompt;
  }

  async run(userInput: string, history: ChatMessage[], cancel: Cancel): Promise<{ usage: Usage | null; elapsed: number; cancelled: boolean }> {
    const started = Date.now();
    let toolCallsMade = 0;
    let steps = 0;
    const toolsUsed: string[] = [];

    this.emit({ type: "Started" });

    const messages: ChatMessage[] = [
      { role: "system", content: this.buildSystemPrompt() },
    ];
    messages.push(...history);
    messages.push({ role: "user", content: userInput });

    // NOTE: Full stream chat implementation would go here.
    // For this port, the model response is simulated.
    // In a full implementation, would call this.provider.streamChat()

    // Simulate a simple response for demonstration
    const simulatedResponse = "I understand your request. I'll help you with that task.";
    const simulatedUsage: Usage = { prompt_tokens: 50, completion_tokens: 30, total_tokens: 80 };

    this.finished(simulatedResponse, simulatedUsage, Date.now() - started);
    return { usage: simulatedUsage, elapsed: Date.now() - started, cancelled: false };
  }
}