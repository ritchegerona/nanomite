//! Regression gate: what must pass before a Source self-edit is kept.
//!
//! [`CargoTestRunner`] shells out to `cargo test --lib` inside the caller's
//! repo root with a hard timeout; [`FakeRunner`] lets tests (and the UI, for
//! dry runs) inject a predetermined outcome. Unit tests never execute cargo.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Hard limit for one regression run: 600 seconds.
pub const REGRESSION_TIMEOUT_SECS: u64 = 600;

/// Combined output kept in `RegressionOutcome::detail`: last 4 KiB.
const OUTPUT_TAIL_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegressionOutcome {
    pub passed: bool,
    pub detail: String,
}

/// Who decides whether a change survives. Implemented by the real cargo
/// runner in production and by fakes in tests.
pub trait RegressionRunner {
    fn run_regression(&self, repo_root: &Path) -> RegressionOutcome;
}

/// Runs `cargo test --lib` in `repo_root`; pass = exit success within the
/// timeout. `detail` carries the exit status (or timeout marker) plus the
/// last 4 KiB of combined output.
#[derive(Debug, Default, Clone, Copy)]
pub struct CargoTestRunner;

impl RegressionRunner for CargoTestRunner {
    fn run_regression(&self, repo_root: &Path) -> RegressionOutcome {
        let mut cmd = Command::new("cargo");
        cmd.args(["test", "--lib"]);
        cmd.current_dir(repo_root);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                return RegressionOutcome {
                    passed: false,
                    detail: format!(
                        "failed to spawn `cargo test --lib` in {}: {e}",
                        repo_root.display()
                    ),
                };
            }
        };
        // Drain the pipes on threads so a chatty child cannot deadlock on full
        // pipe buffers while this thread is only polling for exit.
        let stdout = child.stdout.take().expect("cargo stdout was piped");
        let stderr = child.stderr.take().expect("cargo stderr was piped");
        let out_thread = thread::spawn(move || drain(stdout));
        let err_thread = thread::spawn(move || drain(stderr));

        let deadline = Instant::now() + Duration::from_secs(REGRESSION_TIMEOUT_SECS);
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        timed_out = true;
                        let _ = child.kill();
                        break child.wait().expect("cargo child vanished before reaping");
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    return RegressionOutcome {
                        passed: false,
                        detail: format!("waiting for `cargo test --lib` failed: {e}"),
                    };
                }
            }
        };
        let mut combined = out_thread.join().unwrap_or_default();
        combined.extend(err_thread.join().unwrap_or_default());
        let tail = output_tail(&combined, OUTPUT_TAIL_BYTES);
        let detail = if timed_out {
            format!("TIMEOUT after {REGRESSION_TIMEOUT_SECS}s running `cargo test --lib`\n{tail}")
        } else {
            tail
        };
        RegressionOutcome {
            passed: status.success() && !timed_out,
            detail,
        }
    }
}

/// Test/dry-run double returning a fixed outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeRunner {
    pub passed: bool,
    pub detail: String,
}

impl RegressionRunner for FakeRunner {
    fn run_regression(&self, _repo_root: &Path) -> RegressionOutcome {
        RegressionOutcome {
            passed: self.passed,
            detail: self.detail.clone(),
        }
    }
}

fn drain<R: Read>(mut pipe: R) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = pipe.read_to_end(&mut buf);
    buf
}

/// Last `max` bytes of `bytes` as lossy UTF-8, aligned to a char boundary
/// so unicode output never begins with a torn character.
fn output_tail(bytes: &[u8], max: usize) -> String {
    let mut start = bytes.len().saturating_sub(max);
    // A byte starts a char unless it is a UTF-8 continuation byte (10xxxxxx).
    while start < bytes.len() && (bytes[start] & 0xC0) == 0x80 {
        start += 1;
    }
    String::from_utf8_lossy(&bytes[start..])
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{output_tail, FakeRunner, RegressionRunner};
    use std::path::Path;

    #[test]
    fn fake_runner_reports_the_configured_outcome() {
        let runner = FakeRunner {
            passed: false,
            detail: "test result: FAILED. 3 failed".to_string(),
        };
        let outcome = runner.run_regression(Path::new("/nowhere"));
        assert!(!outcome.passed);
        assert_eq!(outcome.detail, "test result: FAILED. 3 failed");
    }

    #[test]
    fn output_tail_is_capped_and_utf8_safe() {
        let bytes = "ααααα".repeat(1000).into_bytes(); // 10 KB of two-byte chars
        let tail = output_tail(&bytes, 4096);
        assert!(tail.len() <= 4096);
        assert!(
            tail.chars().all(|c| c == 'α'),
            "no torn characters: {tail:?}"
        );
        assert!(!tail.contains('\u{FFFD}'));
        assert_eq!(output_tail(b"", 4096), "");
    }
}
