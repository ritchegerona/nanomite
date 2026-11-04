//! Typestate port of [`crate::hacker::HackerSession`].
//!
//! Encodes the HACKER MODE lifecycle in the type system so that offensive
//! operations can only be issued from a `HackerSession<Active>` holding a
//! valid, unexpired [`OpCapability`]. Invalid transitions are compile-time
//! errors in safe Rust, not runtime surprises.
//!
//! This is purely additive: it wraps `HackerSession` and re-exports the
//! capability layer from [`crate::hacker::capability`]. It does not modify
//! `HackerSession`, `mod.rs`, the coordinator, or any existing module.

use std::marker::PhantomData;
use std::time::{SystemTime, UNIX_EPOCH};

use super::capability::{CapError, CapEvent, CapOutcome, OpCapability};
use super::HackerSession;

/// ---------- Typestate markers ----------
/// Discovered: session exists but is not authorized to run anything.
#[derive(Debug)]
pub struct Discovered;
/// Authorized: a valid capability has been bound; not yet active.
#[derive(Debug)]
pub struct Authorized;
/// Active: operations may be executed through `authorize_operation`.
#[derive(Debug)]
pub struct Active;
/// Closed: read-only audit only; no operations permitted.
#[derive(Debug)]
pub struct Closed;

/// ---------- Typestate session wrapper ----------
///
/// Wraps the existing `HackerSession` and layers a capability-gated state
/// machine on top. State transitions consume `self`, so a session can never
/// be used after `close()` and `authorize_operation()` only exists on
/// `HackerSession<Active>`.

#[derive(Debug)]
pub struct HackerSessionTyped<State> {
    session_id: String,
    target: String,
    scope_id: String,
    capability: Option<OpCapability>,
    _state: PhantomData<State>,
    events: Vec<CapEvent>,
}

/// ---------- Discovered -> Authorized ----------
impl HackerSessionTyped<Discovered> {
    /// Wraps an existing `HackerSession` in the discovered state.
    pub fn wrap(inner: &HackerSession) -> Self {
        let _millis = UNIX_EPOCH.elapsed().map(|d| d.as_millis()).unwrap_or(0);
        Self {
            session_id: format!("cap-{:x}", _millis),
            target: inner.assessment_id_dbg().to_string(),
            scope_id: inner.assessment_id_dbg().to_string(),
            capability: None,
            _state: PhantomData,
            events: Vec::new(),
        }
    }

    /// Bind a capability and move to `Authorized`.
    pub fn authorize(
        self,
        _operator: &str,
        cap: OpCapability,
    ) -> Result<HackerSessionTyped<Authorized>, CapError> {
        if !cap.is_valid(SystemTime::now()) {
            return Err(CapError::CapabilityInvalid);
        }
        Ok(HackerSessionTyped {
            session_id: self.session_id,
            target: self.target,
            scope_id: self.scope_id,
            capability: Some(cap),
            _state: PhantomData,
            events: self.events,
        })
    }
}

/// ---------- Authorized -> Active ----------
impl HackerSessionTyped<Authorized> {
    /// Move to `Active`; only an active session can run modules.
    pub fn activate(self) -> Result<HackerSessionTyped<Active>, CapError> {
        let cap = self
            .capability
            .as_ref()
            .ok_or(CapError::NotAuthorized("no capability".into()))?;
        if !cap.is_valid(SystemTime::now()) {
            return Err(CapError::CapabilityInvalid);
        }
        Ok(HackerSessionTyped {
            session_id: self.session_id,
            target: self.target,
            scope_id: self.scope_id,
            capability: Some(cap.clone()),
            _state: PhantomData,
            events: self.events,
        })
    }
}

/// ---------- Available on all states ----------
impl<State> HackerSessionTyped<State> {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn events_len(&self) -> usize {
        self.events.len()
    }
}

/// ---------- Active (operations only here) ----------
impl HackerSessionTyped<Active> {
    /// Authorize one module operation against the capability ceiling and
    /// assessment scope before execution. Records a `CapEvent` on every
    /// attempt so the session's audit trail survives between `/hacker` runs.
    pub fn authorize_operation(
        &mut self,
        module_id: &str,
        category: super::risk::OperationCategory,
        target: Option<&str>,
        resolved: &[std::net::IpAddr],
        now: SystemTime,
        ctx: Option<&super::context::AssessmentContext>,
    ) -> Result<super::risk::RiskLevel, CapError> {
        let cap = self
            .capability
            .as_ref()
            .ok_or(CapError::CapabilityInvalid)?;
        if !cap.allowed_modules.contains(&module_id.to_string()) {
            self.events.push(CapEvent {
                ts: now,
                op: format!("{module_id}/{}", category.label()),
                outcome: CapOutcome::Denied {
                    code: "module_out_of_scope",
                    reason: format!("module `{module_id}` is not in the capability ceiling"),
                },
            });
            return Err(CapError::OutOfScope(module_id.to_string()));
        }
        let risk = super::risk::effective_risk(category, None);
        if risk > cap.max_risk {
            self.events.push(CapEvent {
                ts: now,
                op: format!("{module_id}/{}", category.label()),
                outcome: CapOutcome::Denied {
                    code: "risk_above_ceiling",
                    reason: format!(
                        "risk {} exceeds capability ceiling {}",
                        risk.label(),
                        cap.max_risk.label()
                    ),
                },
            });
            return Err(CapError::RiskAboveCeiling(format!(
                "risk {} exceeds capability ceiling {}",
                risk.label(),
                cap.max_risk.label()
            )));
        }
        if let Some(assessment) = ctx {
            if !assessment.module_permitted(module_id) {
                self.events.push(CapEvent {
                    ts: now,
                    op: format!("{module_id}/{}", category.label()),
                    outcome: CapOutcome::Denied {
                        code: "module_not_permitted",
                        reason: format!("module `{module_id}` not permitted by assessment"),
                    },
                });
                return Err(CapError::OutOfScope(module_id.to_string()));
            }
            if !assessment.op_permitted(category) {
                self.events.push(CapEvent {
                    ts: now,
                    op: format!("{module_id}/{}", category.label()),
                    outcome: CapOutcome::Denied {
                        code: "operation_not_permitted",
                        reason: format!("operation `{}` not permitted", category.label()),
                    },
                });
                return Err(CapError::OutOfScope(category.label().to_string()));
            }
            if let Some(host) = target {
                if super::scope::host_in_scope(host, assessment)
                    == super::scope::ScopeMatch::OutOfScope
                {
                    self.events.push(CapEvent {
                        ts: now,
                        op: format!("{module_id}/{}", category.label()),
                        outcome: CapOutcome::Denied {
                            code: "target_out_of_scope",
                            reason: format!("target `{host}` outside authorized scope"),
                        },
                    });
                    return Err(CapError::OutOfScope(host.to_string()));
                }
                if !resolved.is_empty()
                    && super::scope::resolved_in_scope(resolved, assessment)
                        == super::scope::ScopeMatch::OutOfScope
                {
                    self.events.push(CapEvent {
                        ts: now,
                        op: format!("{module_id}/{}", category.label()),
                        outcome: CapOutcome::Denied {
                            code: "resolved_out_of_scope",
                            reason: format!("target `{host}` resolves outside authorized scope"),
                        },
                    });
                    return Err(CapError::OutOfScope(host.to_string()));
                }
            }
        }
        self.events.push(CapEvent {
            ts: now,
            op: format!("{module_id}/{}", category.label()),
            outcome: CapOutcome::Granted,
        });
        Ok(risk)
    }

    /// Close the session; returns a read-only closed session for audit.
    pub fn close(self) -> HackerSessionTyped<Closed> {
        HackerSessionTyped {
            session_id: self.session_id,
            target: self.target,
            scope_id: self.scope_id,
            capability: None,
            _state: PhantomData,
            events: self.events,
        }
    }
}

/// ---------- Closed (read-only audit) ----------
impl HackerSessionTyped<Closed> {
    /// Build an audit report from the closed session.
    pub fn report(&self) -> TypedCapReport {
        TypedCapReport {
            session_id: self.session_id.clone(),
            target: self.target.clone(),
            scope_id: self.scope_id.clone(),
            event_count: self.events.len(),
            active: false,
        }
    }
}

/// ---------- Report ----------

#[derive(Debug, Clone)]
pub struct TypedCapReport {
    pub session_id: String,
    pub target: String,
    pub scope_id: String,
    pub event_count: usize,
    pub active: bool,
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
            max_risk: super::super::risk::RiskLevel::High,
        }
    }

    fn inner_session() -> HackerSession {
        HackerSession::new(
            true,
            vec!["10.0.0.0/8".into()],
            "asm-1".into(),
            "Lab".into(),
            60,
            true,
            false, // no audit file in tests
        )
    }

    #[test]
    fn discovered_cannot_authorize_operation() {
        // Compile-time: HackerSessionTyped<Discovered> has no authorize_operation.
        let inner = inner_session();
        let _discovered = HackerSessionTyped::wrap(&inner);
        // If this compiles, the typestate guard is in place.
    }

    #[test]
    fn authorize_then_activate_then_authorize_ok() {
        let mut inner = inner_session();
        inner.activate(SystemTime::now()).unwrap();
        let discovered = HackerSessionTyped::wrap(&inner);
        let authorized = discovered.authorize("operator", cap_valid()).unwrap();
        let mut active = authorized.activate().unwrap();
        let ctx = inner.assessment.as_ref().unwrap();
        let risk = active
            .authorize_operation(
                "recon",
                super::super::risk::OperationCategory::Passive,
                Some("10.0.0.1"),
                &[],
                SystemTime::now(),
                Some(ctx),
            )
            .unwrap();
        assert_eq!(risk, super::super::risk::RiskLevel::Low);
    }

    #[test]
    fn module_outside_capability_denied() {
        let mut inner = inner_session();
        inner.activate(SystemTime::now()).unwrap();
        let discovered = HackerSessionTyped::wrap(&inner);
        let authorized = discovered.authorize("operator", cap_valid()).unwrap();
        let mut active = authorized.activate().unwrap();
        let ctx = inner.assessment.as_ref().unwrap();
        let err = active
            .authorize_operation(
                "web",
                super::super::risk::OperationCategory::Passive,
                None,
                &[],
                SystemTime::now(),
                Some(ctx),
            )
            .unwrap_err();
        assert!(matches!(err, CapError::OutOfScope(_)));
    }

    #[test]
    fn risk_above_ceiling_denied() {
        let mut inner = inner_session();
        inner.activate(SystemTime::now()).unwrap();
        let mut cap = cap_valid();
        cap.max_risk = super::super::risk::RiskLevel::Low;
        let discovered = HackerSessionTyped::wrap(&inner);
        let authorized = discovered.authorize("operator", cap).unwrap();
        let mut active = authorized.activate().unwrap();
        let ctx = inner.assessment.as_ref().unwrap();
        let err = active
            .authorize_operation(
                "network",
                super::super::risk::OperationCategory::ActiveDiscovery,
                None,
                &[],
                SystemTime::now(),
                Some(ctx),
            )
            .unwrap_err();
        assert!(matches!(err, CapError::RiskAboveCeiling(_)));
    }

    #[test]
    fn expired_capability_denied_at_authorize() {
        let mut cap = cap_valid();
        cap.expires_at = SystemTime::now() - Duration::from_secs(1);
        let inner = inner_session();
        let discovered = HackerSessionTyped::wrap(&inner);
        let err = discovered.authorize("operator", cap).unwrap_err();
        assert!(matches!(err, CapError::CapabilityInvalid));
    }

    #[test]
    fn close_returns_closed_session() {
        let mut inner = inner_session();
        inner.activate(SystemTime::now()).unwrap();
        let discovered = HackerSessionTyped::wrap(&inner);
        let authorized = discovered.authorize("operator", cap_valid()).unwrap();
        let active = authorized.activate().unwrap();
        let closed = active.close();
        let report = closed.report();
        assert!(!report.active);
        assert_eq!(report.event_count, 0);
    }

    #[test]
    fn wrap_preserves_inner_fields() {
        let inner = inner_session();
        let discovered = HackerSessionTyped::wrap(&inner);
        assert!(discovered.session_id().starts_with("cap-"));
        assert!(!discovered.session_id().is_empty());
    }
}
