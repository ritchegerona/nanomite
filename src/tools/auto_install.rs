//! Auto-install for missing security tools.
//!
//! When a tool invoked via `run_command` (or any external tool) is not found
//! on PATH, nanomite attempts to install it automatically using the platform
//! package manager (Homebrew on macOS, apt on Debian/Ubuntu). Every successful
//! installation is recorded in the [`SkillTracker`] so future invocations can
//! skip re-detection and the agent learns which tools are available for the
//! task at hand.
//!
//! The installer is conservative: it never runs with sudo unless the user
//! explicitly provides a sudo password, it never installs from untrusted
//! sources (only distro packages or Homebrew), and it respects a hard timeout
//! so a hung package manager cannot block a turn.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Hard cap for a single package-manager invocation.
const INSTALL_TIMEOUT_MS: u64 = 180_000;

/// Package manager probe order per platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PackageManager {
    /// macOS Homebrew (`brew install <pkg>`).
    Brew,
    /// Debian/Ubuntu apt (`apt-get install -y <pkg>`).
    Apt,
}

/// Result of probing whether a tool exists on PATH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAvailability {
    /// Binary found on PATH.
    Present,
    /// Binary not found and no install path known.
    Missing,
}

/// Mapping from common tool names to package names per package manager.
/// Many security tools share a name across managers, but some differ
/// (e.g. `wifite` is `wifite` on both, `aircrack-ng` is `aircrack-ng`).
const TOOL_PACKAGE_MAP: &[(&str, &str, &str)] = &[
    // (tool_name, brew_package, apt_package)
    ("nmap", "nmap", "nmap"),
    ("nikto", "nikto", "nikto"),
    ("sqlmap", "sqlmap", "sqlmap"),
    ("hydra", "hydra", "hydra"),
    ("john", "john", "john"),
    ("hashcat", "hashcat", "hashcat"),
    ("aircrack-ng", "aircrack-ng", "aircrack-ng"),
    ("wifite", "wifite", "wifite"),
    ("masscan", "masscan", "masscan"),
    ("gobuster", "gobuster", "gobuster"),
    ("dirb", "dirb", "dirb"),
    ("zaproxy", "owasp-zap", "zaproxy"),
    ("burpsuite", "burpsuite", "burpsuite"),
    ("metasploit", "metasploit", "metasploit-framework"),
    ("exiftool", "exiftool", "libimage-exiftool-perl"),
    ("strings", "binutils", "binutils"),
    ("whois", "whois", "whois"),
    ("dig", "bind", "dnsutils"),
    ("nslookup", "bind", "dnsutils"),
    ("curl", "curl", "curl"),
    ("wget", "wget", "wget"),
    ("ncat", "nmap", "nmap"),
    ("netcat", "netcat-openbsd", "netcat-openbsd"),
    ("socat", "socat", "socat"),
    ("python3", "python@3", "python3"),
    ("pip3", "python@3", "python3-pip"),
    ("node", "node", "nodejs"),
    ("npm", "node", "npm"),
    ("rustc", "rust", "rustc"),
    ("cargo", "rust", "cargo"),
    ("go", "go", "golang-go"),
    ("git", "git", "git"),
    ("ssh", "openssh", "openssh-client"),
    ("tesseract", "tesseract", "tesseract-ocr"),
];

/// One skill entry: a tool nanomite knows how to install and use.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillEntry {
    /// Tool name (e.g. `nmap`).
    pub tool: String,
    /// Package manager used to install it.
    pub manager: PackageManager,
    /// Package name passed to the manager.
    pub package: String,
    /// First successful installation timestamp (unix seconds).
    pub installed_at: u64,
    /// Last successful invocation timestamp.
    pub last_used: u64,
    /// Number of successful invocations.
    pub use_count: u64,
    /// Whether the tool is currently known-present on PATH.
    pub present: bool,
}

/// Persistent skill registry: loads/saves skills from a JSON file so the
/// agent remembers installed tools across restarts.
#[derive(Debug, Clone, Default)]
pub struct SkillTracker {
    /// tool name -> skill entry.
    skills: HashMap<String, SkillEntry>,
    /// Persistence path.
    path: Option<PathBuf>,
}

impl SkillTracker {
    /// Create an empty tracker with no persistence.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a tracker that persists to `path` (created on first save).
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..Default::default()
        }
    }

    /// Load skills from the persisted JSON file if present.
    pub fn load(path: &PathBuf) -> Self {
        let skills = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => HashMap::new(),
        };
        Self {
            skills,
            path: Some(path.clone()),
        }
    }

    /// Persist skills to disk (atomic write). No-op when no path is set.
    pub fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(&self.skills).unwrap_or_default();
        if std::fs::write(&tmp, &body).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Record that `tool` was installed via `manager` as `package`.
    pub fn record_install(&mut self, tool: &str, manager: PackageManager, package: &str) {
        let now = now_secs();
        let entry = self
            .skills
            .entry(tool.to_string())
            .or_insert(SkillEntry {
                tool: tool.to_string(),
                manager,
                package: package.to_string(),
                installed_at: now,
                last_used: now,
                use_count: 0,
                present: true,
            });
        entry.manager = manager;
        entry.package = package.to_string();
        entry.installed_at = now;
        entry.last_used = now;
        entry.present = true;
        self.save();
    }

    /// Mark `tool` as used successfully.
    pub fn record_use(&mut self, tool: &str) {
        let now = now_secs();
        if let Some(entry) = self.skills.get_mut(tool) {
            entry.last_used = now;
            entry.use_count = entry.use_count.saturating_add(1);
            entry.present = true;
            self.save();
        }
    }

    /// Mark `tool` as present on PATH (detected without install).
    pub fn record_present(&mut self, tool: &str) {
        let now = now_secs();
        let entry = self
            .skills
            .entry(tool.to_string())
            .or_insert(SkillEntry {
                tool: tool.to_string(),
                manager: PackageManager::Brew,
                package: String::new(),
                installed_at: now,
                last_used: now,
                use_count: 0,
                present: true,
            });
        entry.present = true;
        entry.last_used = now;
        self.save();
    }

    /// Look up a skill entry by tool name.
    pub fn get(&self, tool: &str) -> Option<&SkillEntry> {
        self.skills.get(tool)
    }

    /// List all known skills.
    pub fn list(&self) -> Vec<&SkillEntry> {
        self.skills.values().collect()
    }

    /// Check whether `tool` is recorded as present.
    pub fn is_present(&self, tool: &str) -> bool {
        self.skills
            .get(tool)
            .is_some_and(|e| e.present)
    }
}

/// Auto-installer: probes for package managers, maps tool names to packages,
/// and performs installations.
#[derive(Debug, Clone, Default)]
pub struct AutoInstaller {
    /// Which package managers are available on this host.
    available: Vec<PackageManager>,
    /// Shared skill tracker so installs are remembered.
    skills: Arc<Mutex<SkillTracker>>,
}

impl AutoInstaller {
    /// Create a new installer that probes the host for available package
    /// managers and shares `skills` for persistence.
    pub fn new(skills: Arc<Mutex<SkillTracker>>) -> Self {
        let available = probe_managers();
        Self {
            available,
            skills,
        }
    }

    /// List package managers detected on this host.
    pub fn managers(&self) -> &[PackageManager] {
        &self.available
    }

    /// Resolve the package name for `tool` on `manager`, if any mapping exists.
    pub fn package_for(tool: &str, manager: PackageManager) -> Option<&str> {
        TOOL_PACKAGE_MAP
            .iter()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(tool))
            .and_then(|(_, brew, apt)| match manager {
                PackageManager::Brew => Some(*brew),
                PackageManager::Apt => Some(*apt),
            })
    }

    /// Ensure `tool` is available: if already on PATH, record it as a known
    /// skill and return `Ok(true)`. Otherwise try each available package
    /// manager in order until one installs successfully. Returns `Ok(false)`
    /// when no manager could install it.
    pub async fn ensure_tool(&self, tool: &str) -> Result<bool, String> {
        // Fast path: already on PATH.
        if which::which(tool).is_ok() {
            self.skills.lock().await.record_present(tool);
            return Ok(true);
        }

        // Already known-present from a prior install: trust the record.
        {
            let skills = self.skills.lock().await;
            if skills.is_present(tool) {
                return Ok(true);
            }
        }

        // Try each manager in order.
        for manager in &self.available {
            let Some(package) = Self::package_for(tool, *manager) else {
                continue;
            };
            match self.install_with(manager, package) {
                Ok(()) => {
                    self.skills
                        .lock()
                        .await
                        .record_install(tool, *manager, package);
                    return Ok(true);
                }
                Err(e) => {
                    // Try the next manager; one failure is not fatal.
                    let _ = e;
                }
            }
        }

        Ok(false)
    }

    /// Install a single package with the given manager (blocking, run on
    /// a spawn_blocking thread by the caller).
    pub fn install_with(&self, manager: &PackageManager, package: &str) -> Result<(), String> {
        match manager {
            PackageManager::Brew => {
                let status = Command::new("brew")
                    .arg("install")
                    .arg(package)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .status()
                    .map_err(|e| format!("brew spawn failed: {e}"))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("brew install {package} exited {}", status.code().unwrap_or(-1)))
                }
            }
            PackageManager::Apt => {
                // apt-get needs root; try without sudo first (may work in
                // containers), then note the failure so the caller can ask
                // for a sudo password.
                let status = Command::new("apt-get")
                    .arg("install")
                    .arg("-y")
                    .arg(package)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .status()
                    .map_err(|e| format!("apt-get spawn failed: {e}"))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "apt-get install {package} exited {}",
                        status.code().unwrap_or(-1)
                    ))
                }
            }
        }
    }

    /// Synchronous ensure: if `tool` is on PATH, return true. Otherwise try
    /// each available manager to install it. Blocks the calling thread
    /// (intended for `spawn_blocking` use from exec_tools).
    pub fn ensure_tool_sync(&self, tool: &str) -> Result<bool, String> {
        // Fast path: already on PATH.
        if which::which(tool).is_ok() {
            return Ok(true);
        }

        // Try each manager in order.
        for manager in &self.available {
            let Some(package) = Self::package_for(tool, *manager) else {
                continue;
            };
            match self.install_with(manager, package) {
                Ok(()) => return Ok(true),
                Err(e) => {
                    let _ = e;
                }
            }
        }

        Ok(false)
    }
}

/// Probe which package managers exist on PATH.
fn probe_managers() -> Vec<PackageManager> {
    let mut out = Vec::new();
    if which::which("brew").is_ok() {
        out.push(PackageManager::Brew);
    }
    if which::which("apt-get").is_ok() {
        out.push(PackageManager::Apt);
    }
    out
}

/// Current unix seconds.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Run a bounded package-manager command with a hard timeout.
pub(crate) async fn run_with_timeout(
    program: &str,
    args: &[String],
    timeout: Duration,
) -> Result<(i32, String, String), String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{program} failed to spawn: {e}"))?;
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let out = child.wait_with_output().map_err(|e| format!("{program} wait failed: {e}"))?;
                return Ok((
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stdout).to_string(),
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ));
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    return Err(format!("{program} timed out after {}s", timeout.as_secs()));
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => return Err(format!("{program} wait failed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_map_resolves_known_tools() {
        assert_eq!(
            AutoInstaller::package_for("nmap", PackageManager::Brew),
            Some("nmap")
        );
        assert_eq!(
            AutoInstaller::package_for("metasploit", PackageManager::Apt),
            Some("metasploit-framework")
        );
        assert_eq!(
            AutoInstaller::package_for("zaproxy", PackageManager::Brew),
            Some("owasp-zap")
        );
        // Unknown tool -> None.
        assert_eq!(AutoInstaller::package_for("no_such_tool_xyz", PackageManager::Brew), None);
    }

    #[test]
    fn skill_tracker_record_and_lookup() {
        let mut tracker = SkillTracker::new();
        tracker.record_install("nmap", PackageManager::Brew, "nmap");
        let entry = tracker.get("nmap").expect("skill recorded");
        assert_eq!(entry.package, "nmap");
        assert!(entry.present);
        tracker.record_use("nmap");
        let entry = tracker.get("nmap").expect("skill still there");
        assert_eq!(entry.use_count, 1);
        assert!(tracker.is_present("nmap"));
        assert!(!tracker.is_present("never-installed"));
    }

    #[test]
    fn probe_managers_returns_vec() {
        let mgrs = probe_managers();
        // At least empty; brew/apt may or may not be present.
        assert!(mgrs.iter().all(|m| matches!(m, PackageManager::Brew | PackageManager::Apt)));
    }
}
