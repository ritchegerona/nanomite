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
use super::risk::{effective_risk, OperationCategory, RiskLevel};
use super::scope::{host_in_scope, resolved_in_scope, ScopeMatch};

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

fn deny(code: &'static str, reason: impl Into<String>) -> Decision {
    Decision::Denied {
        code,
        reason: reason.into(),
    }
}

/// Authorizes one operation against an assessment.
pub fn authorize(ctx: Option<&AssessmentContext>, op: &Operation<'_>, now: SystemTime) -> Decision {
    let Some(ctx) = ctx else {
        return deny(
            "hacker_no_assessment",
            "no assessment is active; HACKER MODE operations require an authorized assessment",
        );
    };
    if let Err(errors) = ctx.validate() {
        return deny(
            "hacker_invalid_assessment",
            format!("assessment is invalid: {}", errors.join("; ")),
        );
    }
    if !ctx.is_active(now) {
        return deny(
            "hacker_assessment_inactive",
            format!(
                "assessment `{}` is {} and cannot authorize operations",
                ctx.id,
                ctx.status.label()
            ),
        );
    }
    if !ctx.module_permitted(op.module_id) {
        return deny(
            "hacker_module_not_permitted",
            format!(
                "module `{}` is not permitted by this assessment",
                op.module_id
            ),
        );
    }
    if !ctx.op_permitted(op.category) {
        return deny(
            "hacker_operation_not_permitted",
            format!(
                "operation category `{}` is not permitted by this assessment",
                op.category.label()
            ),
        );
    }

    // Active discovery and beyond must name an in-scope target.
    let needs_target = !matches!(op.category, OperationCategory::Passive);
    match op.target {
        Some(target) => {
            let host = host_in_scope(target, ctx);
            if host == ScopeMatch::OutOfScope {
                return deny(
                    "hacker_target_out_of_scope",
                    format!("target `{target}` is outside the authorized scope"),
                );
            }
            if host == ScopeMatch::Ambiguous && needs_target {
                return deny(
                    "hacker_target_unresolved",
                    format!("target `{target}` could not be validated against scope"),
                );
            }
            // If callers resolved the target, every address must be in scope.
            if !op.resolved.is_empty()
                && resolved_in_scope(op.resolved, ctx) == ScopeMatch::OutOfScope
            {
                return deny(
                    "hacker_resolved_out_of_scope",
                    format!(
                        "target `{target}` resolved to at least one address outside the authorized scope"
                    ),
                );
            }
        }
        None if needs_target => {
            return deny(
                "hacker_target_required",
                format!(
                    "operation category `{}` requires an explicit in-scope target",
                    op.category.label()
                ),
            );
        }
        None => {}
    }

    Decision::Allowed {
        risk: effective_risk(op.category, op.declared_risk),
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
    fn missing_assessment_is_denied() {
        let op = Operation::new("recon", OperationCategory::Passive);
        let d = authorize(None, &op, SystemTime::now());
        assert!(matches!(d, Decision::Denied { code, .. } if code == "hacker_no_assessment"));
        assert!(!d.is_allowed());
    }

    #[test]
    fn expired_assessment_cannot_authorize() {
        let ctx = ctx();
        let op = Operation::new("recon", OperationCategory::Passive);
        let after = ctx.expiry + Duration::from_secs(1);
        let d = authorize(Some(&ctx), &op, after);
        assert!(matches!(d, Decision::Denied { code, .. } if code == "hacker_assessment_inactive"));
    }

    #[test]
    fn unpermitted_module_or_operation_is_denied() {
        let ctx = ctx();
        let now = now(&ctx);
        let wrong_module = Operation::new("web", OperationCategory::Passive);
        assert!(matches!(
            authorize(Some(&ctx), &wrong_module, now),
            Decision::Denied { code, .. } if code == "hacker_module_not_permitted"
        ));

        let wrong_op = Operation::new("recon", OperationCategory::Intrusive);
        assert!(matches!(
            authorize(Some(&ctx), &wrong_op, now),
            Decision::Denied { code, .. } if code == "hacker_operation_not_permitted"
        ));
    }

    #[test]
    fn out_of_scope_target_is_denied() {
        let ctx = ctx();
        let now = now(&ctx);
        let op = Operation::new("recon", OperationCategory::ActiveDiscovery).target("8.8.8.8");
        assert!(matches!(
            authorize(Some(&ctx), &op, now),
            Decision::Denied { code, .. } if code == "hacker_target_out_of_scope"
        ));
    }

    #[test]
    fn resolved_addresses_outside_scope_deny_a_permitted_hostname() {
        let ctx = ctx();
        let now = now(&ctx);
        let resolved = vec!["8.8.8.8".parse::<IpAddr>().unwrap()];
        let op = Operation::new("recon", OperationCategory::ActiveDiscovery)
            .target("api.example.com")
            .resolved(&resolved);
        assert!(matches!(
            authorize(Some(&ctx), &op, now),
            Decision::Denied { code, .. } if code == "hacker_resolved_out_of_scope"
        ));
    }

    #[test]
    fn active_operation_requires_a_target() {
        let ctx = ctx();
        let now = now(&ctx);
        let op = Operation::new("recon", OperationCategory::ActiveDiscovery);
        assert!(matches!(
            authorize(Some(&ctx), &op, now),
            Decision::Denied { code, .. } if code == "hacker_target_required"
        ));
    }

    #[test]
    fn in_scope_passive_operation_is_allowed() {
        let ctx = ctx();
        let now = now(&ctx);
        let op = Operation::new("recon", OperationCategory::Passive);
        let d = authorize(Some(&ctx), &op, now);
        assert!(d.is_allowed());
        assert_eq!(d.risk(), Some(RiskLevel::Low));
    }

    #[test]
    fn intrusive_declared_informational_still_reports_critical_floor() {
        // Even if the module under-declares, the category floor wins.
        let mut ctx = ctx();
        ctx.permitted_ops = vec![OperationCategory::Intrusive];
        let now = now(&ctx);
        let op = Operation::new("recon", OperationCategory::Intrusive)
            .target("10.0.0.5")
            .declared_risk(RiskLevel::Informational);
        let d = authorize(Some(&ctx), &op, now);
        assert_eq!(d.risk(), Some(RiskLevel::Critical));
    }
}
