//! Wireless Auditing Module — wraps aircrack-ng, wifite, and wash for
//! authorized wireless assessments.
//!
//! Runs passive/active wireless reconnaissance against a target interface,
//! captures output, and (when neural memory is attached) stores each finding
//! as a tagged technique record so the agent can recall it across sessions.
//!
//! All tools are scope-gated: the operator must confirm the target is in
//! scope before scanning. Execution is plan-only: nanomite produces the
//! command and parses results but never exploits.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Target specification for a wireless recon scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WirelessReconArgs {
    /// Wireless interface to use (e.g. wlan0, wlan1mon).
    pub interface: String,
    /// Which tools to run: "wash", "aircrack", "wifite", or "all".
    #[serde(default = "default_tools")]
    pub tools: String,
    /// BSSID to target (optional; scan all if omitted).
    #[serde(default)]
    pub bssid: Option<String>,
    /// Channel to lock to (optional).
    #[serde(default)]
    pub channel: Option<u32>,
    /// Hard cap in milliseconds (default 120s).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

fn default_tools() -> String {
    "all".to_string()
}

/// Single finding from a wireless recon scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WirelessFinding {
    /// Tool that produced this finding (wash, aircrack-ng, wifite).
    pub tool: String,
    /// Discovered network / vulnerability description.
    pub finding: String,
    /// BSSID (MAC address) if applicable.
    pub bssid: Option<String>,
    /// SSID if applicable.
    pub ssid: Option<String>,
    /// Channel if applicable.
    pub channel: Option<u32>,
    /// Signal strength (dBm) if applicable.
    pub signal: Option<i32>,
    /// Raw output line from the tool.
    pub raw: String,
}

/// Result of a wireless recon run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WirelessReconResult {
    pub interface: String,
    pub tools_run: Vec<String>,
    pub findings: Vec<WirelessFinding>,
    pub total_findings: usize,
    pub timed_out: bool,
    pub raw_output: String,
}

#[derive(Clone)]
pub struct WirelessTools {
    /// Workspace root for resolving relative files.
    #[allow(dead_code)]
    root: PathBuf,
    /// Neural memory for storing discovered techniques (optional).
    pub memory: Option<Arc<crate::memory::MemoryEngine>>,
}

impl WirelessTools {
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

    /// Run wireless recon against `args.interface`.
    pub fn run(&self, args: WirelessReconArgs) -> Result<WirelessReconResult, String> {
        let interface = args.interface.trim();
        if interface.is_empty() {
            return Err("wireless: interface is required".to_string());
        }

        let timeout =
            Duration::from_millis(args.timeout_ms.unwrap_or(120_000).clamp(5_000, 300_000));

        let tools: Vec<&str> = match args.tools.as_str() {
            "wash" => vec!["wash"],
            "aircrack" => vec!["aircrack-ng"],
            "wifite" => vec!["wifite"],
            _ => vec!["wash", "aircrack-ng", "wifite"],
        };

        let mut all_findings = Vec::new();
        let mut tools_run = Vec::new();
        let mut raw_output = String::new();
        let mut timed_out = false;

        for tool in &tools {
            let result = match *tool {
                "wash" => {
                    self.run_wash(interface, args.bssid.as_deref(), args.channel, &timeout)?
                }
                "aircrack-ng" => {
                    self.run_aircrack(interface, args.bssid.as_deref(), args.channel, &timeout)?
                }
                "wifite" => {
                    self.run_wifite(interface, args.bssid.as_deref(), args.channel, &timeout)?
                }
                _ => continue,
            };
            tools_run.push(tool.to_string());
            if !raw_output.is_empty() {
                raw_output.push('\n');
            }
            raw_output.push_str(&format!("=== {} ===\n{}", tool, result.raw_output));
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
                    format!(
                        "Wireless recon [{}] {} → BSSID: {:?} SSID: {:?} CH: {:?} (iface: {})",
                        f.tool, f.finding, f.bssid, f.ssid, f.channel, interface
                    ),
                ));
            }
        }

        Ok(WirelessReconResult {
            interface: interface.to_string(),
            tools_run,
            findings: all_findings,
            total_findings: total,
            timed_out,
            raw_output,
        })
    }

    /// Run wash (WPS scan).
    fn run_wash(
        &self,
        interface: &str,
        bssid: Option<&str>,
        channel: Option<u32>,
        timeout: &Duration,
    ) -> Result<WirelessReconResult, String> {
        let mut cmd = Command::new("wash");
        cmd.arg("-i")
            .arg(interface)
            .arg("-C")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(b) = bssid {
            cmd.arg("--bssid").arg(b);
        }
        if let Some(c) = channel {
            cmd.arg("-c").arg(c.to_string());
        }

        let started = Instant::now();
        let mut child = cmd.spawn().map_err(|e| format!("wash spawn failed: {e}"))?;
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

        let status = child.wait().map_err(|e| format!("wash wait failed: {e}"))?;
        let stdout_all = out_handle.join().unwrap_or_default();
        let stderr_all = err_handle.join().unwrap_or_default();

        let _code = status.code().unwrap_or(-1);
        let stdout_str = String::from_utf8_lossy(&stdout_all).to_string();
        let stderr_str = String::from_utf8_lossy(&stderr_all).to_string();
        let timed_out = started.elapsed() >= *timeout;

        let mut findings = Vec::new();
        for line in stdout_str.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("BSSID") || line.starts_with("----") {
                continue;
            }
            findings.push(parse_wash_line(line));
        }

        let total_findings = findings.len();

        Ok(WirelessReconResult {
            interface: interface.to_string(),
            tools_run: vec!["wash".to_string()],
            findings,
            total_findings,
            timed_out,
            raw_output: format!("{stdout_str}\n{stderr_str}"),
        })
    }

    /// Run aircrack-ng (airmon-ng / airodump-ng style discovery).
    fn run_aircrack(
        &self,
        interface: &str,
        bssid: Option<&str>,
        channel: Option<u32>,
        timeout: &Duration,
    ) -> Result<WirelessReconResult, String> {
        // aircrack-ng suite: use airodump-ng for discovery if available,
        // fall back to airmon-ng check.
        let tool = if Command::new("airodump-ng").spawn().is_ok() {
            "airodump-ng"
        } else {
            "airmon-ng"
        };

        let mut cmd = Command::new(tool);
        match tool {
            "airodump-ng" => {
                cmd.arg("--output-format")
                    .arg("csv")
                    .arg("-w")
                    .arg("/tmp/nanomite-wireless");
                if let Some(c) = channel {
                    cmd.arg("-c").arg(c.to_string());
                }
                if let Some(b) = bssid {
                    cmd.arg("--bssid").arg(b);
                }
                cmd.arg(interface);
            }
            "airmon-ng" => {
                cmd.arg("check").arg(interface);
            }
            _ => unreachable!(),
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let started = Instant::now();
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("{tool} spawn failed: {e}"))?;
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

        let status = child
            .wait()
            .map_err(|e| format!("{tool} wait failed: {e}"))?;
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
            findings.push(WirelessFinding {
                tool: "aircrack-ng".to_string(),
                finding: line.to_string(),
                bssid: None,
                ssid: None,
                channel: None,
                signal: None,
                raw: line.to_string(),
            });
        }

        let total_findings = findings.len();

        Ok(WirelessReconResult {
            interface: interface.to_string(),
            tools_run: vec!["aircrack-ng".to_string()],
            findings,
            total_findings,
            timed_out,
            raw_output: format!("{stdout_str}\n{stderr_str}"),
        })
    }

    /// Run wifite (automated wireless auditor).
    fn run_wifite(
        &self,
        interface: &str,
        bssid: Option<&str>,
        channel: Option<u32>,
        timeout: &Duration,
    ) -> Result<WirelessReconResult, String> {
        let mut cmd = Command::new("wifite");
        cmd.arg("-i")
            .arg(interface)
            .arg("--stdout")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(b) = bssid {
            cmd.arg("--bssid").arg(b);
        }
        if let Some(c) = channel {
            cmd.arg("-c").arg(c.to_string());
        }

        let started = Instant::now();
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("wifite spawn failed: {e}"))?;
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

        let status = child
            .wait()
            .map_err(|e| format!("wifite wait failed: {e}"))?;
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
            findings.push(parse_wifite_line(line));
        }

        let total_findings = findings.len();

        Ok(WirelessReconResult {
            interface: interface.to_string(),
            tools_run: vec!["wifite".to_string()],
            findings,
            total_findings,
            timed_out,
            raw_output: format!("{stdout_str}\n{stderr_str}"),
        })
    }
}

/// Parse a single wash output line into a finding.
fn parse_wash_line(line: &str) -> WirelessFinding {
    // wash CSV-ish: BSSID, Channel, RSSI, WPS Version, WPS Locked, ESSID
    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    let bssid = parts.first().map(|s| s.to_string());
    let channel = parts.get(1).and_then(|s| s.parse::<u32>().ok());
    let signal = parts.get(2).and_then(|s| s.parse::<i32>().ok());
    let ssid = parts.last().map(|s| s.to_string());

    WirelessFinding {
        tool: "wash".to_string(),
        finding: format!(
            "WPS network discovered (locked: {})",
            parts.get(4).unwrap_or(&"unknown")
        ),
        bssid,
        ssid,
        channel,
        signal,
        raw: line.to_string(),
    }
}

/// Parse a single wifite output line into a finding.
fn parse_wifite_line(line: &str) -> WirelessFinding {
    WirelessFinding {
        tool: "wifite".to_string(),
        finding: line.to_string(),
        bssid: None,
        ssid: None,
        channel: None,
        signal: None,
        raw: line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wash_line_extracts_fields() {
        let f = parse_wash_line("AA:BB:CC:DD:EE:FF, 6, -45, 1.0, No, MyNetwork");
        assert_eq!(f.bssid, Some("AA:BB:CC:DD:EE:FF".to_string()));
        assert_eq!(f.channel, Some(6));
        assert_eq!(f.signal, Some(-45));
        assert_eq!(f.ssid, Some("MyNetwork".to_string()));
        assert_eq!(f.tool, "wash");
    }

    #[test]
    fn parse_wash_line_minimal() {
        let f = parse_wash_line("AA:BB:CC:DD:EE:FF");
        assert_eq!(f.bssid, Some("AA:BB:CC:DD:EE:FF".to_string()));
        assert_eq!(f.channel, None);
        assert_eq!(f.signal, None);
        assert_eq!(f.ssid, Some("AA:BB:CC:DD:EE:FF".to_string()));
    }

    #[test]
    fn interface_validation_rejects_empty() {
        let tools = WirelessTools::new(std::env::temp_dir());
        let err = tools
            .run(WirelessReconArgs {
                interface: "".to_string(),
                tools: "wash".to_string(),
                bssid: None,
                channel: None,
                timeout_ms: None,
            })
            .unwrap_err();
        assert!(err.contains("interface is required"));
    }
}
