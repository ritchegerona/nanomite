//! Typed assessment context: what a HACKER MODE engagement is allowed to do,
//! against which targets, for how long, and under whose authority.
//!
//! The context is the *only* source of scope. Model output can never create or
//! widen it: it is built from configuration or from an explicit user action
//! (see `HackerSession`), never from anything the model generates.

use std::time::SystemTime;

use super::risk::OperationCategory;
use super::scope::{parse_cidr, IpNet};

/// Lifecycle state of an assessment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssessmentStatus {
    Draft,
    Authorized,
    Completed,
    Revoked,
}

impl AssessmentStatus {
    pub fn label(self) -> &'static str {
        match self {
            AssessmentStatus::Draft => "draft",
            AssessmentStatus::Authorized => "authorized",
            AssessmentStatus::Completed => "completed",
            AssessmentStatus::Revoked => "revoked",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "draft" => Some(AssessmentStatus::Draft),
            "authorized" => Some(AssessmentStatus::Authorized),
            "completed" | "complete" => Some(AssessmentStatus::Completed),
            "revoked" => Some(AssessmentStatus::Revoked),
            _ => None,
        }
    }
}

/// A single authorized target: an IP literal, a CIDR, or a domain name. Kept
/// as a raw string plus a parsed form so an invalid entry is a validation
/// error rather than a silent no-op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSpec {
    pub raw: String,
}

impl TargetSpec {
    pub fn new(raw: impl Into<String>) -> Self {
        Self { raw: raw.into() }
    }
}

/// Everything the authorization layer needs to judge an operation.
#[derive(Debug, Clone)]
pub struct AssessmentContext {
    pub id: String,
    pub name: String,
    /// Allowed CIDRs / IP literals (parsed lazily by [`Self::allowed_nets`]).
    pub allowed_cidrs: Vec<String>,
    /// Allowed domain suffixes (exact host or subdomain).
    pub allowed_domains: Vec<String>,
    /// Module ids this assessment may run; empty means "none permitted".
    pub permitted_modules: Vec<String>,
    /// Operation categories this assessment may run.
    pub permitted_ops: Vec<OperationCategory>,
    pub start: SystemTime,
    pub expiry: SystemTime,
    /// Free-form reference to the signed authorization (ticket id, SOW).
    pub authorization_ref: Option<String>,
    pub status: AssessmentStatus,
}

impl AssessmentContext {
    /// A draft assessment with no scope and no permitted modules.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        let now = SystemTime::now();
        Self {
            id: id.into(),
            name: name.into(),
            allowed_cidrs: Vec::new(),
            allowed_domains: Vec::new(),
            permitted_modules: Vec::new(),
            permitted_ops: Vec::new(),
            start: now,
            expiry: now,
            authorization_ref: None,
            status: AssessmentStatus::Draft,
        }
    }

    /// Validates the context's scope entries. Returns every problem found.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.id.trim().is_empty() {
            errors.push("assessment id is empty".into());
        }
        if self.name.trim().is_empty() {
            errors.push("assessment name is empty".into());
        }
        for cidr in &self.allowed_cidrs {
            if let Err(e) = parse_cidr(cidr) {
                errors.push(format!("allowed_cidrs: {e}"));
            }
        }
        for domain in &self.allowed_domains {
            if super::scope::normalize_host(domain).is_none() {
                errors.push(format!("allowed_domains: invalid domain `{domain}`"));
            }
        }
        if self.expiry <= self.start {
            errors.push("expiry must be after start".into());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Parsed allowed networks; unparsable entries are skipped (validation
    /// surfaces them separately).
    pub fn allowed_nets(&self) -> Vec<IpNet> {
        self.allowed_cidrs
            .iter()
            .filter_map(|c| parse_cidr(c).ok())
            .collect()
    }

    /// Whether the assessment is authorized and within its window at `now`.
    pub fn is_active(&self, now: SystemTime) -> bool {
        self.status == AssessmentStatus::Authorized && now >= self.start && now < self.expiry
    }

    pub fn module_permitted(&self, module_id: &str) -> bool {
        self.permitted_modules
            .iter()
            .any(|m| m.eq_ignore_ascii_case(module_id))
    }

    pub fn op_permitted(&self, category: OperationCategory) -> bool {
        self.permitted_ops.contains(&category)
    }

    /// Seconds until expiry (0 when already expired). Negative durations clamp
    /// to 0.
    pub fn seconds_remaining(&self, now: SystemTime) -> u64 {
        self.expiry
            .duration_since(now)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn authorized() -> AssessmentContext {
        let mut ctx = AssessmentContext::new("asm-1", "Internal network");
        ctx.allowed_cidrs = vec!["10.0.0.0/8".into()];
        ctx.allowed_domains = vec!["example.com".into()];
        ctx.permitted_modules = vec!["recon".into()];
        ctx.permitted_ops = vec![OperationCategory::Passive];
        ctx.status = AssessmentStatus::Authorized;
        ctx.start = SystemTime::now();
        ctx.expiry = ctx.start + Duration::from_secs(3600);
        ctx
    }

    #[test]
    fn valid_context_passes_validation() {
        assert!(authorized().validate().is_ok());
    }

    #[test]
    fn invalid_scope_entries_are_reported() {
        let mut ctx = authorized();
        ctx.allowed_cidrs = vec!["not-a-cidr".into()];
        ctx.allowed_domains = vec!["  ".into()];
        let errs = ctx.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("allowed_cidrs")));
        assert!(errs.iter().any(|e| e.contains("allowed_domains")));
    }

    #[test]
    fn only_authorized_and_unexpired_is_active() {
        let mut ctx = authorized();
        let now = ctx.start + Duration::from_secs(10);
        assert!(ctx.is_active(now));

        ctx.status = AssessmentStatus::Draft;
        assert!(!ctx.is_active(now));

        ctx.status = AssessmentStatus::Authorized;
        let after = ctx.expiry + Duration::from_secs(1);
        assert!(!ctx.is_active(after));
    }

    #[test]
    fn expiry_must_be_after_start() {
        let mut ctx = authorized();
        ctx.expiry = ctx.start - Duration::from_secs(1);
        assert!(ctx
            .validate()
            .unwrap_err()
            .iter()
            .any(|e| e.contains("expiry")));
    }

    #[test]
    fn module_and_op_membership_is_case_insensitive_for_modules() {
        let ctx = authorized();
        assert!(ctx.module_permitted("RECON"));
        assert!(!ctx.module_permitted("web"));
        assert!(ctx.op_permitted(OperationCategory::Passive));
        assert!(!ctx.op_permitted(OperationCategory::Intrusive));
    }
}
