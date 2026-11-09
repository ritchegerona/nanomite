//! Kali integration: a curated bridge to installed assessment tools.
//!
//! This is an orchestration layer, not a tool implementation. Each entry runs a
//! **fixed, conservative argv template** with one authorized target - arbitrary
//! flags are not accepted, so the model cannot turn it into a shell or point a
//! tool at a second host. The catalogue is deliberately limited to
//! reconnaissance, enumeration, web/TLS analysis and vulnerability scanning.
//! Credential-attack, wireless-attack and exploitation-framework tools
//! (`hydra`, `john`, `hashcat`, `aircrack-ng`, `responder`, `msfconsole`,
//! `sqlmap`, ...) are **not** wired in: use `run_command` or your own
//! authorized framework for those.
//!
//! Process execution is bounded by the shared `run_with_timeout` path in
//! `tools::external_tools`, so there is one timeout/kill/output-cap
//! implementation. The runner and PATH probe are injected for offline tests.

use std::path::PathBuf;
use std::time::Duration;

use super::ModuleReport;
use crate::hacker::findings::{Confidence, Finding};
use crate::hacker::registry::ModuleCategory;
use crate::hacker::risk::RiskLevel;

/// Hard cap for one Kali tool run.
pub const KALI_TIMEOUT: Duration = Duration::from_secs(180);
/// Report body lines kept from stdout.
const MAX_STDOUT_LINES: usize = 120;
/// Report body lines kept from stderr.
const MAX_STDERR_LINES: usize = 40;
/// Characters kept per reported line.
const MAX_LINE_CHARS: usize = 240;

/// One catalogued assessment tool.
#[derive(Debug, Clone, Copy)]
pub struct KaliTool {
    pub name: &'static str,
    pub description: &'static str,
    pub category: ModuleCategory,
    pub risk: RiskLevel,
    /// Argument template; `{target}` is replaced with the authorized target.
    pub template: &'static [&'static str],
}

const fn tool(
    name: &'static str,
    description: &'static str,
    category: ModuleCategory,
    risk: RiskLevel,
    template: &'static [&'static str],
) -> KaliTool {
    KaliTool {
        name,
        description,
        category,
        risk,
        template,
    }
}

/// The shipped catalogue. Assessment/enumeration only - see the module docs.
pub const CATALOGUE: &[KaliTool] = &[
    tool(
        "nmap",
        "service/version discovery over the top 100 ports",
        ModuleCategory::Network,
        RiskLevel::Medium,
        &[
            "-Pn",
            "-sV",
            "--version-light",
            "-T4",
            "--max-retries",
            "1",
            // Cap per-probe RTT: with default timing, filtered ports stall
            // the scan for the full TCP timeout (~40s/host measured); the
            // caps cut that to ~10s with no loss on responsive hosts.
            "--initial-rtt-timeout",
            "200ms",
            "--max-rtt-timeout",
            "300ms",
            "--host-timeout",
            "60s",
            "--top-ports",
            "100",
            "{target}",
        ],
    ),
    tool(
        "masscan",
        "fast bounded port sweep",
        ModuleCategory::Network,
        RiskLevel::Medium,
        &["--top-ports", "100", "--rate", "100", "{target}"],
    ),
    tool(
        "naabu",
        "port sweep",
        ModuleCategory::Network,
        RiskLevel::Medium,
        &["-silent", "-top-ports", "100", "-host", "{target}"],
    ),
    tool(
        "rustscan",
        "port scan",
        ModuleCategory::Network,
        RiskLevel::Medium,
        &["-a", "{target}", "--top", "100"],
    ),
    tool(
        "nikto",
        "web server misconfiguration scan",
        ModuleCategory::Web,
        RiskLevel::Medium,
        &["-h", "{target}", "-nointeractive"],
    ),
    tool(
        "whatweb",
        "web technology fingerprint",
        ModuleCategory::Web,
        RiskLevel::Medium,
        &["--no-color", "{target}"],
    ),
    tool(
        "httpx",
        "HTTP probe: status, title, technologies",
        ModuleCategory::Recon,
        RiskLevel::Medium,
        &[
            "-silent",
            "-status-code",
            "-title",
            "-tech-detect",
            "{target}",
        ],
    ),
    tool(
        "wpscan",
        "WordPress enumeration",
        ModuleCategory::Web,
        RiskLevel::Medium,
        &["--no-banner", "--no-update", "--url", "{target}"],
    ),
    tool(
        "nuclei",
        "template-based checks",
        ModuleCategory::Vulnerability,
        RiskLevel::High,
        &[
            "-silent",
            "-u",
            "{target}",
            "-severity",
            "low,medium,high,critical",
        ],
    ),
    tool(
        "sslyze",
        "TLS configuration analysis",
        ModuleCategory::Web,
        RiskLevel::Medium,
        &["--regular", "{target}"],
    ),
    tool(
        "sslscan",
        "TLS configuration analysis",
        ModuleCategory::Web,
        RiskLevel::Medium,
        &["--no-colour", "{target}"],
    ),
    tool(
        "testssl.sh",
        "TLS configuration analysis",
        ModuleCategory::Web,
        RiskLevel::Medium,
        &["--quiet", "{target}"],
    ),
    tool(
        "wafw00f",
        "WAF fingerprint",
        ModuleCategory::Web,
        RiskLevel::Medium,
        &["{target}"],
    ),
    tool(
        "subfinder",
        "passive subdomain enumeration",
        ModuleCategory::Recon,
        RiskLevel::Medium,
        &["-silent", "-d", "{target}"],
    ),
    tool(
        "assetfinder",
        "passive subdomain enumeration",
        ModuleCategory::Recon,
        RiskLevel::Medium,
        &["--subs-only", "{target}"],
    ),
    tool(
        "amass",
        "passive attack-surface mapping",
        ModuleCategory::Recon,
        RiskLevel::Medium,
        &["enum", "-passive", "-d", "{target}"],
    ),
    tool(
        "dnsrecon",
        "DNS record enumeration",
        ModuleCategory::Recon,
        RiskLevel::Medium,
        &["-d", "{target}", "-t", "std"],
    ),
    tool(
        "whois",
        "registration data",
        ModuleCategory::Osint,
        RiskLevel::Low,
        &["{target}"],
    ),
    tool(
        "dig",
        "DNS A records",
        ModuleCategory::Recon,
        RiskLevel::Low,
        &["+short", "A", "{target}"],
    ),
    tool(
        "host",
        "DNS A records",
        ModuleCategory::Recon,
        RiskLevel::Low,
        &["-t", "A", "{target}"],
    ),
    tool(
        "nbtscan",
        "NetBIOS name scan",
        ModuleCategory::Network,
        RiskLevel::Medium,
        &["{target}"],
    ),
    tool(
        "enum4linux",
        "SMB/NetBIOS enumeration",
        ModuleCategory::Network,
        RiskLevel::High,
        &["-a", "{target}"],
    ),
];

/// Looks up a catalogued tool by name (case-insensitive).
pub fn find(name: &str) -> Option<&'static KaliTool> {
    CATALOGUE
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name.trim()))
}

/// Builds the argv for `tool` and `target`. Never invokes a shell.
pub fn build_argv(tool: &KaliTool, target: &str) -> Vec<String> {
    tool.template
        .iter()
        .map(|a| a.replace("{target}", target))
        .collect()
}

/// Output of one bounded process run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcOutput {
    pub exit_code: i32,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

/// Executes one bounded process. Injected so tests never spawn anything.
pub trait ProcessRunner: Send + Sync {
    fn run(&self, program: &str, args: &[String], timeout: Duration) -> Result<ProcOutput, String>;
}

/// Production runner, delegating to the shared bounded executor.
pub struct SystemRunner;

impl ProcessRunner for SystemRunner {
    fn run(&self, program: &str, args: &[String], timeout: Duration) -> Result<ProcOutput, String> {
        let out = crate::tools::external_tools::run_bounded(program, args, None, timeout, program)?;
        Ok(ProcOutput {
            exit_code: out.exit_code,
            timed_out: out.timed_out,
            stdout: out.stdout,
            stderr: out.stderr,
            duration_ms: out.duration_ms,
        })
    }
}

/// Detects whether a binary is on PATH. Injected for offline tests.
pub trait BinaryProbe: Send + Sync {
    fn present(&self, name: &str) -> bool;
}

/// Production probe over `PATH`.
pub struct PathProbe;

impl BinaryProbe for PathProbe {
    fn present(&self, name: &str) -> bool {
        let Some(paths) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&paths).any(|dir| executable(dir.join(name)))
    }
}

fn executable(path: PathBuf) -> bool {
    let Ok(meta) = std::fs::metadata(&path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Report of which catalogued tools are installed.
pub fn available(probe: &dyn BinaryProbe) -> ModuleReport {
    let mut installed = 0;
    let mut report = ModuleReport::new("kali", "available", "local", String::new());
    for entry in CATALOGUE {
        let present = probe.present(entry.name);
        if present {
            installed += 1;
        }
        report = report.line(format!(
            "{} {:<12} {} · {} · {}",
            if present { "✓" } else { "✗" },
            entry.name,
            entry.category.label(),
            entry.risk.label(),
            entry.description
        ));
    }
    report.summary = format!(
        "{installed}/{} installed · {} missing",
        CATALOGUE.len(),
        CATALOGUE.len() - installed
    );
    report
}

/// Builds a bounded report for one completed tool run.
pub fn run_report(
    tool: &KaliTool,
    target: &str,
    argv: &[String],
    out: &ProcOutput,
) -> ModuleReport {
    let summary = if out.timed_out {
        format!("timed out after {}ms", out.duration_ms)
    } else {
        format!("exit {} · {}ms", out.exit_code, out.duration_ms)
    };
    let mut report = ModuleReport::new("kali", "run", target, summary.clone());
    report = report.line(format!(
        "tool: {} ({} · {})",
        tool.name,
        tool.category.label(),
        tool.risk.label()
    ));
    report = report.line(format!("command: {} {}", tool.name, argv.join(" ")));

    let stdout = bounded_lines(&out.stdout, MAX_STDOUT_LINES);
    if stdout.is_empty() {
        report = report.line("(no stdout)");
    }
    let mut output_excerpt = stdout.join("\n");
    for line in stdout {
        report = report.line(line);
    }
    let stderr = bounded_lines(&out.stderr, MAX_STDERR_LINES);
    for line in stderr {
        report = report.line(format!("stderr: {line}"));
    }

    // One informational finding per run so the (bounded) tool output is
    // retained as hashed evidence, not just transcript text.
    if out.timed_out {
        output_excerpt.push_str("\n(timed out)");
    }
    let detail = if output_excerpt.trim().is_empty() {
        format!("{} {} ({})", tool.name, argv.join(" "), summary)
    } else {
        format!("{} {}\n{}", tool.name, argv.join(" "), output_excerpt)
    };
    report = report.finding(
        Finding::new(
            "kali",
            "run",
            target,
            format!("{} executed", tool.name),
            RiskLevel::Informational,
        )
        .confidence(Confidence::Confirmed)
        .detail(detail),
    );
    report
}

fn bounded_lines(text: &str, max: usize) -> Vec<String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .take(max)
        .map(|l| l.chars().take(MAX_LINE_CHARS).collect())
        .collect()
}

/// Human-readable catalogue for `/hacker help`.
pub fn catalogue_text() -> Vec<String> {
    CATALOGUE
        .iter()
        .map(|t| {
            format!(
                "kali/run {:<12} {} · {} · {}",
                t.name,
                t.category.label(),
                t.risk.label(),
                t.description
            )
        })
        .collect()
}

/// True when `host` is safe to place as a single argv value: not empty, not
/// flag-like, and free of whitespace/control characters. This is the second
/// line of defence behind the no-shell executor.
pub fn safe_target(host: &str) -> bool {
    let host = host.trim();
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && !host.chars().any(|c| c.is_whitespace() || c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    struct StubProbe(HashSet<&'static str>);
    impl BinaryProbe for StubProbe {
        fn present(&self, name: &str) -> bool {
            self.0.contains(name)
        }
    }

    #[test]
    fn safe_target_rejects_flags_and_whitespace() {
        assert!(safe_target("10.0.0.5"));
        assert!(safe_target("example.com"));
        assert!(safe_target("2001:db8::1"));
        assert!(!safe_target(""));
        assert!(!safe_target("-oN"));
        assert!(!safe_target("10.0.0.5 --script"));
        assert!(!safe_target("a\nb"));
    }

    #[test]
    fn catalogue_is_unique_and_target_templated() {
        let mut seen = HashSet::new();
        for entry in CATALOGUE {
            assert!(seen.insert(entry.name), "duplicate tool {}", entry.name);
            assert!(!entry.description.is_empty());
            assert!(
                entry.template.contains(&"{target}"),
                "{} has no {{target}} placeholder",
                entry.name
            );
            assert!(!entry.name.is_empty());
        }
    }

    #[test]
    fn catalogue_excludes_weaponized_and_credential_tooling() {
        for forbidden in [
            "msfconsole",
            "metasploit",
            "hydra",
            "medusa",
            "john",
            "hashcat",
            "aircrack-ng",
            "responder",
            "sqlmap",
            "bettercap",
        ] {
            assert!(
                find(forbidden).is_none(),
                "`{forbidden}` must not be wired into the curated catalogue"
            );
        }
    }

    #[test]
    fn lookup_is_case_insensitive_and_unknown_is_none() {
        assert_eq!(find("NMAP").map(|t| t.name), Some("nmap"));
        assert!(find("nope").is_none());
    }

    #[test]
    fn argv_substitutes_every_occurrence_without_a_shell() {
        let nmap = find("nmap").unwrap();
        let argv = build_argv(nmap, "10.0.0.5");
        assert_eq!(argv.last().unwrap(), "10.0.0.5");
        assert!(!argv.iter().any(|a| a.contains(";") || a.contains("&&")));

        let dig = find("dig").unwrap();
        assert_eq!(
            build_argv(dig, "example.com"),
            vec!["+short", "A", "example.com"]
        );
    }

    #[test]
    fn available_reports_installed_and_missing() {
        let report = available(&StubProbe(HashSet::from(["nmap", "dig"])));
        assert!(report.summary.starts_with("2/"));
        assert!(report.summary.contains("missing"));
        assert!(report.lines.iter().any(|l| l.starts_with("✓ nmap")));
        assert!(report.lines.iter().any(|l| l.starts_with("✗ nikto")));
    }

    #[test]
    fn report_is_bounded_and_labels_stderr() {
        let tool = find("nmap").unwrap();
        let argv = build_argv(tool, "10.0.0.5");
        let long_stdout = (0..500)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = ProcOutput {
            exit_code: 1,
            timed_out: false,
            stdout: long_stdout,
            stderr: "boom".into(),
            duration_ms: 12,
        };
        let report = run_report(tool, "10.0.0.5", &argv, &out);
        assert_eq!(report.summary, "exit 1 · 12ms");
        assert!(report.lines.iter().any(|l| l == "stderr: boom"));
        // command line + 120 stdout lines + stderr line
        assert!(report.lines.len() <= 2 + MAX_STDOUT_LINES + 1);
        assert!(report.lines.iter().any(|l| l.starts_with("command: nmap")));
    }

    #[test]
    fn run_report_records_one_confirmed_finding_as_evidence() {
        use crate::hacker::findings::Confidence;
        let tool = find("nmap").unwrap();
        let argv = build_argv(tool, "10.0.0.5");
        let out = ProcOutput {
            exit_code: 0,
            timed_out: false,
            stdout: "PORT   STATE SERVICE\n22/tcp open  ssh".into(),
            stderr: String::new(),
            duration_ms: 42,
        };
        let report = run_report(tool, "10.0.0.5", &argv, &out);
        assert_eq!(report.findings.len(), 1);
        let f = &report.findings[0];
        assert_eq!(f.confidence, Confidence::Confirmed);
        assert_eq!(f.severity, RiskLevel::Informational);
        assert_eq!(f.title, "nmap executed");
        assert_eq!(f.target, "10.0.0.5");
        assert!(f.detail.contains("22/tcp open"), "{}", f.detail);
    }

    #[test]
    fn available_emits_no_findings() {
        let report = available(&StubProbe(HashSet::from(["nmap"])));
        assert!(report.findings.is_empty(), "inventory is not a finding");
    }

    #[test]
    fn timeout_is_reported() {
        let tool = find("whois").unwrap();
        let out = ProcOutput {
            exit_code: -1,
            timed_out: true,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 180_000,
        };
        let report = run_report(tool, "example.com", &build_argv(tool, "example.com"), &out);
        assert!(report.summary.contains("timed out"));
        assert!(report.lines.iter().any(|l| l == "(no stdout)"));
    }

    #[cfg(unix)]
    #[test]
    fn system_runner_executes_without_a_shell_and_captures_output() {
        let out = SystemRunner
            .run("echo", &["nanomite".to_string()], Duration::from_secs(5))
            .expect("echo runs");
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout.contains("nanomite"));
        assert!(!out.timed_out);
    }
}
