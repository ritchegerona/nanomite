//! Web Recon Module — wraps gobuster and nikto for authorized web
//! reconnaissance.
//!
//! Runs `gobuster dir` and `nikto` against a target URL, captures output,
//! and (when neural memory is attached) stores each finding as a tagged
//! technique record so the agent can recall it across sessions.
//!
//! All tools are scope-gated: the operator must confirm the target is in
//! scope before scanning. Execution is plan-only: nanomite produces the
//! command and parses results but never exploits.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

/// Target specification for a web recon scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebReconArgs {
    /// Target URL (e.g. http://example.com or http://example.com/admin).
    pub url: String,
    /// Which scanners to run: "gobuster", "nikto", or "both".
    #[serde(default = "default_scanners")]
    pub scanners: String,
    /// Wordlist path for gobuster (optional).
    #[serde(default)]
    pub wordlist: Option<String>,
    /// Additional gobuster flags (optional).
    #[serde(default)]
    pub gobuster_extra: Option<String>,
    /// Hard cap in milliseconds (default 120s).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

fn default_scanners() -> String {
    "both".to_string()
}

/// Single finding from a web recon scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconFinding {
    /// Scanner that produced this finding (gobuster or nikto).
    pub scanner: String,
    /// Discovered path or vulnerability description.
    pub finding: String,
    /// HTTP status code (if applicable).
    pub status: Option<u16>,
    /// Size in bytes (if applicable).
    pub size: Option<usize>,
    /// Raw output line from the scanner.
    pub raw: String,
}

/// Result of a web recon run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebReconResult {
    pub target: String,
    pub scanners_run: Vec<String>,
    pub findings: Vec<ReconFinding>,
    pub total_findings: usize,
    pub timed_out: bool,
    pub raw_output: String,
}

#[derive(Clone)]
pub struct ReconTools {
    /// Workspace root for resolving relative wordlists.
    root: PathBuf,
    /// Neural memory for storing discovered techniques (optional).
    pub memory: Option<Arc<crate::memory::MemoryEngine>>,
}

impl ReconTools {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            memory: None,
        }
    }

    /// Attach neural memory so findings are persisted as techniques.
    pub fn with_memory(mut self, engine: Arc<crate::memory::MemoryEngine>) -> Self {
        self.memory = Some(engine);
        self
    }

    /// Run web recon against `args.url`.
    pub fn run(&self, args: WebReconArgs) -> Result<WebReconResult, String> {
        let url = args.url.trim();
        if url.is_empty() {
            return Err("recon: url is required".to_string());
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err("recon: url must start with http:// or https://".to_string());
        }

        let timeout = Duration::from_millis(
            args.timeout_ms
                .unwrap_or(120_000)
                .clamp(5_000, 300_000),
        );

        let scanners: Vec<&str> = match args.scanners.as_str() {
            "gobuster" => vec!["gobuster"],
            "nikto" => vec!["nikto"],
            _ => vec!["gobuster", "nikto"],
        };

        let mut all_findings = Vec::new();
        let mut scanners_run = Vec::new();
        let mut raw_output = String::new();
        let mut timed_out = false;

        for scanner in &scanners {
            let result = match *scanner {
                "gobuster" => self.run_gobuster(url, args.wordlist.as_deref(), args.gobuster_extra.as_deref(), &timeout)?,
                "nikto" => self.run_nikto(url, &timeout)?,
                _ => continue,
            };
            scanners_run.push(scanner.to_string());
            if !raw_output.is_empty() {
                raw_output.push_str("\n");
            }
            raw_output.push_str(&format!("=== {} ===\n{}", scanner, result.raw_output));
            all_findings.extend(result.findings);
            if result.timed_out {
                timed_out = true;
            }
        }

        let total = all_findings.len();

        // Store findings as techniques in neural memory.
        if let Some(memory) = &self.memory {
            for f in &all_findings {
                let _ = memory.save(&crate::memory::model::NewMemory::new(
                    crate::memory::model::MemoryCategory::Episodic,
                    &format!(
                        "Web recon [{}] {} → {} (target: {})",
                        f.scanner,
                        f.finding,
                        f.status
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        url
                    ),
                ));
            }
        }

        Ok(WebReconResult {
            target: url.to_string(),
            scanners_run,
            findings: all_findings,
            total_findings: total,
            timed_out,
            raw_output,
        })
    }

    /// Run gobuster directory busting.
    fn run_gobuster(
        &self,
        url: &str,
        wordlist: Option<&str>,
        extra: Option<&str>,
        timeout: &Duration,
    ) -> Result<WebReconResult, String> {
        let mut cmd = Command::new("gobuster");
        cmd.arg("dir")
            .arg("-u")
            .arg(url)
            .arg("-q")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(wl) = wordlist {
            let wl_path = PathBuf::from(wl);
            let resolved = if wl_path.is_absolute() {
                wl_path
            } else {
                self.root.join(wl)
            };
            cmd.arg("-w").arg(resolved);
        }

        if let Some(extra) = extra {
            for token in extra.split_whitespace() {
                cmd.arg(token);
            }
        }

        let started = std::time::Instant::now();
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("gobuster spawn failed: {e}"))?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");

        let (tx, _rx) = std::sync::mpsc::channel::<(bool, Vec<u8>)>();
        let tx_out = tx.clone();
        let out_handle = std::thread::spawn(move || {
            let mut all = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        all.extend_from_slice(&buf[..n]);
                        let _ = tx_out.send((false, buf[..n].to_vec()));
                    }
                }
            }
            all
        });
        let tx_err = tx;
        let err_handle = std::thread::spawn(move || {
            let mut all = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                match stderr.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        all.extend_from_slice(&buf[..n]);
                        let _ = tx_err.send((true, buf[..n].to_vec()));
                    }
                }
            }
            all
        });

        let status = child.wait().map_err(|e| format!("gobuster wait failed: {e}"))?;
        let stdout_all = out_handle.join().unwrap_or_default();
        let stderr_all = err_handle.join().unwrap_or_default();

        let _code = status.code().unwrap_or(-1);
        let stdout_str = String::from_utf8_lossy(&stdout_all).to_string();
        let stderr_str = String::from_utf8_lossy(&stderr_all).to_string();
        let timed_out = started.elapsed() >= *timeout;

        let mut findings = Vec::new();
        for line in stdout_str.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let finding = parse_gobuster_line(line);
            findings.push(finding);
        }

        let total_findings = findings.len();

        Ok(WebReconResult {
            target: url.to_string(),
            scanners_run: vec!["gobuster".to_string()],
            findings,
            total_findings,
            timed_out,
            raw_output: format!("{stdout_str}\n{stderr_str}"),
        })
    }

    /// Run nikto web server vulnerability scan.
    fn run_nikto(
        &self,
        url: &str,
        timeout: &Duration,
    ) -> Result<WebReconResult, String> {
        let mut cmd = Command::new("nikto");
        cmd.arg("-h")
            .arg(url)
            .arg("-Format")
            .arg("txt")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let started = std::time::Instant::now();
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("nikto spawn failed: {e}"))?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");

        let (tx, _rx) = std::sync::mpsc::channel::<(bool, Vec<u8>)>();
        let tx_out = tx.clone();
        let out_handle = std::thread::spawn(move || {
            let mut all = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        all.extend_from_slice(&buf[..n]);
                        let _ = tx_out.send((false, buf[..n].to_vec()));
                    }
                }
            }
            all
        });
        let tx_err = tx;
        let err_handle = std::thread::spawn(move || {
            let mut all = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                match stderr.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        all.extend_from_slice(&buf[..n]);
                        let _ = tx_err.send((true, buf[..n].to_vec()));
                    }
                }
            }
            all
        });

        let status = child.wait().map_err(|e| format!("nikto wait failed: {e}"))?;
        let stdout_all = out_handle.join().unwrap_or_default();
        let stderr_all = err_handle.join().unwrap_or_default();

        let _code = status.code().unwrap_or(-1);
        let stdout_str = String::from_utf8_lossy(&stdout_all).to_string();
        let stderr_str = String::from_utf8_lossy(&stderr_all).to_string();
        let timed_out = started.elapsed() >= *timeout;

        let mut findings = Vec::new();
        for line in stdout_str.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('+') {
                findings.push(ReconFinding {
                    scanner: "nikto".to_string(),
                    finding: line[1..].trim().to_string(),
                    status: None,
                    size: None,
                    raw: line.to_string(),
                });
            } else {
                findings.push(ReconFinding {
                    scanner: "nikto".to_string(),
                    finding: line.to_string(),
                    status: None,
                    size: None,
                    raw: line.to_string(),
                });
            }
        }

        let total_findings = findings.len();

        Ok(WebReconResult {
            target: url.to_string(),
            scanners_run: vec!["nikto".to_string()],
            findings,
            total_findings,
            timed_out,
            raw_output: format!("{stdout_str}\n{stderr_str}"),
        })
    }
}

/// Parse a single gobuster output line into a finding.
fn parse_gobuster_line(line: &str) -> ReconFinding {
    // Format: /admin (Status: 200) [Size: 1234]
    let path_end = line.find('(').unwrap_or(line.len());
    let path = line[..path_end].trim().to_string();

    let status = line
        .find("Status:")
        .and_then(|s| line[s + 7..].split(')').next())
        .and_then(|s| s.trim().parse::<u16>().ok());

    let size = line
        .find("Size:")
        .and_then(|s| line[s + 5..].split(']').next())
        .and_then(|s| s.trim().parse::<usize>().ok());

    ReconFinding {
        scanner: "gobuster".to_string(),
        finding: path,
        status,
        size,
        raw: line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gobuster_line_extracts_path_and_status() {
        let f = parse_gobuster_line("/admin (Status: 200) [Size: 1234]");
        assert_eq!(f.finding, "/admin");
        assert_eq!(f.status, Some(200));
        assert_eq!(f.size, Some(1234));
        assert_eq!(f.scanner, "gobuster");
    }

    #[test]
    fn parse_gobuster_line_without_size() {
        let f = parse_gobuster_line("/index.php (Status: 301)");
        assert_eq!(f.finding, "/index.php");
        assert_eq!(f.status, Some(301));
        assert_eq!(f.size, None);
    }

    #[test]
    fn parse_gobuster_line_plain_path() {
        let f = parse_gobuster_line("/robots.txt");
        assert_eq!(f.finding, "/robots.txt");
        assert_eq!(f.status, None);
        assert_eq!(f.size, None);
    }

    #[test]
    fn url_validation_rejects_bad_urls() {
        let tools = ReconTools::new(std::env::temp_dir());
        let err = tools
            .run(WebReconArgs {
                url: "not-a-url".to_string(),
                scanners: "gobuster".to_string(),
                wordlist: None,
                gobuster_extra: None,
                timeout_ms: None,
            })
            .unwrap_err();
        assert!(err.contains("http:// or https://"));

        let err = tools
            .run(WebReconArgs {
                url: "".to_string(),
                scanners: "gobuster".to_string(),
                wordlist: None,
                gobuster_extra: None,
                timeout_ms: None,
            })
            .unwrap_err();
        assert!(err.contains("url is required"));
    }
}
