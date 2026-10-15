//! Shell and remote-execution tools: workspace-scoped `run_command` and `ssh_run`.
//!
//! Both are sync by design — the runtime dispatches every tool call through
//! `tokio::task::spawn_blocking`, so blocking here never stalls the UI.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Largest stdout/stderr payload returned to the model (chars, not bytes).
const MAX_OUTPUT: usize = 16 * 1024;
/// Wall-clock ceiling for one command.
const MAX_TIMEOUT_MS: u64 = 120_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCommandArgs {
    /// Shell command line, run through `sh -c` inside the workspace.
    pub command: String,
    /// Optional workspace-relative working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Hard cap in milliseconds (max 120s).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCommandResult {
    pub command: String,
    pub cwd: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshRunArgs {
    /// `user@host` or an ssh-config alias.
    pub host: String,
    /// Command line to execute on the remote host.
    pub command: String,
    /// Hard cap in milliseconds (max 120s).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshRunResult {
    pub host: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}

pub struct ExecTools {
    root: PathBuf,
}

impl ExecTools {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolves a workspace-relative cwd; refuses anything that escapes it.
    fn resolve_cwd(&self, cwd: Option<&str>) -> Result<PathBuf, String> {
        match cwd {
            None => Ok(self.root.clone()),
            Some(raw) => {
                let raw = raw.trim();
                if raw.is_empty() || raw == "." {
                    return Ok(self.root.clone());
                }
                let p = Path::new(raw);
                let joined = if p.is_absolute() {
                    PathBuf::from(raw)
                } else {
                    self.root.join(p)
                };
                let normalized = crate::tools::file_tools::normalize(&joined);
                if !normalized.starts_with(&self.root) {
                    return Err("refused: cwd escapes the workspace".into());
                }
                Ok(normalized)
            }
        }
    }

    pub fn run_command(&self, args: RunCommandArgs) -> Result<RunCommandResult, String> {
        if args.command.trim().is_empty() {
            return Err("refused: empty command".into());
        }
        let dir = self.resolve_cwd(args.cwd.as_deref())?;
        let timeout = Duration::from_millis(
            args.timeout_ms
                .unwrap_or(60_000)
                .clamp(1_000, MAX_TIMEOUT_MS),
        );

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&args.command)
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (stdout, stderr, code, timed_out) =
            run_with_timeout(&mut cmd, "sh", timeout, &args.command)?;
        Ok(RunCommandResult {
            command: args.command,
            cwd: dir.to_string_lossy().into_owned(),
            stdout,
            stderr,
            exit_code: code,
            timed_out,
        })
    }

    pub fn ssh_run(&self, args: SshRunArgs) -> Result<SshRunResult, String> {
        let host = args.host.trim().to_string();
        if host.is_empty() || host.contains(char::is_whitespace) {
            return Err("refused: host must be a single token (user@host)".into());
        }
        if args.command.trim().is_empty() {
            return Err("refused: empty command".into());
        }
        let timeout = Duration::from_millis(
            args.timeout_ms
                .unwrap_or(30_000)
                .clamp(1_000, MAX_TIMEOUT_MS),
        );

        let mut cmd = Command::new("ssh");
        cmd.args([
            "-o",
            "BatchMode=yes", // never hang on a password prompt
            "-o",
            "ConnectTimeout=10",
            "-o",
            "StrictHostKeyChecking=accept-new",
        ])
        .arg(&host)
        .arg(&args.command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let (stdout, stderr, code, timed_out) =
            run_with_timeout(&mut cmd, "ssh", timeout, &args.command)?;
        Ok(SshRunResult {
            host,
            stdout,
            stderr,
            exit_code: code,
            timed_out,
        })
    }
}

/// Spawns `cmd`, drains both pipes on helper threads, polls for exit, kills on timeout.
fn run_with_timeout(
    cmd: &mut Command,
    what: &str,
    timeout: Duration,
    label: &str,
) -> Result<(String, String, i32, bool), String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{what} failed to spawn: {e}"))?;
    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");

    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if started.elapsed() >= timeout {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(e) => return Err(format!("{what} wait failed: {e}")),
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
    let code = match status {
        Some(st) => st.code().unwrap_or(-1),
        None => -1,
    };
    let _ = label;
    Ok((cap(stdout), cap(stderr), code, timed_out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_captures_output() {
        let exec = ExecTools::new(std::env::temp_dir());
        let r = exec
            .run_command(RunCommandArgs {
                command: "echo nanomite-tools".into(),
                cwd: None,
                timeout_ms: None,
            })
            .expect("run");
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("nanomite-tools"));
        assert!(!r.timed_out);
    }

    #[test]
    fn reports_nonzero_exit_and_stderr() {
        let exec = ExecTools::new(std::env::temp_dir());
        let r = exec
            .run_command(RunCommandArgs {
                command: "echo boom >&2; exit 3".into(),
                cwd: None,
                timeout_ms: None,
            })
            .expect("run");
        assert_eq!(r.exit_code, 3);
        assert!(r.stderr.contains("boom"));
    }

    #[test]
    fn refuses_cwd_escape_and_empty_command() {
        let exec = ExecTools::new(PathBuf::from("/tmp"));
        let err = exec
            .run_command(RunCommandArgs {
                command: "ls".into(),
                cwd: Some("../..".into()),
                timeout_ms: None,
            })
            .unwrap_err();
        assert!(err.contains("escapes the workspace"));
        assert!(exec
            .run_command(RunCommandArgs {
                command: "   ".into(),
                cwd: None,
                timeout_ms: None,
            })
            .is_err());
    }

    #[test]
    fn ssh_validates_arguments_without_network() {
        let exec = ExecTools::new(PathBuf::from("/tmp"));
        assert!(exec
            .ssh_run(SshRunArgs {
                host: String::new(),
                command: "ls".into(),
                timeout_ms: None,
            })
            .is_err());
        assert!(exec
            .ssh_run(SshRunArgs {
                host: "two tokens".into(),
                command: "ls".into(),
                timeout_ms: None,
            })
            .is_err());
    }
}
