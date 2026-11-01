//! Risk model for HACKER MODE operations.
//!
//! A separate scale from [`crate::agent::policy::Risk`], which classifies a
//! *tool call* for the general agent policy. This one classifies a
//! cybersecurity *operation* inside an assessment, and drives whether the
//! authorization layer permits it, needs explicit approval, or refuses it.

/// Severity/impact of a security operation, ascending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RiskLevel {
    /// Read-only local work: parsing metadata, reading evidence.
    Informational,
    /// Passive intelligence gathering that touches no target directly.
    Low,
    /// Authorized active discovery against an in-scope target.
    Medium,
    /// Authenticated or otherwise sensitive assessment work.
    High,
    /// Potentially disruptive work, lab-only in Phase 1.
    Critical,
}

impl RiskLevel {
    pub fn label(self) -> &'static str {
        match self {
            RiskLevel::Informational => "informational",
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
            RiskLevel::Critical => "critical",
        }
    }

    /// Parses a manifest/config value; unknown values return `None` so a
    /// typo never silently downgrades to a safer-looking level.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "informational" | "info" => Some(RiskLevel::Informational),
            "low" => Some(RiskLevel::Low),
            "medium" | "med" => Some(RiskLevel::Medium),
            "high" => Some(RiskLevel::High),
            "critical" | "crit" => Some(RiskLevel::Critical),
            _ => None,
        }
    }

    /// `High` and above need a recorded authorization reference and are the
    /// levels a lab-only gate should keep behind explicit approval.
    pub fn is_sensitive(self) -> bool {
        self >= RiskLevel::High
    }
}

/// The kind of work an operation performs. The category sets a floor on the
/// risk level, so a module that forgets to declare its own risk still lands
/// somewhere honest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationCategory {
    /// Local, read-only analysis (metadata, logs, hashes).
    Passive,
    /// Active discovery against an in-scope target (port/service checks).
    ActiveDiscovery,
    /// Authenticated assessment (requires credentials/approval).
    Authenticated,
    /// Potentially disruptive validation, lab-only.
    Intrusive,
}

impl OperationCategory {
    pub fn label(self) -> &'static str {
        match self {
            OperationCategory::Passive => "passive",
            OperationCategory::ActiveDiscovery => "active-discovery",
            OperationCategory::Authenticated => "authenticated",
            OperationCategory::Intrusive => "intrusive",
        }
    }

    /// Parses a config/manifest value.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "passive" => Some(OperationCategory::Passive),
            "active" | "active-discovery" | "active_discovery" => {
                Some(OperationCategory::ActiveDiscovery)
            }
            "authenticated" | "auth" => Some(OperationCategory::Authenticated),
            "intrusive" => Some(OperationCategory::Intrusive),
            _ => None,
        }
    }

    /// Floor risk level for this category.
    pub fn default_risk(self) -> RiskLevel {
        match self {
            OperationCategory::Passive => RiskLevel::Low,
            OperationCategory::ActiveDiscovery => RiskLevel::Medium,
            OperationCategory::Authenticated => RiskLevel::High,
            OperationCategory::Intrusive => RiskLevel::Critical,
        }
    }
}

/// The risk of one concrete operation: `max(category floor, declared level)`.
pub fn effective_risk(category: OperationCategory, declared: Option<RiskLevel>) -> RiskLevel {
    match declared {
        Some(level) => level.max(category.default_risk()),
        None => category.default_risk(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_ordered_and_parse_round_trips() {
        assert!(RiskLevel::Informational < RiskLevel::Low);
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);
        for level in [
            RiskLevel::Informational,
            RiskLevel::Low,
            RiskLevel::Medium,
            RiskLevel::High,
            RiskLevel::Critical,
        ] {
            assert_eq!(RiskLevel::parse(level.label()), Some(level));
        }
        assert_eq!(RiskLevel::parse("nope"), None);
    }

    #[test]
    fn sensitive_levels_are_high_and_above() {
        assert!(!RiskLevel::Medium.is_sensitive());
        assert!(RiskLevel::High.is_sensitive());
        assert!(RiskLevel::Critical.is_sensitive());
    }

    #[test]
    fn category_sets_a_floor_so_risk_cannot_be_understated() {
        // Declaring "informational" for an intrusive op is not allowed to win.
        assert_eq!(
            effective_risk(OperationCategory::Intrusive, Some(RiskLevel::Informational)),
            RiskLevel::Critical
        );
        // A stricter declaration is honored.
        assert_eq!(
            effective_risk(OperationCategory::ActiveDiscovery, Some(RiskLevel::High)),
            RiskLevel::High
        );
        assert_eq!(
            effective_risk(OperationCategory::Passive, None),
            RiskLevel::Low
        );
    }
}
