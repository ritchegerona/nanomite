//! HACKER MODE: a cybersecurity operations environment layered on top of the
//! existing nanomite agent runtime.
//!
//! This is **not** `ToolPolicy::Hacker` (a general tool-permission level). It
//! is a separate, opt-in environment that adds an independent authorization
//! layer, an assessment scope, an append-only audit trail, and a module
//! registry. It reuses the provider, runtime, tool dispatch, TUI and
//! persistence infrastructure unchanged.
//!
//! Phase 1 is the foundation: typed assessment context, scope validation,
//! authorization, audit, a descriptor module registry, `/hacker` commands and
//! a read-only workspace overlay. No active scanning yet.

pub mod audit;
pub mod authorization;
pub mod capability;
pub mod context;
pub mod coordinator;
pub mod findings;
pub mod modules;
pub mod registry;
pub mod risk;
pub mod scope;
pub mod store;
pub mod typestate;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub use audit::{hacker_dir, AuditLog, AuditRecord};
pub use authorization::{authorize, Decision, Operation};
pub use context::{AssessmentContext, AssessmentStatus};
pub use findings::{Confidence, Finding, FindingStatus};
pub use registry::{HackerModule, ModuleCategory, Registry};
pub use risk::{OperationCategory, RiskLevel};
pub use scope::ScopeMatch;
pub use store::{
    EvidenceCheck, FindingSink, FindingsBackend, FindingsStore, NullSink, ReportFormat,
    ReportSummary, StoredAssessmentSpec,
};

/// The live HACKER MODE session held by the app.
///
/// The assessment is built from configured scope at activation time - never
/// from model output - so "authorized" always means "the user configured it".
pub struct HackerSession {
    /// Whether HACKER MODE is enabled in configuration at all.
    pub enabled: bool,
    /// Whether the environment is currently entered (`/hacker` vs `/hacker exit`).
    pub active: bool,
    /// The current assessment, when one was successfully activated.
    pub assessment: Option<AssessmentContext>,
    pub audit: AuditLog,
    pub registry: Registry,
    /// Configured scope used to build an assessment at activation.
    scope_specs: Vec<String>,
    /// Assessment id/name for the configured scope.
    assessment_id: String,
    assessment_name: String,
    /// How long an activated assessment stays valid.
    session_minutes: u64,
    /// Owner decision: auto-approve risky tool calls while an *authorized*
    /// assessment is active. Gated on a valid, unexpired, in-scope assessment
    /// (see [`Self::auto_approve_active`]); never unconditional.
    auto_approve: bool,
    /// Shared with the UI confirmer: true exactly while auto-approve applies.
    bypass: Arc<AtomicBool>,
}

impl HackerSession {
    /// Builds a session from configuration. `scope_specs` is a list of CIDRs
    /// and/or domain names (the user-provided authorized scope).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        enabled: bool,
        scope_specs: Vec<String>,
        assessment_id: String,
        assessment_name: String,
        session_minutes: u64,
        auto_approve: bool,
        audit_enabled: bool,
    ) -> Self {
        let audit = if audit_enabled {
            AuditLog::open(&hacker_dir())
        } else {
            AuditLog::disabled()
        };
        Self {
            enabled,
            active: false,
            assessment: None,
            audit,
            registry: Registry::builtin(),
            scope_specs,
            assessment_id,
            assessment_name,
            session_minutes,
            auto_approve,
            bypass: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The flag the UI confirmer consults to skip the approval prompt.
    pub fn bypass_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.bypass)
    }

    /// Public accessor for the assessment id, used by the typestate wrapper
    /// for audit labeling without exposing the private field.
    pub fn assessment_id_dbg(&self) -> &str {
        &self.assessment_id
    }

    /// Public accessor for the configured session duration in minutes.
    pub fn session_minutes(&self) -> u64 {
        self.session_minutes
    }

    /// Enters HACKER MODE. Requires the feature enabled and non-empty scope;
    /// builds and validates the assessment. Returns a human error on refusal.
    pub fn activate(&mut self, now: SystemTime) -> Result<(), String> {
        if !self.enabled {
            return Err("HACKER MODE is disabled - set `hacker_enabled = true`".into());
        }
        if self.scope_specs.is_empty() {
            return Err(
                "no authorized scope configured - set `hacker_scope` to CIDRs/domains".into(),
            );
        }
        let ctx = self.build_assessment(now)?;
        ctx.validate().map_err(|e| e.join("; "))?;
        self.assessment = Some(ctx);
        self.active = true;
        self.sync_bypass(now);
        self.audit.append(
            &AuditRecord::new("allowed", "ok")
                .with_assessment(self.assessment_id.clone())
                .with_operation("hacker-mode-activate")
                .with_detail(format!("scope: {}", self.scope_specs.join(", "))),
        );
        Ok(())
    }

    /// Resumes a persisted assessment, preserving its original window.
    ///
    /// Refuses when the feature is disabled, the assessment is not authorized,
    /// its window has expired (or has not started), or it has no recorded
    /// scope. Resuming never widens scope: it adopts exactly what was stored.
    pub fn resume(
        &mut self,
        spec: &store::StoredAssessmentSpec,
        now: SystemTime,
    ) -> Result<(), String> {
        if !self.enabled {
            return Err("HACKER MODE is disabled - set `hacker_enabled = true`".into());
        }
        if spec.cidrs.is_empty() && spec.domains.is_empty() {
            return Err(format!(
                "assessment `{}` has no recorded scope - cannot resume",
                spec.id
            ));
        }
        if !spec
            .status
            .eq_ignore_ascii_case(AssessmentStatus::Authorized.label())
        {
            return Err(format!(
                "assessment `{}` is {} - not resumable",
                spec.id, spec.status
            ));
        }
        if now >= spec.expiry {
            return Err(format!(
                "assessment `{}` window already expired - raise a new one",
                spec.id
            ));
        }
        if now < spec.start {
            return Err(format!("assessment `{}` is not valid yet", spec.id));
        }

        let mut ctx = AssessmentContext::new(spec.id.clone(), spec.name.clone());
        ctx.allowed_cidrs = spec.cidrs.clone();
        ctx.allowed_domains = spec.domains.clone();
        ctx.permitted_modules = self.registry.iter().map(|m| m.id().to_string()).collect();
        ctx.permitted_ops = vec![
            OperationCategory::Passive,
            OperationCategory::ActiveDiscovery,
            OperationCategory::Authenticated,
            OperationCategory::Intrusive,
        ];
        ctx.authorization_ref = spec
            .authorization_ref
            .clone()
            .or_else(|| Some(format!("resumed ({})", spec.id)));
        ctx.status = AssessmentStatus::Authorized;
        ctx.start = spec.start;
        ctx.expiry = spec.expiry;
        ctx.validate().map_err(|e| e.join("; "))?;

        // Keep the config-derived fields in step, so a later activate() rebuilds
        // the same scope rather than the old config.
        self.scope_specs = spec
            .cidrs
            .iter()
            .chain(spec.domains.iter())
            .cloned()
            .collect();
        self.assessment_id = spec.id.clone();
        self.assessment_name = spec.name.clone();
        self.assessment = Some(ctx);
        self.active = true;
        self.sync_bypass(now);
        self.audit.append(
            &AuditRecord::new("allowed", "ok")
                .with_assessment(spec.id.clone())
                .with_operation("hacker-mode-resume")
                .with_detail(format!("scope: {}", self.scope_specs.join(", "))),
        );
        Ok(())
    }

    /// Leaves HACKER MODE and clears the assessment and the bypass.
    pub fn deactivate(&mut self) {
        if self.active {
            self.audit.append(
                &AuditRecord::new("allowed", "ok").with_operation("hacker-mode-deactivate"),
            );
        }
        self.active = false;
        self.assessment = None;
        self.bypass.store(false, Ordering::SeqCst);
    }

    fn build_assessment(&self, now: SystemTime) -> Result<AssessmentContext, String> {
        let mut ctx =
            AssessmentContext::new(self.assessment_id.clone(), self.assessment_name.clone());
        for spec in &self.scope_specs {
            let spec = spec.trim();
            if spec.is_empty() {
                continue;
            }
            if scope::normalize_host(spec)
                .and_then(|h| h.parse::<std::net::IpAddr>().ok())
                .is_some()
                || spec.contains('/')
            {
                ctx.allowed_cidrs.push(spec.to_string());
            } else {
                ctx.allowed_domains.push(spec.to_string());
            }
        }
        // Every module is permitted by the manager assessment; an operation is
        // still gated by category and by scope at authorize() time.
        ctx.permitted_modules = self.registry.iter().map(|m| m.id().to_string()).collect();
        ctx.permitted_ops = vec![
            OperationCategory::Passive,
            OperationCategory::ActiveDiscovery,
            OperationCategory::Authenticated,
            OperationCategory::Intrusive,
        ];
        ctx.authorization_ref = Some(format!("configured scope ({})", self.scope_specs.join(",")));
        ctx.status = AssessmentStatus::Authorized;
        ctx.start = now;
        ctx.expiry = now + Duration::from_secs(self.session_minutes.max(1) * 60);
        Ok(ctx)
    }

    /// True while the owner's auto-approve applies: environment active AND a
    /// valid, authorized, unexpired assessment is loaded.
    pub fn auto_approve_active(&self, now: SystemTime) -> bool {
        self.auto_approve
            && self.active
            && self
                .assessment
                .as_ref()
                .is_some_and(|ctx| ctx.validate().is_ok() && ctx.is_active(now))
    }

    /// Recomputes the shared bypass flag from the current state.
    pub fn sync_bypass(&self, now: SystemTime) {
        self.bypass
            .store(self.auto_approve_active(now), Ordering::SeqCst);
    }

    /// A one-line status for the workspace header / `/hacker status`.
    pub fn status_line(&self, now: SystemTime) -> String {
        if !self.enabled {
            return "hacker mode: disabled".into();
        }
        if !self.active {
            return "hacker mode: off".into();
        }
        match &self.assessment {
            Some(ctx) if ctx.is_active(now) => format!(
                "hacker mode: active · {} · {}s left · auto-approve {}",
                ctx.id,
                ctx.seconds_remaining(now),
                if self.auto_approve { "on" } else { "off" }
            ),
            _ => "hacker mode: active · assessment incongruent".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(scope: Vec<&str>, enabled: bool) -> HackerSession {
        HackerSession::new(
            enabled,
            scope.into_iter().map(|s| s.to_string()).collect(),
            "asm-1".into(),
            "Local test".into(),
            60,
            true,
            false, // no audit file in tests
        )
    }

    #[test]
    fn disabled_mode_cannot_activate() {
        let mut s = session(vec!["10.0.0.0/8"], false);
        assert!(s.activate(SystemTime::now()).is_err());
        assert!(!s.active);
    }

    #[test]
    fn empty_scope_cannot_activate() {
        let mut s = session(vec![], true);
        assert!(s.activate(SystemTime::now()).is_err());
    }

    #[test]
    fn activating_populates_scope_and_enables_gated_auto_approve() {
        let mut s = session(vec!["10.0.0.0/8", "example.com"], true);
        let now = SystemTime::now();
        s.activate(now).expect("activates");
        assert!(s.active);
        let ctx = s.assessment.as_ref().unwrap();
        assert_eq!(ctx.allowed_cidrs, vec!["10.0.0.0/8".to_string()]);
        assert_eq!(ctx.allowed_domains, vec!["example.com".to_string()]);
        assert!(s.auto_approve_active(now));
    }

    #[test]
    fn auto_approve_turns_off_when_assessment_expires_or_deactivated() {
        let mut s = session(vec!["10.0.0.0/8"], true);
        let now = SystemTime::now();
        s.activate(now).unwrap();
        // Far past the 60-minute window.
        let later = now + Duration::from_secs(61 * 60);
        assert!(!s.auto_approve_active(later));

        s.deactivate();
        assert!(!s.auto_approve_active(now));
    }

    #[test]
    fn bypass_flag_tracks_auto_approve() {
        let mut s = session(vec!["10.0.0.0/8"], true);
        let flag = s.bypass_flag();
        assert!(!flag.load(Ordering::SeqCst));
        s.activate(SystemTime::now()).unwrap();
        assert!(flag.load(Ordering::SeqCst));
        s.deactivate();
        assert!(!flag.load(Ordering::SeqCst));
    }

    fn spec(
        status: &str,
        start: SystemTime,
        expiry: SystemTime,
        cidrs: Vec<&str>,
        domains: Vec<&str>,
    ) -> StoredAssessmentSpec {
        StoredAssessmentSpec {
            id: "asm-stored".into(),
            name: "Stored engagement".into(),
            authorization_ref: Some("TICKET-9".into()),
            status: status.into(),
            start,
            expiry,
            cidrs: cidrs.into_iter().map(str::to_string).collect(),
            domains: domains.into_iter().map(str::to_string).collect(),
        }
    }

    #[test]
    fn resume_adopts_the_stored_scope_and_window() {
        let now = SystemTime::now();
        let mut s = session(vec![], true);
        let stored = spec(
            "authorized",
            now - Duration::from_secs(60),
            now + Duration::from_secs(3600),
            vec!["10.0.0.0/8"],
            vec!["example.com"],
        );
        s.resume(&stored, now).expect("resumes");
        assert!(s.active);
        let ctx = s.assessment.as_ref().unwrap();
        assert_eq!(ctx.id, "asm-stored");
        assert_eq!(ctx.allowed_cidrs, vec!["10.0.0.0/8".to_string()]);
        assert_eq!(ctx.allowed_domains, vec!["example.com".to_string()]);
        assert_eq!(ctx.authorization_ref.as_deref(), Some("TICKET-9"));
        // The stored window is preserved, not renewed.
        assert!(ctx.expiry <= now + Duration::from_secs(3600));
        assert!(s.auto_approve_active(now));
    }

    #[test]
    fn resume_refuses_disabled_empty_completed_expired_and_future() {
        let now = SystemTime::now();
        let good = spec(
            "authorized",
            now - Duration::from_secs(1),
            now + Duration::from_secs(10),
            vec!["10.0.0.0/8"],
            vec![],
        );

        let mut off = session(vec!["10.0.0.0/8"], false);
        assert!(off.resume(&good, now).is_err(), "disabled must refuse");

        let mut s = session(vec![], true);
        let no_scope = spec(
            "authorized",
            now - Duration::from_secs(1),
            now + Duration::from_secs(10),
            vec![],
            vec![],
        );
        assert!(s
            .resume(&no_scope, now)
            .unwrap_err()
            .contains("no recorded scope"));

        let completed = spec(
            "completed",
            now - Duration::from_secs(1),
            now + Duration::from_secs(10),
            vec!["10.0.0.0/8"],
            vec![],
        );
        assert!(s
            .resume(&completed, now)
            .unwrap_err()
            .contains("not resumable"));

        let expired = spec(
            "authorized",
            now - Duration::from_secs(100),
            now - Duration::from_secs(1),
            vec!["10.0.0.0/8"],
            vec![],
        );
        assert!(s.resume(&expired, now).unwrap_err().contains("expired"));

        let future = spec(
            "authorized",
            now + Duration::from_secs(60),
            now + Duration::from_secs(120),
            vec!["10.0.0.0/8"],
            vec![],
        );
        assert!(s
            .resume(&future, now)
            .unwrap_err()
            .contains("not valid yet"));
        assert!(!s.active);
    }

    #[test]
    fn resume_keeps_later_activation_on_the_same_scope() {
        let now = SystemTime::now();
        let mut s = session(vec!["192.168.0.0/16"], true);
        let stored = spec(
            "authorized",
            now - Duration::from_secs(1),
            now + Duration::from_secs(600),
            vec!["10.0.0.0/8"],
            vec!["example.com"],
        );
        s.resume(&stored, now).unwrap();
        // Re-activating (e.g. after exit) rebuilds from the resumed config.
        s.deactivate();
        s.activate(now).unwrap();
        let ctx = s.assessment.as_ref().unwrap();
        assert_eq!(ctx.allowed_cidrs, vec!["10.0.0.0/8".to_string()]);
        assert_eq!(ctx.allowed_domains, vec!["example.com".to_string()]);
    }

    #[test]
    fn scope_specs_split_into_cidrs_and_domains() {
        let mut s = session(vec!["10.0.0.0/8", "192.168.1.5", "example.com"], true);
        s.activate(SystemTime::now()).unwrap();
        let ctx = s.assessment.as_ref().unwrap();
        assert_eq!(ctx.allowed_cidrs.len(), 2);
        assert_eq!(ctx.allowed_domains, vec!["example.com".to_string()]);
    }
}
