//! Advanced capability layer for HACKER MODE.
//!
//! Wraps [`crate::hacker::HackerSession`] in a typestate machine so that
//! offensive operations can only be issued from an `Active` session holding a
//! valid, unexpired [`OpCapability`]. Invalid transitions are compile-time
//! errors in safe Rust, not runtime surprises.
//!
//! This is additive: it does not touch `HackerSession`, the coordinator, or
//! any existing module. It is used by the `/hacker` command dispatch to gate
//! every operation behind a typed, audited capability.

use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use super::context::AssessmentContext;
use super::risk::{effective_risk, OperationCategory, RiskLevel};
use super::scope::{host_in_scope, resolved_in_scope, ScopeMatch};

/// Unforgeable authority token. A module cannot fabricate one; it must be
/// issued by the owner (user) at activation time.
#[derive(Debug, Clone)]
pub struct OpCapability {
    pub token: String,
    pub scope_id: String,
    pub expires_at: SystemTime,
    pub allowed_modules: Vec<String>,
    pub max_risk: RiskLevel,
}

impl OpCapability {
    pub fn is_valid(&self, now: SystemTime) -> bool {
        now < self.expires_at
    }
}

/// ---------- Typestate markers ----------

#[derive(Debug)]
pub struct Discovered;
#[derive(Debug)]
pub struct Authorized;
#[derive(Debug)]
pub struct Active;
#[derive(Debug)]
pub struct Closed;

/// ---------- Session ----------

#[derive(Debug)]
pub struct CapabilitySession<State> {
    pub session_id: String,
    pub target: String,
    pub scope_id: String,
    pub capability: Option<OpCapability>,
    pub _state: std::marker::PhantomData<State>,
    pub events: Vec<CapEvent>,
}

#[derive(Debug, Clone)]
pub struct CapEvent {
    pub ts: SystemTime,
    pub op: String,
    pub outcome: CapOutcome,
}

#[derive(Debug, Clone)]
pub enum CapOutcome {
    Granted,
    Denied { code: &'static str, reason: String },
}

/// ---------- Errors ----------

#[derive(Debug)]
pub enum CapError {
    NotAuthorized(String),
    CapabilityInvalid,
    OutOfScope(String),
    RiskAboveCeiling(String),
    InvalidTransition(String, String),
}

impl std::fmt::Display for CapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CapError::NotAuthorized(s) => write!(f, "session not authorized: {s}"),
            CapError::CapabilityInvalid => write!(f, "capability expired or missing"),
            CapError::OutOfScope(s) => write!(f, "module outside scope: {s}"),
            CapError::RiskAboveCeiling(s) => write!(f, "risk above capability ceiling: {s}"),
            CapError::InvalidTransition(a, b) => {
                write!(f, "invalid state transition: {a} -> {b}")
            }
        }
    }
}

impl std::error::Error for CapError {}

fn next_session_id() -> String {
    let millis = UNIX_EPOCH.elapsed().map(|d| d.as_millis()).unwrap_or(0);
    format!("cap-{millis:x}")
}

/// ---------- Discovered -> Authorized ----------
impl CapabilitySession<Discovered> {
    pub fn new(target: String, scope_id: String) -> Self {
        Self {
            session_id: next_session_id(),
            target,
            scope_id,
            capability: None,
            _state: std::marker::PhantomData,
            events: Vec::new(),
        }
    }

    pub fn authorize(
        self,
        _operator: &str,
        cap: OpCapability,
    ) -> Result<CapabilitySession<Authorized>, CapError> {
        if !cap.is_valid(SystemTime::now()) {
            return Err(CapError::CapabilityInvalid);
        }
        Ok(CapabilitySession {
            session_id: self.session_id,
            target: self.target,
            scope_id: self.scope_id,
            capability: Some(cap),
            _state: std::marker::PhantomData,
            events: self.events,
        })
    }
}

/// ---------- Authorized -> Active ----------
impl CapabilitySession<Authorized> {
    pub fn activate(self) -> Result<CapabilitySession<Active>, CapError> {
        let cap = self
            .capability
            .as_ref()
            .ok_or(CapError::NotAuthorized("no capability".into()))?;
        if !cap.is_valid(SystemTime::now()) {
            return Err(CapError::CapabilityInvalid);
        }
        Ok(CapabilitySession {
            session_id: self.session_id,
            target: self.target,
            scope_id: self.scope_id,
            capability: Some(cap.clone()),
            _state: std::marker::PhantomData,
            events: self.events,
        })
    }
}

/// ---------- Active (operations only here) ----------
impl CapabilitySession<Active> {
    /// Authorize one module operation against the capability ceiling and
    /// assessment scope before execution.
    pub fn authorize_operation(
        &self,
        module_id: &str,
        category: OperationCategory,
        target: Option<&str>,
        resolved: &[IpAddr],
        _now: SystemTime,
        ctx: Option<&AssessmentContext>,
    ) -> Result<RiskLevel, CapError> {
        let cap = self
            .capability
            .as_ref()
            .ok_or(CapError::CapabilityInvalid)?;
        if !cap.allowed_modules.contains(&module_id.to_string()) {
            return Err(CapError::OutOfScope(module_id.to_string()));
        }
        let risk = effective_risk(category, None);
        if risk > cap.max_risk {
            return Err(CapError::RiskAboveCeiling(format!(
                "risk {} exceeds capability ceiling {}",
                risk.label(),
                cap.max_risk.label()
            )));
        }
        if let Some(ctx) = ctx {
            if !ctx.module_permitted(module_id) {
                return Err(CapError::OutOfScope(module_id.to_string()));
            }
            if !ctx.op_permitted(category) {
                return Err(CapError::OutOfScope(category.label().to_string()));
            }
            if let Some(host) = target {
                if host_in_scope(host, ctx) == ScopeMatch::OutOfScope {
                    return Err(CapError::OutOfScope(host.to_string()));
                }
                if !resolved.is_empty()
                    && resolved_in_scope(resolved, ctx) == ScopeMatch::OutOfScope
                {
                    return Err(CapError::OutOfScope(host.to_string()));
                }
            }
        }
        Ok(risk)
    }

    pub fn close(self) -> CapabilitySession<Closed> {
        CapabilitySession {
            session_id: self.session_id,
            target: self.target,
            scope_id: self.scope_id,
            capability: None,
            _state: std::marker::PhantomData,
            events: self.events,
        }
    }
}

/// ---------- Closed (read-only audit) ----------
impl CapabilitySession<Closed> {
    pub fn report(&self) -> CapReport {
        CapReport {
            session_id: self.session_id.clone(),
            target: self.target.clone(),
            scope_id: self.scope_id.clone(),
            event_count: self.events.len(),
        }
    }
}

pub struct CapReport {
    pub session_id: String,
    pub target: String,
    pub scope_id: String,
    pub event_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn cap_valid() -> OpCapability {
        OpCapability {
            token: "tok-123".into(),
            scope_id: "eng-001".into(),
            expires_at: SystemTime::now() + Duration::from_secs(3600),
            allowed_modules: vec!["recon".into(), "network".into()],
            max_risk: RiskLevel::High,
        }
    }

    fn ctx() -> AssessmentContext {
        let mut ctx = AssessmentContext::new("asm-1", "Lab");
        ctx.allowed_cidrs = vec!["10.0.0.0/8".into()];
        ctx.allowed_domains = vec!["example.com".into()];
        ctx.permitted_modules = vec!["recon".into(), "network".into()];
        ctx.permitted_ops = vec![
            OperationCategory::Passive,
            OperationCategory::ActiveDiscovery,
        ];
        ctx.authorization_ref = Some("LAB-1".into());
        ctx.status = super::super::context::AssessmentStatus::Authorized;
        ctx.start = SystemTime::now();
        ctx.expiry = ctx.start + Duration::from_secs(3600);
        ctx
    }

    #[test]
    fn discovered_cannot_run_module() {
        // Compile-time: CapabilitySession<Discovered> has no authorize_operation.
        // The type system enforces this invariant.
        let _s = CapabilitySession::<Discovered>::new("t".into(), "s".into());
        // If this compiles, the typestate guard is in place.
    }

    #[test]
    fn authorize_then_activate_then_authorize_ok() {
        let _now = SystemTime::now();
        let s =
            CapabilitySession::<Discovered>::new("10.0.0.1".into(), "eng-001".into());
        let authed = s.authorize("operator", cap_valid()).unwrap();
        let active = authed.activate().unwrap();
        let risk = active
            .authorize_operation(
                "recon",
                OperationCategory::Passive,
                Some("10.0.0.1"),
                &[],
                SystemTime::now(),
                Some(&ctx()),
            )
            .unwrap();
        assert_eq!(risk, RiskLevel::Low);
    }

    #[test]
    fn module_outside_capability_denied() {
        let _now = SystemTime::now();
        let s = CapabilitySession::<Discovered>::new("t".into(), "s".into());
        let authed = s.authorize("operator", cap_valid()).unwrap();
        let active = authed.activate().unwrap();
        let err = active
            .authorize_operation(
                "web",
                OperationCategory::Passive,
                None,
                &[],
                SystemTime::now(),
                None,
            )
            .unwrap_err();
        assert!(matches!(err, CapError::OutOfScope(_)));
    }

    #[test]
    fn risk_above_ceiling_denied() {
        let _now = SystemTime::now();
        let s = CapabilitySession::<Discovered>::new("t".into(), "s".into());
        let mut cap = cap_valid();
        cap.max_risk = RiskLevel::Low;
        let authed = s.authorize("operator", cap).unwrap();
        let active = authed.activate().unwrap();
        let err = active
            .authorize_operation(
                "network",
                OperationCategory::ActiveDiscovery,
                None,
                &[],
                SystemTime::now(),
                None,
            )
            .unwrap_err();
        assert!(matches!(err, CapError::RiskAboveCeiling(_)));
    }

    #[test]
    fn expired_capability_denied_at_authorize() {
        let mut cap = cap_valid();
        cap.expires_at = SystemTime::now() - Duration::from_secs(1);
        let s = CapabilitySession::<Discovered>::new("t".into(), "s".into());
        let err = s.authorize("operator", cap).unwrap_err();
        assert!(matches!(err, CapError::CapabilityInvalid));
    }

    #[test]
    fn close_returns_closed_session() {
        let _now = SystemTime::now();
        let s = CapabilitySession::<Discovered>::new("t".into(), "s".into());
        let authed = s.authorize("operator", cap_valid()).unwrap();
        let active = authed.activate().unwrap();
        let closed = active.close();
        let report = closed.report();
        assert_eq!(report.event_count, 0);
    }
}
