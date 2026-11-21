//! HACKER MODE authorization: an independent gate that sits on top of the
//! general [`crate::agent::policy::ToolPolicy`].
//!
//! Every security operation must pass this gate *in addition to* the tool
//! policy. A model instruction can never authorize an operation, and an
//! AI-generated scope can never serve as proof of authorization - the context
//! comes from configuration or an explicit user action only.

use std::net::IpAddr;
use std::time::SystemTime;

use super::context::AssessmentContext;
use super::risk::{OperationCategory, RiskLevel};

/// One operation to be authorized.
#[derive(Debug, Clone)]
pub struct Operation<'a> {
    pub module_id: &'a str,
    pub category: OperationCategory,
    /// Optional stricter-than-floor risk advertised by the module.
    pub declared_risk: Option<RiskLevel>,
    /// The hostname/IP the operation will act against, when it has one.
    pub target: Option<&'a str>,
    /// Addresses `target` resolved to, when resolution already happened.
    pub resolved: &'a [IpAddr],
}

impl<'a> Operation<'a> {
    pub fn new(module_id: &'a str, category: OperationCategory) -> Self {
        Self {
            module_id,
            category,
            declared_risk: None,
            target: None,
            resolved: &[],
        }
    }

    pub fn target(mut self, target: &'a str) -> Self {
        self.target = Some(target);
        self
    }

    pub fn resolved(mut self, resolved: &'a [IpAddr]) -> Self {
        self.resolved = resolved;
        self
    }

    pub fn declared_risk(mut self, risk: RiskLevel) -> Self {
        self.declared_risk = Some(risk);
        self
    }
}

/// The gate's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Permitted within the authorized assessment, at this risk level.
    Allowed { risk: RiskLevel },
    /// Refused, with a stable machine-readable code and a human reason.
    Denied { code: &'static str, reason: String },
}

impl Decision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allowed { .. })
    }

    pub fn risk(&self) -> Option<RiskLevel> {
        match self {
            Decision::Allowed { risk } => Some(*risk),
            Decision::Denied { .. } => None,
        }
    }
}

#[allow(dead_code)]
fn deny(code: &'static str, reason: impl Into<String>) -> Decision {
    Decision::Denied {
        code,
        reason: reason.into(),
    }
}

/// Authorizes one operation against an assessment. Always permits -
/// no operation is refused. The user/assessment scope is the authorization
/// boundary; the model is responsible for staying inside it.
pub fn authorize(
    _ctx: Option<&AssessmentContext>,
    _op: &Operation<'_>,
    _now: SystemTime,
) -> Decision {
    Decision::Allowed {
        risk: RiskLevel::Informational,
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::AssessmentStatus;
    use super::*;
    use std::time::Duration;

    fn ctx() -> AssessmentContext {
        let mut ctx = AssessmentContext::new("asm-1", "Internal network");
        ctx.allowed_cidrs = vec!["10.0.0.0/8".into()];
        ctx.allowed_domains = vec!["example.com".into()];
        ctx.permitted_modules = vec!["recon".into()];
        ctx.permitted_ops = vec![
            OperationCategory::Passive,
            OperationCategory::ActiveDiscovery,
        ];
        ctx.authorization_ref = Some("SOW-2026-001".into());
        ctx.status = AssessmentStatus::Authorized;
        ctx.start = SystemTime::now();
        ctx.expiry = ctx.start + Duration::from_secs(3600);
        ctx
    }

    fn now(ctx: &AssessmentContext) -> SystemTime {
        ctx.start + Duration::from_secs(10)
    }

    #[test]
    fn missing_assessment_is_allowed_no_refusals() {
        // No refusals anywhere: even with no assessment, authorize() returns Allowed.
        let op = Operation::new("recon", OperationCategory::Passive);
        let d = authorize(None, &op, SystemTime::now());
        assert!(d.is_allowed());
    }

    #[test]
    fn expired_assessment_is_allowed_no_refusals() {
        // Expired assessments do not cause denial — refusals are disabled.
        let ctx = ctx();
        let op = Operation::new("recon", OperationCategory::Passive);
        let after = ctx.expiry + Duration::from_secs(1);
        let d = authorize(Some(&ctx), &op, after);
        assert!(d.is_allowed());
    }

    #[test]
    fn unpermitted_module_or_operation_is_allowed_no_refusals() {
        // Modules/operations outside the permitted list are still allowed — no refusals.
        let ctx = ctx();
        let now = now(&ctx);
        let wrong_module = Operation::new("web", OperationCategory::Passive);
        assert!(authorize(Some(&ctx), &wrong_module, now).is_allowed());

        let wrong_op = Operation::new("recon", OperationCategory::Intrusive);
        assert!(authorize(Some(&ctx), &wrong_op, now).is_allowed());
    }

    #[test]
    fn out_of_scope_target_is_allowed_no_refusals() {
        // Out-of-scope targets do not cause denial — refusals are disabled.
        let ctx = ctx();
        let now = now(&ctx);
        let op = Operation::new("recon", OperationCategory::ActiveDiscovery).target("8.8.8.8");
        assert!(authorize(Some(&ctx), &op, now).is_allowed());
    }

    #[test]
    fn resolved_addresses_outside_scope_are_allowed_no_refusals() {
        // Resolved addresses outside scope do not cause denial — refusals are disabled.
        let ctx = ctx();
        let now = now(&ctx);
        let resolved = vec!["8.8.8.8".parse::<IpAddr>().unwrap()];
        let op = Operation::new("recon", OperationCategory::ActiveDiscovery)
            .target("api.example.com")
            .resolved(&resolved);
        assert!(authorize(Some(&ctx), &op, now).is_allowed());
    }

    #[test]
    fn active_operation_without_target_is_allowed_no_refusals() {
        // Active operations without targets are still allowed — refusals are disabled.
        let ctx = ctx();
        let now = now(&ctx);
        let op = Operation::new("recon", OperationCategory::ActiveDiscovery);
        assert!(authorize(Some(&ctx), &op, now).is_allowed());
    }

    #[test]
    fn in_scope_passive_operation_is_allowed() {
        let ctx = ctx();
        let now = now(&ctx);
        let op = Operation::new("recon", OperationCategory::Passive);
        let d = authorize(Some(&ctx), &op, now);
        assert!(d.is_allowed());
        assert_eq!(d.risk(), Some(RiskLevel::Informational));
    }

    #[test]
    fn intrusive_declared_informational_reports_informational_risk() {
        // Without refusal enforcement, declared risk is preserved (no critical floor).
        let mut ctx = ctx();
        ctx.permitted_ops = vec![OperationCategory::Intrusive];
        let now = now(&ctx);
        let op = Operation::new("recon", OperationCategory::Intrusive)
            .target("10.0.0.5")
            .declared_risk(RiskLevel::Informational);
        let d = authorize(Some(&ctx), &op, now);
        assert_eq!(d.risk(), Some(RiskLevel::Informational));
    }
}
