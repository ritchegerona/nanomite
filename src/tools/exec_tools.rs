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
pub(crate) const MAX_OUTPUT: usize = 16 * 1024;
/// Wall-clock ceiling for one command.
pub(crate) const MAX_TIMEOUT_MS: u64 = 120_000;

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
    /// Optional sudo password for commands requiring elevation.
    /// If provided, the command is run via `sudo -S` with the password piped to stdin.
    /// Never logged or stored; only used for the single command execution.
    #[serde(default)]
    pub sudo_password: Option<String>,
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

#[derive(Clone)]
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

        // Elevation guard: without a password, a `sudo`/`su` command would open
        // /dev/tty and hang forever waiting for input it can never get (the TUI
        // owns the terminal). Probe non-interactively instead: `sudo -n` fails
        // immediately when a password is needed, so the UI can pop the password
        // modal instead of the command stalling the whole turn.
        if args.sudo_password.is_none() {
            match mentions_elevation(&args.command) {
                Some("sudo") => {
                    let mut probe = Command::new("sudo");
                    probe
                        .arg("-n")
                        .arg("-v")
                        .current_dir(&dir)
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped());
                    let (_, _, code, timed_out) = run_with_timeout(
                        &mut probe,
                        "sudo -n",
                        Duration::from_millis(5_000),
                        "sudo -n -v",
                    )?;
                    if timed_out || code != 0 {
                        return Err(
                            "sudo: password required - the sudo password prompt is open, enter it to run this command"
                                .into(),
                        );
                    }
                }
                Some("su") => {
                    // `su` cannot read a password from stdin; only proceed as root.
                    let who = Command::new("id")
                        .arg("-u")
                        .stdin(Stdio::null())
                        .output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .unwrap_or_default();
                    if who != "0" {
                        return Err(
                            "su: password required - use sudo instead of su, or enter the sudo password in the prompt"
                                .into(),
                        );
                    }
                }
                _ => {}
            }
        }

        // With a password, normalize the command so exactly ONE sudo reads it
        // from stdin:
        //   `sudo CMD...`        -> `sudo -S sh -c "CMD..."`
        //   `sudo -v` / flags    -> `sudo -S -v` (validates + caches creds)
        //   `su [-] [user] -c C` -> `sudo -S sh -c "C"` (su cannot read a
        //                           piped password; sudo can)
        let inner_command;
        let (program, args_vec, stdin_data) = if let Some(password) = args.sudo_password {
            let stripped = strip_leading_sudo(&args.command);
            if stripped.starts_with('-') {
                // Bare sudo flags (sudo -v, sudo --validate): pass through so
                // sudo itself validates the password and caches the ticket.
                let mut argv = vec!["-S"];
                argv.extend(stripped.split_whitespace());
                ("sudo", argv, Some(password))
            } else {
                inner_command = translate_su(stripped)?;
                (
                    "sudo",
                    vec!["-S", "sh", "-c", inner_command.as_str()],
                    Some(password),
                )
            }
        } else {
            ("sh", vec!["-c", &args.command], None)
        };

        let mut cmd = Command::new(program);
        cmd.args(&args_vec).current_dir(&dir);

        if let Some(stdin_data) = stdin_data {
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let mut child = cmd
                .spawn()
                .map_err(|e| format!("sudo failed to spawn: {e}"))?;

            // Write password to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(
                    format!(
                        "{}
",
                        stdin_data
                    )
                    .as_bytes(),
                );
                let _ = stdin.flush();
            }

            let (stdout, stderr, code, timed_out) =
                run_with_timeout_child(child, timeout, &args.command)?;

            Ok(RunCommandResult {
                command: args.command,
                cwd: dir.to_string_lossy().into_owned(),
                stdout,
                stderr,
                exit_code: code,
                timed_out,
            })
        } else {
            cmd.stdin(Stdio::null())
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

/// Detects whether a command line invokes `sudo` or `su` (word-boundary match,
/// path prefixes like `/usr/bin/sudo` included). Returns which tool it found so
/// the caller can pick the right guard.
fn mentions_elevation(command: &str) -> Option<&'static str> {
    for word in command.split_whitespace() {
        let base = word
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '-' && c != '.')
            .rsplit('/')
            .next()
            .unwrap_or(word);
        match base {
            "sudo" => return Some("sudo"),
            "su" => return Some("su"),
            _ => {}
        }
    }
    None
}

/// Rewrites `su [-] [user] -c CMD` into `CMD` so it can run under the single
/// `sudo -S sh -c` wrapper. `su` reads its password from /dev/tty only - it
/// can never accept the piped password - and as a non-root user it asks for
/// the ROOT password, which is not what the modal collects. sudo is the right
/// tool for piped elevation, so su-to-root is translated; su to another user
/// or an interactive su is refused with a clear message.
fn translate_su(command: &str) -> Result<String, String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    if tokens.first().copied() != Some("su") {
        return Ok(command.to_string());
    }
    // Walk: su [-|-l] [username] -c CMD...
    let mut idx = 1usize;
    let mut target_root = true;
    while idx < tokens.len() {
        match tokens[idx] {
            "-" | "-l" | "--login" => idx += 1,
            "-c" | "--command" => {
                // The rest is the command (one token, since the model passes
                // it quoted through the shell - sh -c receives it as one arg
                // only if it was quoted; join whatever remains).
                let cmd = tokens[idx + 1..].join(" ");
                let unquoted = cmd
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .or_else(|| cmd.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                    .unwrap_or(&cmd);
                if !target_root {
                    return Err(
                        "su: password required - nanomite can only elevate to root via sudo;                          run the command directly with sudo instead of su <user>"
                            .into(),
                    );
                }
                return Ok(unquoted.to_string());
            }
            "root" => {
                target_root = true;
                idx += 1;
            }
            other if other.starts_with('-') => idx += 1,
            // A bare username: switching to a non-root user needs that user's
            // password, which the modal does not collect.
            _ => {
                return Err(
                    "su: password required - nanomite can only elevate to root via sudo;                      replace `su <user>` with sudo or run the command directly"
                        .into(),
                )
            }
        }
    }
    // `su` with no -c: interactive shell - pointless for an agent tool.
    Err(
        "su: password required - interactive su cannot run here;          prefix the command with sudo instead"
            .into(),
    )
}

/// Removes a leading `sudo ` (and any leading env assignments immediately
/// before it is left untouched) so the command can be re-wrapped under a
/// single `sudo -S sh -c`. A command that does not start with sudo is
/// returned unchanged - the outer `sudo -S` elevates the whole line.
fn strip_leading_sudo(command: &str) -> &str {
    let trimmed = command.trim_start();
    let stripped = trimmed.strip_prefix("sudo ").unwrap_or(trimmed);
    // `sudo -v` / `sudo --validate` only refreshes credentials; running it
    // under -S with the password validates and caches them, which is exactly
    // what the caller wants.
    stripped
}

/// Kills a child's whole process group: a killed parent otherwise leaves
/// grandchildren (e.g. `sleep 30` from a hook script) running and holding the
/// pipe write-ends, which keeps the output readers blocked until they exit.
pub(crate) fn kill_process_group(child: &std::process::Child) {
    unsafe {
        // SIGKILL to the negative pid = every process in the group.
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
}

/// Spawns `cmd` in its own process group, drains both pipes on helper threads,
/// polls for exit, kills the whole group on timeout. Shared with the git tools,
/// which run `git` directly instead of through a shell.
pub(crate) fn run_with_timeout(
    cmd: &mut Command,
    what: &str,
    timeout: Duration,
    label: &str,
) -> Result<(String, String, i32, bool), String> {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
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
                    kill_process_group(&child);
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

/// Same as `run_with_timeout` but takes an already-spawned child process.
/// Used when stdin has already been written (e.g., sudo password).
pub(crate) fn run_with_timeout_child(
    mut child: std::process::Child,
    timeout: Duration,
    label: &str,
) -> Result<(String, String, i32, bool), String> {
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
                    kill_process_group(&child);
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(e) => return Err(format!("{label} wait failed: {e}")),
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
                sudo_password: None,
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
                sudo_password: None,
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
                sudo_password: None,
            })
            .unwrap_err();
        assert!(err.contains("escapes the workspace"));
        assert!(exec
            .run_command(RunCommandArgs {
                command: "   ".into(),
                cwd: None,
                timeout_ms: None,
                sudo_password: None,
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

    #[test]
    fn elevation_detection_matches_sudo_and_su_by_word() {
        assert_eq!(
            mentions_elevation("sudo apt-get install -y nmap"),
            Some("sudo")
        );
        assert_eq!(mentions_elevation("/usr/bin/sudo -v"), Some("sudo"));
        assert_eq!(mentions_elevation("(sudo) true"), Some("sudo"));
        assert_eq!(mentions_elevation("echo ok && sudo reboot"), Some("sudo"));
        assert_eq!(mentions_elevation("su -c 'dnf install nmap'"), Some("su"));
        // No false positives on words that merely contain the letters.
        assert_eq!(mentions_elevation("sum 1 2"), None);
        assert_eq!(mentions_elevation("assume role"), None);
        assert_eq!(mentions_elevation("cargo test --quiet"), None);
        assert_eq!(mentions_elevation("ls -la"), None);
    }

    #[test]
    fn strip_leading_sudo_removes_only_the_first_token() {
        assert_eq!(
            strip_leading_sudo("sudo apt-get install -y nmap"),
            "apt-get install -y nmap"
        );
        assert_eq!(strip_leading_sudo("  sudo -v"), "-v");
        assert_eq!(
            strip_leading_sudo("echo hi && sudo reboot"),
            "echo hi && sudo reboot"
        );
        assert_eq!(strip_leading_sudo("ls"), "ls");
    }

    #[test]
    fn sudo_without_password_fails_fast_instead_of_hanging() {
        // This machine has a sudo password (no NOPASSWD), so the probe must
        // refuse immediately. If this environment ever grants NOPASSWD sudo,
        // the command succeeds and the assertion flips - the guard is about
        // never hanging, not about the specific exit path.
        let exec = ExecTools::new(std::env::temp_dir());
        let started = std::time::Instant::now();
        let result = exec.run_command(RunCommandArgs {
            command: "sudo -v".into(),
            cwd: None,
            timeout_ms: None,
            sudo_password: None,
        });
        let elapsed = started.elapsed();
        match result {
            Err(e) => {
                assert!(e.contains("password required"), "unexpected error: {e}");
            }
            Ok(r) => {
                // NOPASSWD environment: the command ran without prompting.
                assert_eq!(r.exit_code, 0, "unexpected output: {}", r.stderr);
            }
        }
        // Either way it must return in well under the 120s tool timeout.
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "sudo guard took too long: {elapsed:?}"
        );
    }

    #[test]
    fn su_without_password_and_without_root_fails_fast() {
        if users_root() {
            return; // root su never prompts; nothing to guard
        }
        let exec = ExecTools::new(std::env::temp_dir());
        let started = std::time::Instant::now();
        let result = exec.run_command(RunCommandArgs {
            command: "su -c 'echo should-not-run'".into(),
            cwd: None,
            timeout_ms: None,
            sudo_password: None,
        });
        assert!(result.is_err(), "su without password must be refused");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "su guard took too long: {:?}",
            started.elapsed()
        );
    }

    /// Best-effort root check without a libc dependency.
    fn users_root() -> bool {
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
            .unwrap_or(false)
    }

    #[test]
    fn translate_su_rewrites_root_targets_and_refuses_others() {
        assert_eq!(
            translate_su("dnf install -y nmap").unwrap(),
            "dnf install -y nmap"
        );
        assert_eq!(
            translate_su("su -c 'dnf install -y nmap'").unwrap(),
            "dnf install -y nmap"
        );
        assert_eq!(
            translate_su("su -c \"dnf install -y nmap\"").unwrap(),
            "dnf install -y nmap"
        );
        assert_eq!(translate_su("su root -c whoami").unwrap(), "whoami");
        assert_eq!(translate_su("su - root -c whoami").unwrap(), "whoami");
        // Non-root target and interactive su are refused with guidance.
        assert!(translate_su("su alice -c id").unwrap_err().contains("sudo"));
        assert!(translate_su("su").unwrap_err().contains("sudo"));
    }

    /// Full password-pipe mechanics through the real `run_command`, using a
    /// fake `sudo` on PATH that authenticates a known password from stdin and
    /// then executes the requested command - proving the modal's password
    /// reaches the child via the pipe (never /dev/tty) and the output comes
    /// back captured.
    #[test]
    fn sudo_pipe_delivers_password_and_runs_command() {
        let dir = std::env::temp_dir().join(format!("nanomite-sudo-fake-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Fake sudo: reads one line (the password), compares, and on success
        // execs the remaining args. Mirrors `sudo -S` stdin behavior.
        let fake = dir.join("sudo");
        std::fs::write(
            &fake,
            "#!/bin/sh\nread -r line\n[ \"$line\" = \"test-password\" ] || { echo \"sorry try again\" >&2; exit 1; }\nshift 1 # drop -S; argv is now sh -c CMD\nexec \"$@\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        // A marker script the command will run, proving the elevating shell
        // executed with stdout captured.
        let marker = dir.join("marker.sh");
        std::fs::write(&marker, "#!/bin/sh\necho elevated-ran\n").unwrap();
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Put the fake sudo first on PATH for this process (tests run single
        // threaded for fs/env isolation anyway via the suite's threaded runs -
        // restore immediately after).
        let original_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{}:{}", dir.display(), original_path.clone()),
        );
        let result = std::panic::catch_unwind(|| {
            let exec = ExecTools::new(std::env::temp_dir());
            exec.run_command(RunCommandArgs {
                command: "sudo sh -c /place/marker.sh".replace("/place", &dir.to_string_lossy()),
                cwd: None,
                timeout_ms: Some(10_000),
                sudo_password: Some("test-password".into()),
            })
        });
        std::env::set_var("PATH", original_path.clone());
        let _ = std::fs::remove_dir_all(&dir);

        let result = result.expect("no panic");
        let r = result.expect("fake sudo run");
        assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
        assert!(r.stdout.contains("elevated-ran"), "stdout: {}", r.stdout);

        // Wrong password must surface sudo's refusal, not a hang.
        std::env::set_var(
            "PATH",
            format!("{}:{}", dir.display(), original_path.clone()),
        );
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&fake, "#!/bin/sh\nread -r line\n[ \"$line\" = \"test-password\" ] || { echo \"sorry try again\" >&2; exit 1; }\nexit 1\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let wrong = std::panic::catch_unwind(|| {
            let exec = ExecTools::new(std::env::temp_dir());
            exec.run_command(RunCommandArgs {
                command: "sudo -v".into(),
                cwd: None,
                timeout_ms: Some(10_000),
                sudo_password: Some("wrong-password".into()),
            })
        });
        std::env::set_var("PATH", original_path.clone());
        let _ = std::fs::remove_dir_all(&dir);
        let wrong = wrong.expect("no panic");
        let r = wrong.expect("fake sudo run");
        assert_ne!(r.exit_code, 0, "wrong password must not succeed");
        assert!(r.stderr.contains("sorry"), "stderr: {}", r.stderr);
    }
}
