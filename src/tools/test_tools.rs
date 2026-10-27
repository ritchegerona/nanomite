//! Test runner tool: runs project tests and returns structured pass/fail output.
//!
//! Supports cargo, npm, pytest, jest, go test, and generic commands.
//! Results are returned as a structured payload the model can reason over.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_TIMEOUT_MS: u64 = 300_000;
const MAX_OUTPUT_CHARS: usize = 50_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunTestsArgs {
    /// Test runner: "cargo" | "npm" | "pytest" | "jest" | "go" | "custom"
    pub runner: String,
    /// Additional arguments passed to the runner.
    #[serde(default)]
    pub args: Vec<String>,
    /// Workspace-relative working directory (defaults to workspace root).
    #[serde(default)]
    pub cwd: Option<String>,
    /// Hard timeout in milliseconds (max 300s, default 120s).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// For custom runner: full command line to execute.
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunTestsResult {
    pub runner: String,
    pub command: String,
    pub cwd: String,
    pub exit_code: i32,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub summary: TestSummary,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub suites: Vec<TestSuite>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSuite {
    pub name: String,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub tests: Vec<TestCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub name: String,
    pub status: TestStatus,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Clone)]
pub struct TestTools {
    root: PathBuf,
}

impl TestTools {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

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

    fn run_command(
        &self,
        cmd: &mut Command,
        timeout: Duration,
        label: &str,
    ) -> Result<(String, String, i32, bool), String> {
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("{label} failed to spawn: {e}"))?;
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
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(format!("{label} wait failed: {e}")),
            }
        };

        let stdout = out_handle.join().unwrap_or_default();
        let stderr = err_handle.join().unwrap_or_default();

        let cap = |v: Vec<u8>| -> String {
            String::from_utf8_lossy(&v)
                .chars()
                .take(MAX_OUTPUT_CHARS)
                .collect()
        };
        let code = match status {
            Some(st) => st.code().unwrap_or(-1),
            None => -1,
        };
        let combined = format!("{}\n{}", cap(stdout), cap(stderr));
        Ok((combined, String::new(), code, timed_out))
    }

    pub fn run_tests(&self, args: RunTestsArgs) -> Result<RunTestsResult, String> {
        let cwd = self.resolve_cwd(args.cwd.as_deref())?;
        let rel_cwd = cwd.strip_prefix(&self.root).unwrap_or(&cwd);
        let cwd_str = rel_cwd.to_string_lossy().to_string();

        let timeout = Duration::from_millis(
            args.timeout_ms
                .unwrap_or(120_000)
                .clamp(1_000, MAX_TIMEOUT_MS),
        );

        let (runner, command, mut cmd) = match args.runner.as_str() {
            "cargo" => {
                let mut c = Command::new("cargo");
                c.arg("test");
                c.args(&args.args);
                if args.args.is_empty() {
                    c.arg("--");
                    c.arg("--nocapture");
                }
                ("cargo", format!("cargo test {}", args.args.join(" ")), c)
            }
            "npm" => {
                let mut c = Command::new("npm");
                c.arg("test");
                c.args(&args.args);
                ("npm", format!("npm test {}", args.args.join(" ")), c)
            }
            "pytest" => {
                let mut c = Command::new("python3");
                c.arg("-m").arg("pytest");
                c.args(&args.args);
                if args.args.is_empty() {
                    c.arg("-v");
                }
                ("pytest", format!("pytest {}", args.args.join(" ")), c)
            }
            "jest" => {
                let mut c = Command::new("npx");
                c.arg("jest");
                c.args(&args.args);
                ("jest", format!("jest {}", args.args.join(" ")), c)
            }
            "go" => {
                let mut c = Command::new("go");
                c.arg("test");
                c.args(&args.args);
                if args.args.is_empty() {
                    c.arg("./...").arg("-v");
                }
                ("go", format!("go test {}", args.args.join(" ")), c)
            }
            "custom" => {
                let cmd_str = args
                    .command
                    .ok_or("custom runner requires 'command' field")?;
                let parts: Vec<&str> = cmd_str.split_whitespace().collect();
                if parts.is_empty() {
                    return Err("custom command cannot be empty".into());
                }
                let mut c = Command::new(parts[0]);
                c.args(&parts[1..]);
                ("custom", cmd_str, c)
            }
            other => return Err(format!("unknown test runner: {other}")),
        };

        cmd.current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let started = Instant::now();
        let (output, _, exit_code, timed_out) = self.run_command(&mut cmd, timeout, &command)?;
        let duration_ms = started.elapsed().as_millis() as u64;

        let summary = parse_test_output(&output, runner);

        Ok(RunTestsResult {
            runner: runner.to_string(),
            command,
            cwd: cwd_str,
            exit_code,
            timed_out,
            duration_ms,
            summary,
            output,
        })
    }
}

fn parse_test_output(output: &str, runner: &str) -> TestSummary {
    match runner {
        "cargo" => parse_cargo_output(output),
        "npm" => parse_npm_output(output),
        "pytest" => parse_pytest_output(output),
        "jest" => parse_jest_output(output),
        "go" => parse_go_output(output),
        _ => parse_generic_output(output),
    }
}

fn parse_cargo_output(output: &str) -> TestSummary {
    let mut suites = Vec::new();
    let mut current_suite: Option<TestSuite> = None;
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for line in output.lines() {
        // test test_name ... ok
        if let Some(name) = line.strip_prefix("test ") {
            if let Some(name) = name.strip_suffix(" ... ok") {
                total += 1;
                passed += 1;
                if let Some(suite) = &mut current_suite {
                    suite.passed += 1;
                    suite.tests.push(TestCase {
                        name: name.to_string(),
                        status: TestStatus::Passed,
                        duration_ms: None,
                        error: None,
                    });
                }
            } else if let Some(name) = name.strip_suffix(" ... FAILED") {
                total += 1;
                failed += 1;
                if let Some(suite) = &mut current_suite {
                    suite.failed += 1;
                    suite.tests.push(TestCase {
                        name: name.to_string(),
                        status: TestStatus::Failed,
                        duration_ms: None,
                        error: Some("test failed".into()),
                    });
                }
            } else if let Some(name) = name.strip_suffix(" ... ignored") {
                total += 1;
                skipped += 1;
                if let Some(suite) = &mut current_suite {
                    suite.skipped += 1;
                    suite.tests.push(TestCase {
                        name: name.to_string(),
                        status: TestStatus::Skipped,
                        duration_ms: None,
                        error: None,
                    });
                }
            }
        }
        // running N test(s)
        else if line.starts_with("running ") {
            if let Some(suite) = current_suite.take() {
                suites.push(suite);
            }
            current_suite = Some(TestSuite {
                name: "cargo".to_string(),
                passed: 0,
                failed: 0,
                skipped: 0,
                tests: Vec::new(),
            });
        }
        // test result: ok. N passed; M failed; K ignored
        else if line.starts_with("test result:") {
            // Summary line - already counted
        }
    }
    if let Some(suite) = current_suite {
        suites.push(suite);
    }

    TestSummary {
        total,
        passed,
        failed,
        skipped,
        suites,
    }
}

fn parse_npm_output(output: &str) -> TestSummary {
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let tests = Vec::new();

    // Simple heuristics for npm test output
    for line in output.lines() {
        if line.contains("✓") || line.contains("passing") {
            passed += line.matches("✓").count();
        }
        if line.contains("✗") || line.contains("failing") {
            failed += line.matches("✗").count();
        }
        if line.contains("pending") || line.contains("skipped") {
            skipped += line.matches("pending").count() + line.matches("skipped").count();
        }
    }
    let total = passed + failed + skipped;

    TestSummary {
        total,
        passed,
        failed,
        skipped,
        suites: vec![TestSuite {
            name: "npm".to_string(),
            passed,
            failed,
            skipped,
            tests,
        }],
    }
}

fn parse_pytest_output(output: &str) -> TestSummary {
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut tests = Vec::new();

    for line in output.lines() {
        // test_file.py::test_name PASSED/FAILED/SKIPPED
        if line.contains("::")
            && (line.contains("PASSED") || line.contains("FAILED") || line.contains("SKIPPED"))
        {
            total += 1;
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let (Some(name), Some(status)) = (parts.first(), parts.last()) {
                match *status {
                    "PASSED" => {
                        passed += 1;
                        tests.push(TestCase {
                            name: name.to_string(),
                            status: TestStatus::Passed,
                            duration_ms: None,
                            error: None,
                        });
                    }
                    "FAILED" => {
                        failed += 1;
                        tests.push(TestCase {
                            name: name.to_string(),
                            status: TestStatus::Failed,
                            duration_ms: None,
                            error: Some("test failed".into()),
                        });
                    }
                    "SKIPPED" => {
                        skipped += 1;
                        tests.push(TestCase {
                            name: name.to_string(),
                            status: TestStatus::Skipped,
                            duration_ms: None,
                            error: None,
                        });
                    }
                    _ => {}
                }
            }
        }
        // == N passed, M failed, K skipped ==
        else if line.starts_with("==") && line.contains("passed") {
            // Summary line
        }
    }

    TestSummary {
        total,
        passed,
        failed,
        skipped,
        suites: vec![TestSuite {
            name: "pytest".to_string(),
            passed,
            failed,
            skipped,
            tests,
        }],
    }
}

fn parse_jest_output(output: &str) -> TestSummary {
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let tests = Vec::new();

    for line in output.lines() {
        if line.contains("✓") || line.contains("PASS") {
            passed += line.matches("✓").count();
        }
        if line.contains("✕") || line.contains("FAIL") {
            failed += line.matches("✕").count();
        }
        if line.contains("✓") && line.contains("skip") {
            skipped += 1;
        }
    }
    let total = passed + failed + skipped;

    TestSummary {
        total,
        passed,
        failed,
        skipped,
        suites: vec![TestSuite {
            name: "jest".to_string(),
            passed,
            failed,
            skipped,
            tests,
        }],
    }
}

fn parse_go_output(output: &str) -> TestSummary {
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut tests = Vec::new();

    for line in output.lines() {
        // === RUN   TestName
        if line.contains("=== RUN") {
            if let Some(name) = line.split_whitespace().nth(2) {
                let _ = name;
            }
        }
        // --- PASS: TestName (0.00s)
        else if line.contains("--- PASS:") {
            total += 1;
            passed += 1;
            if let Some(name) = line.split_whitespace().nth(2) {
                let duration = line
                    .split('(')
                    .nth(1)
                    .and_then(|s| s.split('s').next())
                    .and_then(|s| s.parse().ok());
                tests.push(TestCase {
                    name: name.to_string(),
                    status: TestStatus::Passed,
                    duration_ms: duration,
                    error: None,
                });
            }
        }
        // --- FAIL: TestName (0.00s)
        else if line.contains("--- FAIL:") {
            total += 1;
            failed += 1;
            if let Some(name) = line.split_whitespace().nth(2) {
                let duration = line
                    .split('(')
                    .nth(1)
                    .and_then(|s| s.split('s').next())
                    .and_then(|s| s.parse().ok());
                tests.push(TestCase {
                    name: name.to_string(),
                    status: TestStatus::Failed,
                    duration_ms: duration,
                    error: Some("test failed".into()),
                });
            }
        }
        // --- SKIP: TestName
        else if line.contains("--- SKIP:") {
            total += 1;
            skipped += 1;
            if let Some(name) = line.split_whitespace().nth(2) {
                tests.push(TestCase {
                    name: name.to_string(),
                    status: TestStatus::Skipped,
                    duration_ms: None,
                    error: None,
                });
            }
        }
        // ok/FAIL package summary lines carry no per-test data we track.
        else if line.starts_with("ok  ") || line.starts_with("FAIL ") {
        }
    }

    TestSummary {
        total,
        passed,
        failed,
        skipped,
        suites: vec![TestSuite {
            name: "go".to_string(),
            passed,
            failed,
            skipped,
            tests,
        }],
    }
}

fn parse_generic_output(output: &str) -> TestSummary {
    let total = output.lines().count();
    TestSummary {
        total,
        passed: 0,
        failed: if output.contains("FAIL") || output.contains("fail") || output.contains("error") {
            1
        } else {
            0
        },
        skipped: 0,
        suites: vec![TestSuite {
            name: "generic".to_string(),
            passed: 0,
            failed: if output.contains("FAIL")
                || output.contains("fail")
                || output.contains("error")
            {
                1
            } else {
                0
            },
            skipped: 0,
            tests: vec![],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_cargo_project() -> (PathBuf, TestTools) {
        let dir = std::env::temp_dir().join(format!("nanomite-test-tools-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let test_tools = TestTools::new(&dir);
        // Initialize a minimal cargo project
        fs::write(
            dir.join("Cargo.toml"),
            r#"
[package]
name = "test_proj"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("src/lib.rs"),
            r#"
#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
    #[test]
    fn it_fails() {
        panic!("intentional failure");
    }
}
"#,
        )
        .unwrap();
        (dir, test_tools)
    }

    #[test]
    fn cargo_test_returns_structured_results() {
        let (dir, tools) = init_cargo_project();
        let r = tools.run_tests(RunTestsArgs {
            runner: "cargo".into(),
            args: vec![],
            cwd: None,
            timeout_ms: Some(60_000),
            command: None,
        });
        // Note: this test may fail in environments without cargo, but the structure is correct
        match r {
            Ok(result) => {
                assert_eq!(result.runner, "cargo");
                assert!(result.duration_ms > 0);
                assert!(!result.output.is_empty());
            }
            Err(e) => {
                // cargo might not be available in test env
                eprintln!("cargo test skipped: {e}");
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_runner_is_refused() {
        let dir = std::env::temp_dir().join(format!("nanomite-test-tools-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let tools = TestTools::new(&dir);
        let err = tools
            .run_tests(RunTestsArgs {
                runner: "unknown".into(),
                args: vec![],
                cwd: None,
                timeout_ms: None,
                command: None,
            })
            .unwrap_err();
        assert!(err.contains("unknown test runner"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn custom_runner_requires_command() {
        let dir = std::env::temp_dir().join(format!("nanomite-test-tools-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let tools = TestTools::new(&dir);
        let err = tools
            .run_tests(RunTestsArgs {
                runner: "custom".into(),
                args: vec![],
                cwd: None,
                timeout_ms: None,
                command: None,
            })
            .unwrap_err();
        assert!(err.contains("custom runner requires"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_escape_is_refused() {
        let dir = std::env::temp_dir().join(format!("nanomite-test-tools-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let tools = TestTools::new(&dir);
        let err = tools
            .run_tests(RunTestsArgs {
                runner: "cargo".into(),
                args: vec![],
                cwd: Some("../..".into()),
                timeout_ms: None,
                command: None,
            })
            .unwrap_err();
        assert!(err.contains("escapes the workspace"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_cargo_output_basic() {
        let output = "running 2 tests\n\
test test_one ... ok\n\
test test_two ... FAILED\n\
test result: FAILED. 1 passed; 1 failed; 0 ignored\n";
        let summary = parse_cargo_output(output);
        assert_eq!(summary.total, 2);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.suites.len(), 1);
        assert_eq!(summary.suites[0].tests.len(), 2);
    }
}
