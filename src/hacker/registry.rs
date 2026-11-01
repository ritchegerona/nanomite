//! Module registry: the catalogue of HACKER MODE capability modules.
//!
//! Phase 1 ships descriptor modules (metadata only) so `/hacker modules` is
//! meaningful; actual execution lands per module in later phases. The trait is
//! the extension point those phases implement - adding a module must not touch
//! the coordinator, the policy layer, or the UI.

use super::risk::RiskLevel;

/// Coarse grouping used by the module list and the assessment's permitted set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleCategory {
    Recon,
    Network,
    Web,
    Vulnerability,
    Forensics,
    Osint,
    ThreatIntel,
    Password,
    Wireless,
    Exploitation,
    PostExploitation,
    Reporting,
}

impl ModuleCategory {
    pub fn label(self) -> &'static str {
        match self {
            ModuleCategory::Recon => "reconnaissance",
            ModuleCategory::Network => "network",
            ModuleCategory::Web => "web",
            ModuleCategory::Vulnerability => "vulnerability",
            ModuleCategory::Forensics => "forensics",
            ModuleCategory::Osint => "osint",
            ModuleCategory::ThreatIntel => "threat-intel",
            ModuleCategory::Password => "password",
            ModuleCategory::Wireless => "wireless",
            ModuleCategory::Exploitation => "exploitation",
            ModuleCategory::PostExploitation => "post-exploitation",
            ModuleCategory::Reporting => "reporting",
        }
    }
}

/// One capability a module offers, with its floor risk level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleCapability {
    pub id: &'static str,
    pub label: &'static str,
    pub risk: RiskLevel,
}

/// The extension point for a HACKER MODE module. Implementations live in
/// `hacker/modules/*` and are pure descriptors in Phase 1.
pub trait HackerModule: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn category(&self) -> ModuleCategory;
    fn capabilities(&self) -> Vec<ModuleCapability>;
}

/// A metadata-only module built from a static table.
pub struct DescriptorModule {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: ModuleCategory,
    pub capabilities: &'static [ModuleCapability],
}

impl HackerModule for DescriptorModule {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn category(&self) -> ModuleCategory {
        self.category
    }
    fn capabilities(&self) -> Vec<ModuleCapability> {
        self.capabilities.to_vec()
    }
}

const fn cap(id: &'static str, label: &'static str, risk: RiskLevel) -> ModuleCapability {
    ModuleCapability { id, label, risk }
}

const CAPS_RECON: &[ModuleCapability] = &[
    cap("domain-inventory", "domain inventory", RiskLevel::Low),
    cap("dns-info", "DNS information", RiskLevel::Low),
    cap("host-discovery", "host discovery", RiskLevel::Medium),
];
const CAPS_NETWORK: &[ModuleCapability] = &[
    cap("port-inventory", "port inventory", RiskLevel::Medium),
    cap("service-versions", "service versions", RiskLevel::Medium),
    cap("topology", "topology notes", RiskLevel::Low),
];
const CAPS_WEB: &[ModuleCapability] = &[
    cap("http-analysis", "HTTP analysis", RiskLevel::Medium),
    cap("security-headers", "security headers", RiskLevel::Low),
    cap("tls-config", "TLS configuration", RiskLevel::Low),
];
const CAPS_VULN: &[ModuleCapability] = &[
    cap("cve-correlation", "CVE correlation", RiskLevel::Low),
    cap("severity", "severity classification", RiskLevel::Low),
    cap("validation", "finding validation", RiskLevel::Medium),
];
const CAPS_FORENSICS: &[ModuleCapability] = &[
    cap("hash", "SHA-256 hashing", RiskLevel::Informational),
    cap("log-analysis", "log analysis", RiskLevel::Low),
    cap("timeline", "timeline analysis", RiskLevel::Low),
];
const CAPS_OSINT: &[ModuleCapability] = &[
    cap("public-records", "public records", RiskLevel::Low),
    cap("footprint", "org footprint", RiskLevel::Low),
];
const CAPS_THREAT_INTEL: &[ModuleCapability] = &[
    cap("ioc", "IOC analysis", RiskLevel::Low),
    cap("reputation", "reputation lookup", RiskLevel::Medium),
];
const CAPS_PASSWORD: &[ModuleCapability] = &[
    cap("policy", "policy inspection", RiskLevel::Low),
    cap("hash-id", "hash identification", RiskLevel::Low),
];
const CAPS_WIRELESS: &[ModuleCapability] = &[
    cap("encryption", "encryption config", RiskLevel::Medium),
    cap("ap-inventory", "AP inventory", RiskLevel::Medium),
];
const CAPS_EXPLOITATION: &[ModuleCapability] =
    &[cap("lab-validation", "lab validation", RiskLevel::Critical)];
const CAPS_POST_EXPLOITATION: &[ModuleCapability] =
    &[cap("impact", "impact documentation", RiskLevel::Critical)];
const CAPS_REPORTING: &[ModuleCapability] = &[
    cap("summary", "assessment summary", RiskLevel::Informational),
    cap("export", "markdown export", RiskLevel::Informational),
];

/// The built-in module set. Four are real in Phase 2; the rest are catalogued
/// here so the assessment can permit them by id and the UI can list them.
pub fn builtin_modules() -> Vec<Box<dyn HackerModule>> {
    vec![
        Box::new(DescriptorModule {
            id: "recon",
            name: "Reconnaissance",
            description: "Passive-first discovery of domains, hosts and public attack surface.",
            category: ModuleCategory::Recon,
            capabilities: CAPS_RECON,
        }),
        Box::new(DescriptorModule {
            id: "network",
            name: "Network Security",
            description: "Authorized port/service inventory and exposure assessment.",
            category: ModuleCategory::Network,
            capabilities: CAPS_NETWORK,
        }),
        Box::new(DescriptorModule {
            id: "web",
            name: "Web Security",
            description: "HTTP/TLS/header and cookie configuration analysis.",
            category: ModuleCategory::Web,
            capabilities: CAPS_WEB,
        }),
        Box::new(DescriptorModule {
            id: "vuln",
            name: "Vulnerability Analysis",
            description: "CVE correlation and severity classification with evidence.",
            category: ModuleCategory::Vulnerability,
            capabilities: CAPS_VULN,
        }),
        Box::new(DescriptorModule {
            id: "forensics",
            name: "Digital Forensics",
            description: "File metadata, hashing, log and timeline analysis; read-only.",
            category: ModuleCategory::Forensics,
            capabilities: CAPS_FORENSICS,
        }),
        Box::new(DescriptorModule {
            id: "osint",
            name: "OSINT",
            description: "Public-source intelligence gathering and correlation.",
            category: ModuleCategory::Osint,
            capabilities: CAPS_OSINT,
        }),
        Box::new(DescriptorModule {
            id: "threat-intel",
            name: "Threat Intelligence",
            description: "IOC analysis and reputation correlation with confidence scoring.",
            category: ModuleCategory::ThreatIntel,
            capabilities: CAPS_THREAT_INTEL,
        }),
        Box::new(DescriptorModule {
            id: "password",
            name: "Password Auditing",
            description: "Offline hash-strength auditing for authorized datasets.",
            category: ModuleCategory::Password,
            capabilities: CAPS_PASSWORD,
        }),
        Box::new(DescriptorModule {
            id: "wireless",
            name: "Wireless Security",
            description: "Wireless configuration and encryption assessment (authorized).",
            category: ModuleCategory::Wireless,
            capabilities: CAPS_WIRELESS,
        }),
        Box::new(DescriptorModule {
            id: "exploitation",
            name: "Exploitation",
            description: "Controlled validation inside explicitly authorized lab targets.",
            category: ModuleCategory::Exploitation,
            capabilities: CAPS_EXPLOITATION,
        }),
        Box::new(DescriptorModule {
            id: "post-exploitation",
            name: "Post-Exploitation",
            description: "Isolated-lab impact assessment; no persistence or exfiltration.",
            category: ModuleCategory::PostExploitation,
            capabilities: CAPS_POST_EXPLOITATION,
        }),
        Box::new(DescriptorModule {
            id: "reporting",
            name: "Reporting",
            description: "Assessment summaries, findings and markdown report export.",
            category: ModuleCategory::Reporting,
            capabilities: CAPS_REPORTING,
        }),
    ]
}

/// The registry the app holds: own the module boxes and look them up by id.
pub struct Registry {
    modules: Vec<Box<dyn HackerModule>>,
}

impl Registry {
    pub fn builtin() -> Self {
        Self {
            modules: builtin_modules(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn HackerModule> {
        self.modules.iter().map(|m| m.as_ref())
    }

    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&dyn HackerModule> {
        self.modules
            .iter()
            .map(|m| m.as_ref())
            .find(|m| m.id().eq_ignore_ascii_case(id))
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn builtin_registry_has_all_twelve_modules_with_unique_ids() {
        let reg = Registry::builtin();
        assert_eq!(reg.len(), 12);
        let ids: HashSet<_> = reg.iter().map(|m| m.id()).collect();
        assert_eq!(ids.len(), 12, "module ids must be unique");
        for id in [
            "recon",
            "network",
            "web",
            "vuln",
            "forensics",
            "osint",
            "threat-intel",
            "password",
            "wireless",
            "exploitation",
            "post-exploitation",
            "reporting",
        ] {
            assert!(reg.get(id).is_some(), "missing module {id}");
        }
    }

    #[test]
    fn every_module_declares_capabilities() {
        for m in Registry::builtin().iter() {
            assert!(!m.name().is_empty(), "{} has no name", m.id());
            assert!(!m.description().is_empty(), "{} has no description", m.id());
            assert!(
                !m.capabilities().is_empty(),
                "{} declares no capabilities",
                m.id()
            );
        }
    }

    #[test]
    fn lookup_is_case_insensitive_and_unknown_is_none() {
        let reg = Registry::builtin();
        assert!(reg.get("RECON").is_some());
        assert!(reg.get("nope").is_none());
    }
}
