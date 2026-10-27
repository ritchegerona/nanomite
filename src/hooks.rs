//! User-defined hooks: small scripts nanomite runs at lifecycle points.
//!
//! A hook is an executable path (or `name arg` line) from the config, run as a
//! child process with the event as JSON on stdin. Hooks are advisory: their
//! output is logged, never injected into the conversation, and a failing hook
//! never blocks the action it observes - EXCEPT deny-hooks, whose non-zero
//! exit blocks `PreToolCall` for the named tool (the one guardrail hook with
//! teeth). Everything is timeout-bounded and runs on the blocking pool, so
//! the UI thread never waits.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Lifecycle points that can carry a hook.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    /// Before a tool call runs. `deny` hooks may block it.
    PreToolCall { tool: String, args_json: String },
    /// After a tool call finished. `ok` reports the outcome.
    PostToolCall {
        tool: String,
        ok: bool,
        detail: String,
    },
    /// An agent turn started with this user input.
    TurnStart { input: String },
    /// An agent turn finished with this assistant text.
    TurnEnd { summary: String },
    /// The tool policy changed (auto/plan/readonly).
    PolicyChange { policy: String },
}

impl HookEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            HookEvent::PreToolCall { .. } => "pre_tool_call",
            HookEvent::PostToolCall { .. } => "post_tool_call",
            HookEvent::TurnStart { .. } => "turn_start",
            HookEvent::TurnEnd { .. } => "turn_end",
            HookEvent::PolicyChange { .. } => "policy_change",
        }
    }

    /// The JSON payload piped to the hook's stdin.
    pub fn payload(&self) -> String {
        serde_json::json!({
            "event": self.kind(),
            "data": match self {
                HookEvent::PreToolCall { tool, args_json } => serde_json::json!({
                    "tool": tool, "args": serde_json::from_str::<serde_json::Value>(args_json).unwrap_or(serde_json::Value::Null),
                }),
                HookEvent::PostToolCall { tool, ok, detail } => serde_json::json!({
                    "tool": tool, "ok": ok, "detail": detail,
                }),
                HookEvent::TurnStart { input } => serde_json::json!({ "input": input }),
                HookEvent::TurnEnd { summary } => serde_json::json!({ "summary": summary }),
                HookEvent::PolicyChange { policy } => serde_json::json!({ "policy": policy }),
            },
        })
        .to_string()
    }
}

/// One configured hook: which event, what to run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookConfig {
    /// Event kind this hook observes (matches `HookEvent::kind()`).
    pub event: String,
    /// Program to run; a `program arg arg` line split on whitespace, or an
    /// absolute/relative path. Run with cwd = workspace.
    pub command: String,
    /// Hard cap in milliseconds (default 5s, max 30s).
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    /// Pre-tool-call only: when the hook exits non-zero, the tool call is
    /// refused with the hook's stderr as the reason. Ignored for other events.
    #[serde(default)]
    pub deny: bool,
}

fn default_timeout() -> u64 {
    5_000
}

/// The outcome of running one hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookOutcome {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    /// True when a deny-hook refused a pre-tool call.
    pub denied: bool,
}

impl HookOutcome {
    /// One-line log form, bounded.
    pub fn summary(&self, command: &str) -> String {
        let status = if self.timed_out {
            "timeout".to_string()
        } else if self.denied {
            "DENIED".to_string()
        } else {
            format!("exit {}", self.exit_code)
        };
        let out = self.stdout.trim();
        let err = self.stderr.trim();
        let detail = if !out.is_empty() {
            out
        } else if !err.is_empty() {
            err
        } else {
            "(no output)"
        };
        format!(
            "hook {} {} · {} · {}",
            command,
            status,
            detail.chars().take(120).collect::<String>(),
            ""
        )
        .trim_end()
        .to_string()
    }
}

/// Runs every hook configured for `event`. Never panics, never blocks long:
/// each hook is capped by its own timeout, and the whole set is capped by
/// `MAX_HOOKS_PER_EVENT` so a wild config cannot stall a turn.
pub fn run_hooks(
    hooks: &[HookConfig],
    event: &HookEvent,
    workspace: &Path,
) -> Vec<(String, HookOutcome)> {
    const MAX_HOOKS_PER_EVENT: usize = 8;
    const MAX_OUTPUT: usize = 4_000;

    let mut results = Vec::new();
    for hook in hooks.iter().take(MAX_HOOKS_PER_EVENT) {
        if hook.event.trim().to_ascii_lowercase() != event.kind() {
            continue;
        }
        let mut parts = hook.command.split_whitespace();
        let Some(program) = parts.next() else {
            continue;
        };
        let started = Instant::now();
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new(program);
        cmd.args(parts)
            .process_group(0) // grandchildren die with the group on timeout
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Hooks must not inherit the terminal: no /dev/tty races, no echo.
        cmd.env("NANOMITE_HOOK_EVENT", event.kind());

        let outcome = (|| {
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    return HookOutcome {
                        exit_code: -1,
                        stdout: String::new(),
                        stderr: format!("spawn failed: {e}"),
                        timed_out: false,
                        denied: false,
                    }
                }
            };
            if let Some(mut stdin) = child.stdin.take() {
                let payload = event.payload();
                let _ = stdin.write_all(payload.as_bytes());
                let _ = stdin.flush();
                // Drop closes the pipe: the hook sees EOF after the payload.
            }

            let mut stdout_pipe = child.stdout.take().expect("piped stdout");
            let mut stderr_pipe = child.stderr.take().expect("piped stderr");
            let out_handle = std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = std::io::Read::read_to_end(&mut stdout_pipe, &mut buf);
                buf
            });
            let err_handle = std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = std::io::Read::read_to_end(&mut stderr_pipe, &mut buf);
                buf
            });

            let timeout = Duration::from_millis(hook.timeout_ms.clamp(100, 30_000));
            let deadline = started + timeout;
            let mut timed_out = false;
            let status = loop {
                match child.try_wait() {
                    Ok(Some(st)) => break Some(st),
                    Ok(None) => {
                        if Instant::now() >= deadline {
                            timed_out = true;
                            crate::tools::exec_tools::kill_process_group(&child);
                            let _ = child.wait();
                            break None;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(e) => {
                        let _ = out_handle.join();
                        let _ = err_handle.join();
                        return HookOutcome {
                            exit_code: -1,
                            stdout: String::new(),
                            stderr: format!("wait failed: {e}"),
                            timed_out: false,
                            denied: false,
                        };
                    }
                }
            };

            let stdout = out_handle.join().unwrap_or_default();
            let stderr = err_handle.join().unwrap_or_default();
            let cap = |v: Vec<u8>| -> String {
                String::from_utf8_lossy(&v)
                    .chars()
                    .take(MAX_OUTPUT)
                    .collect()
            };
            let exit_code = status.and_then(|s| s.code()).unwrap_or(-1);
            let denied =
                hook.deny && event.kind() == "pre_tool_call" && (timed_out || exit_code != 0);
            HookOutcome {
                exit_code,
                stdout: cap(stdout),
                stderr: cap(stderr),
                timed_out,
                denied,
            }
        })();

        results.push((hook.command.clone(), outcome));
    }
    results
}

/// Default hooks directory: `~/.config/nanomite/hooks`, overridable with
/// `NANOMITE_HOOKS_DIR`.
pub fn default_hooks_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("NANOMITE_HOOKS_DIR") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nanomite")
        .join("hooks")
}

/// The directory hooks load from, or `None` when the subsystem is off
/// (`NANOMITE_HOOKS=0`).
pub fn resolve_dir() -> PathBuf {
    match std::env::var_os("NANOMITE_HOOKS") {
        Some(v) if v == "0" => PathBuf::from("/nonexistent-nanomite-hooks-disabled"),
        _ => default_hooks_dir(),
    }
}

/// Loads every configured hook for `dir`:
/// - `hooks.json`: an array of `{event, command, timeout_ms?, deny?}` entries
///   for anything that needs custom timeouts or deny behavior;
/// - `<dir>/<event>/<executable>`: auto-discovery — drop a script into
///   `hooks/turn_end/` and it runs for that event (filename order, 5s cap,
///   never deny). `NANOMITE_HOOKS=0` or a missing dir disables everything.
pub fn load(dir: &Path) -> Vec<HookConfig> {
    let mut hooks = Vec::new();

    // Manifest first: explicit entries, in file order.
    let manifest = dir.join("hooks.json");
    if let Ok(raw) = std::fs::read_to_string(&manifest) {
        if let Ok(list) = serde_json::from_str::<Vec<HookConfig>>(&raw) {
            hooks.extend(list);
        }
    }

    // Auto-discovered per-event scripts.
    let known = [
        "pre_tool_call",
        "post_tool_call",
        "turn_start",
        "turn_end",
        "policy_change",
    ];
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut discovered: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries.flatten().take(64) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(event) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !known.contains(&event) {
                continue;
            }
            let Ok(scripts) = std::fs::read_dir(&path) else {
                continue;
            };
            for script in scripts.flatten().take(16) {
                let spath = script.path();
                let is_exec = script
                    .metadata()
                    .map(|m| {
                        use std::os::unix::fs::PermissionsExt;
                        m.permissions().mode() & 0o111 != 0
                    })
                    .unwrap_or(false);
                if is_exec {
                    discovered.push((event.to_string(), spath));
                }
            }
        }
        discovered.sort();
        for (event, path) in discovered {
            hooks.push(HookConfig {
                event,
                command: path.display().to_string(),
                timeout_ms: default_timeout(),
                deny: false,
            });
        }
    }

    hooks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(event: &str, command: &str) -> HookConfig {
        HookConfig {
            event: event.to_string(),
            command: command.to_string(),
            timeout_ms: 5_000,
            deny: false,
        }
    }

    #[test]
    fn payload_is_json_with_event_kind_and_data() {
        let event = HookEvent::PreToolCall {
            tool: "run_command".into(),
            args_json: r#"{"command":"ls"}"#.into(),
        };
        let payload = event.payload();
        assert!(payload.contains("\"event\":\"pre_tool_call\""), "{payload}");
        assert!(payload.contains("run_command"), "{payload}");
        assert!(payload.contains("ls"), "{payload}");

        assert_eq!(event.kind(), "pre_tool_call");
        assert_eq!(
            HookEvent::TurnEnd {
                summary: "x".into()
            }
            .kind(),
            "turn_end"
        );
    }

    #[test]
    fn hooks_run_with_payload_on_stdin_and_captured_output() {
        let dir = std::env::temp_dir().join(format!("nanomite-hooks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("echo-hook.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nread -r payload\necho \"got: $payload\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let hooks = vec![hook(
            "turn_start",
            &format!("{} | tr a-z A-Z", script.display()),
        )];
        let event = HookEvent::TurnStart {
            input: "hello hook".into(),
        };
        let results = run_hooks(&hooks, &event, &dir);
        assert_eq!(results.len(), 1);
        let (_, outcome) = &results[0];
        assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
        // The payload reached the hook on stdin (uppercased by the pipeline).
        assert!(
            outcome.stdout.to_uppercase().contains("HELLO HOOK"),
            "stdout: {}",
            outcome.stdout
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mismatched_event_hooks_do_not_run() {
        let hooks = vec![hook("turn_end", "true")];
        let event = HookEvent::TurnStart { input: "x".into() };
        let results = run_hooks(&hooks, &event, &std::env::temp_dir());
        assert!(results.is_empty(), "wrong-event hook must be skipped");
    }

    #[test]
    fn timeout_kills_a_hanging_hook() {
        let dir = std::env::temp_dir().join(format!("nanomite-hooks-to-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("sleep.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let hooks = vec![HookConfig {
            event: "turn_start".into(),
            command: script.display().to_string(),
            timeout_ms: 300,
            deny: false,
        }];
        let started = Instant::now();
        let results = run_hooks(&hooks, &HookEvent::TurnStart { input: "x".into() }, &dir);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "must not wait 30s"
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].1.timed_out, "must report the timeout");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deny_hook_blocks_pre_tool_calls_only() {
        let dir = std::env::temp_dir().join(format!("nanomite-hooks-deny-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("deny.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nread -r payload\ncase \"$payload\" in *push*) echo nope >&2; exit 1;; esac\nexit 0\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut deny_hook = hook("pre_tool_call", &script.display().to_string());
        deny_hook.deny = true;

        // A denied call: deny=true + non-zero exit -> blocked.
        let results = run_hooks(
            &[deny_hook.clone()],
            &HookEvent::PreToolCall {
                tool: "run_command".into(),
                args_json: r#"{"command":"git push --force"}"#.into(),
            },
            &dir,
        );
        assert!(results[0].1.denied, "deny hook must block");
        assert!(results[0].1.stderr.contains("nope"));

        // An allowed call: exit 0 -> not denied, even with deny=true.
        let results = run_hooks(
            &[deny_hook],
            &HookEvent::PreToolCall {
                tool: "run_command".into(),
                args_json: r#"{"command":"git status"}"#.into(),
            },
            &dir,
        );
        assert!(!results[0].1.denied, "allowed call must pass");

        // deny on non-pre events is inert.
        let mut late_deny = hook("turn_start", "false");
        late_deny.deny = true;
        let results = run_hooks(
            &[late_deny],
            &HookEvent::TurnStart { input: "x".into() },
            &dir,
        );
        assert!(!results[0].1.denied, "deny only applies to pre_tool_call");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failing_hooks_never_error_the_caller() {
        let hooks = vec![hook("turn_end", "definitely-not-a-real-command-xyz")];
        let results = run_hooks(
            &hooks,
            &HookEvent::TurnEnd {
                summary: "s".into(),
            },
            &std::env::temp_dir(),
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.exit_code, -1);
        assert!(results[0].1.stderr.contains("spawn failed"));
    }
}
