//! Skill tracking for nanomite.
//!
//! Every tool nanomite successfully installs or uses is recorded as a "skill"
//! — a durable, queryable record that the agent can consult in future turns.
//! Skills are persisted to a JSON file so they survive restarts, and each
//! skill tracks: when it was first installed, which package manager installed
//! it, how many times it has been used, and when it was last used.
//!
//! The agent can list its skills at any time via the `skills_list` tool and
//! query whether it knows how to use a specific tool via `skills_known`.
//! This gives the model a memory of "I installed nmap before, I can use it
//! again" without re-probing PATH every turn.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Current unix seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One tracked skill: a tool the agent knows how to install and use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Tool name (e.g. `nmap`, `sqlmap`, `hydra`).
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// Category: recon, web, wireless, password, exploitation, forensics, etc.
    pub category: SkillCategory,
    /// Package manager used (`brew`, `apt`, `system`, `none`).
    pub installed_via: String,
    /// Package name passed to the manager (empty if pre-installed).
    pub package_name: String,
    /// Unix seconds when first recorded.
    pub first_recorded: u64,
    /// Unix seconds when last successfully used.
    pub last_used: u64,
    /// Number of successful invocations.
    pub use_count: u64,
    /// Whether the tool is currently known-present on PATH.
    pub present: bool,
    /// Free-form notes (installation notes, usage tips, flags).
    pub notes: String,
}

/// Skill categories mirroring the agent's security capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum SkillCategory {
    Recon,
    Web,
    Wireless,
    Password,
    Exploitation,
    Forensics,
    Network,
    System,
    Osint,
}

impl std::fmt::Display for SkillCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillCategory::Recon => write!(f, "recon"),
            SkillCategory::Web => write!(f, "web"),
            SkillCategory::Wireless => write!(f, "wireless"),
            SkillCategory::Password => write!(f, "password"),
            SkillCategory::Exploitation => write!(f, "exploitation"),
            SkillCategory::Forensics => write!(f, "forensics"),
            SkillCategory::Network => write!(f, "network"),
            SkillCategory::System => write!(f, "system"),
            SkillCategory::Osint => write!(f, "osint"),
        }
    }
}

/// Default skill descriptions for common security tools.
const DEFAULT_SKILLS: &[(&str, &str, &str, &str, &str)] = &[
    // (name, description, category, installed_via, package_name)
    (
        "nmap",
        "Network discovery and security auditing / port scanner",
        "recon",
        "system",
        "nmap",
    ),
    (
        "nikto",
        "Web server vulnerability scanner",
        "web",
        "system",
        "nikto",
    ),
    (
        "sqlmap",
        "Automatic SQL injection and database takeover tool",
        "web",
        "system",
        "sqlmap",
    ),
    (
        "hydra",
        "Network login cracker supporting many protocols",
        "password",
        "system",
        "hydra",
    ),
    (
        "john",
        "John the Ripper password cracker",
        "password",
        "system",
        "john",
    ),
    (
        "hashcat",
        "World's fastest CPU/GPU password recovery tool",
        "password",
        "system",
        "hashcat",
    ),
    (
        "aircrack-ng",
        "Wi-Fi security auditing and attack suite",
        "wireless",
        "system",
        "aircrack-ng",
    ),
    (
        "wifite",
        "Automated Wi-Fi attack tool",
        "wireless",
        "system",
        "wifite",
    ),
    (
        "masscan",
        "Fast Internet-scale port scanner",
        "recon",
        "system",
        "masscan",
    ),
    (
        "gobuster",
        "Directory/file & DNS busting tool",
        "web",
        "system",
        "gobuster",
    ),
    ("dirb", "Web content scanner", "web", "system", "dirb"),
    (
        "zaproxy",
        "OWASP Zed Attack Proxy (web app scanner)",
        "web",
        "system",
        "zaproxy",
    ),
    (
        "burpsuite",
        "Web application security testing platform",
        "web",
        "system",
        "burpsuite",
    ),
    (
        "metasploit",
        "Penetration testing framework",
        "exploitation",
        "system",
        "metasploit",
    ),
    (
        "exiftool",
        "Read/write metadata from images, documents, binaries",
        "forensics",
        "system",
        "exiftool",
    ),
    (
        "strings",
        "Extract printable strings from binary files",
        "forensics",
        "system",
        "binutils",
    ),
    (
        "tesseract",
        "OCR engine for image-to-text",
        "forensics",
        "system",
        "tesseract",
    ),
    (
        "whois",
        "Domain/IP registration lookup",
        "osint",
        "system",
        "whois",
    ),
];

/// Skill registry: in-memory map persisted to disk.
#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    skills: BTreeMap<String, Skill>,
    path: Option<PathBuf>,
}

impl SkillRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry that persists to `path`.
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..Default::default()
        }
    }

    /// Seed the registry with default security-tool skills (not yet installed).
    pub fn seed_defaults(&mut self) {
        let now = now_secs();
        for (name, desc, cat, via, pkg) in DEFAULT_SKILLS {
            let category = match *cat {
                "recon" => SkillCategory::Recon,
                "web" => SkillCategory::Web,
                "wireless" => SkillCategory::Wireless,
                "password" => SkillCategory::Password,
                "exploitation" => SkillCategory::Exploitation,
                "forensics" => SkillCategory::Forensics,
                "network" => SkillCategory::Network,
                "system" => SkillCategory::System,
                "osint" => SkillCategory::Osint,
                _ => SkillCategory::System,
            };
            self.skills.insert(
                name.to_string(),
                Skill {
                    name: name.to_string(),
                    description: desc.to_string(),
                    category,
                    installed_via: via.to_string(),
                    package_name: pkg.to_string(),
                    first_recorded: now,
                    last_used: now,
                    use_count: 0,
                    present: false,
                    notes: String::new(),
                },
            );
        }
        self.save();
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

    /// Load skills from the persisted JSON file if present; otherwise seed
    /// defaults and save.
    pub fn load(path: &PathBuf) -> Self {
        let skills = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => BTreeMap::new(),
        };
        let mut reg = Self {
            skills,
            path: Some(path.clone()),
        };
        if reg.skills.is_empty() {
            reg.seed_defaults();
        }
        reg
    }

    /// Record that `name` was successfully installed via `via` as `package`.
    /// Updates timestamps and presence; creates a default entry if missing.
    pub fn record_installed(
        &mut self,
        name: &str,
        via: &str,
        package: &str,
        category: SkillCategory,
    ) {
        let now = now_secs();
        let entry = self.skills.entry(name.to_string()).or_insert(Skill {
            name: name.to_string(),
            description: String::new(),
            category,
            installed_via: via.to_string(),
            package_name: package.to_string(),
            first_recorded: now,
            last_used: now,
            use_count: 0,
            present: true,
            notes: String::new(),
        });
        entry.installed_via = via.to_string();
        entry.package_name = package.to_string();
        entry.present = true;
        entry.last_used = now;
        self.save();
    }

    /// Mark `name` as successfully used: update last_used and increment
    /// use_count.
    pub fn record_used(&mut self, name: &str) {
        let now = now_secs();
        if let Some(entry) = self.skills.get_mut(name) {
            entry.last_used = now;
            entry.use_count = entry.use_count.saturating_add(1);
            entry.present = true;
            self.save();
        }
    }

    /// Mark `name` as present on PATH (detected without install).
    pub fn record_present(&mut self, name: &str, category: SkillCategory) {
        let now = now_secs();
        let entry = self.skills.entry(name.to_string()).or_insert(Skill {
            name: name.to_string(),
            description: String::new(),
            category,
            installed_via: "system".to_string(),
            package_name: String::new(),
            first_recorded: now,
            last_used: now,
            use_count: 0,
            present: true,
            notes: String::new(),
        });
        entry.present = true;
        entry.last_used = now;
        self.save();
    }

    /// Look up a skill by name.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// List all skills, newest-first by first_recorded.
    pub fn list(&self) -> Vec<&Skill> {
        let mut v: Vec<&Skill> = self.skills.values().collect();
        v.sort_by_key(|s| std::cmp::Reverse(s.first_recorded));
        v
    }

    /// List skills present on PATH.
    pub fn present(&self) -> Vec<&Skill> {
        self.skills.values().filter(|s| s.present).collect()
    }

    /// List skills missing from PATH.
    pub fn missing(&self) -> Vec<&Skill> {
        self.skills.values().filter(|s| !s.present).collect()
    }

    /// Total skill count.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Count by category.
    pub fn count_by_category(&self) -> Vec<(SkillCategory, usize)> {
        let mut counts: BTreeMap<SkillCategory, usize> = BTreeMap::new();
        for s in self.skills.values() {
            *counts.entry(s.category).or_insert(0) += 1;
        }
        counts.into_iter().collect()
    }
}

/// Arguments for `skills_list` tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillsListArgs {
    /// Filter to only present skills: "present", "missing", or "all".
    #[serde(default)]
    pub filter: String,
}

/// Arguments for `skills_known` tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillsKnownArgs {
    /// Tool name to check.
    pub name: String,
}

/// Result of `skills_list`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillsListResult {
    pub skills: Vec<SkillSummary>,
    pub total: usize,
    pub present_count: usize,
    pub missing_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillSummary {
    pub name: String,
    pub category: String,
    pub installed_via: String,
    pub package_name: String,
    pub present: bool,
    pub use_count: u64,
    pub first_recorded: u64,
    pub last_used: u64,
}

/// Result of `skills_known`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillsKnownResult {
    pub name: String,
    pub known: bool,
    pub present: bool,
    pub installed_via: Option<String>,
    pub package_name: Option<String>,
    pub use_count: u64,
    pub category: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_defaults_creates_all_known_skills() {
        let mut reg = SkillRegistry::new();
        reg.seed_defaults();
        assert!(!reg.skills.is_empty());
        assert!(reg.get("nmap").is_some());
        assert!(reg.get("sqlmap").is_some());
        assert!(reg.get("hydra").is_some());
        assert!(reg.get("aircrack-ng").is_some());
    }

    #[test]
    fn record_installed_marks_present() {
        let mut reg = SkillRegistry::new();
        reg.seed_defaults();
        reg.record_installed("nmap", "brew", "nmap", SkillCategory::Recon);
        let s = reg.get("nmap").unwrap();
        assert!(s.present);
        assert_eq!(s.installed_via, "brew");
        assert_eq!(s.package_name, "nmap");
    }

    #[test]
    fn record_used_increments_count() {
        let mut reg = SkillRegistry::new();
        reg.seed_defaults();
        reg.record_used("nmap");
        let s = reg.get("nmap").unwrap();
        assert_eq!(s.use_count, 1);
        assert!(s.present);
    }

    #[test]
    fn present_and_missing_partition() {
        let mut reg = SkillRegistry::new();
        reg.seed_defaults();
        assert_eq!(reg.present().len(), 0);
        assert_eq!(reg.missing().len(), reg.len());
        reg.record_present("nmap", SkillCategory::Recon);
        assert_eq!(reg.present().len(), 1);
        assert_eq!(reg.missing().len(), reg.len() - 1);
    }

    #[test]
    fn category_counts_work() {
        let mut reg = SkillRegistry::new();
        reg.seed_defaults();
        let counts = reg.count_by_category();
        let total: usize = counts.iter().map(|(_, c)| c).sum();
        assert_eq!(total, reg.len());
    }
}
