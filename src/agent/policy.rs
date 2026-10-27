//! Central tool authorization policy.
//!
//! Every tool call funnels through `runtime::run_tool`, so that one choke point
//! is where a policy is enforced: a call the policy refuses is returned to the
//! model as a JSON error it can read and react to. This is deliberately
//! *subsystem-independent* - unlike the memory engine's own
//! `MemoryPolicy::authorize`, which only guards memory.
//!
//! Risk classification is intentionally coarse and conservative: the goal is a
//! mode switch that stops a model from mutating state or leaving the machine,
//! not a per-argument permission model. Unknown names classify as `Safe` so a
//! new read-only tool is never accidentally blocked; genuinely unknown names
//! are still rejected by the dispatch layer.

/// Risk class of a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Read-only: lists, reads, searches, web fetches, memory lookups.
    Safe,
    /// Mutating: workspace writes, memory writes, staging a self-edit.
    Write,
    /// Leaves the machine: shell, SSH, or deleting stored state.
    External,
}

impl Risk {
    /// Risk class of a tool name. Unknown names are `Safe`; the dispatcher
    /// still rejects names it does not know.
    pub fn of(name: &str) -> Risk {
        match name.trim() {
            "run_command" | "ssh_run" | "memory_delete" => Risk::External,
            "write_file" | "edit_file" | "memory_save" | "memory_update" | "propose_self_edit"
            | "git_commit" | "git_branch" | "index_symbols" | "create_plan" | "update_task"
            | "delete_plan" => Risk::Write,
            _ => Risk::Safe,
        }
    }
}

/// The plan tools stay callable in Plan mode: building and refining the plan
/// is the mode's whole purpose. Everything mutating still waits for the user.
pub fn is_plan_tool(name: &str) -> bool {
    matches!(
        name.trim(),
        "create_plan" | "get_plan" | "update_task" | "list_plans"
    )
}

/// Which calls the current mode permits.
///
/// The default is [`ToolPolicy::Auto`]: everything the user has enabled runs,
/// exactly as before the policy existed, so upgrading never silently changes
/// behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolPolicy {
    #[default]
    Auto,
    /// Plan mode: reads run; mutations are refused with an instruction to
    /// report a plan instead.
    Plan,
    /// Read-only: reads run; mutations are refused.
    Readonly,
}

impl ToolPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolPolicy::Auto => "auto",
            ToolPolicy::Plan => "plan",
            ToolPolicy::Readonly => "readonly",
        }
    }

    pub fn label(self) -> &'static str {
        self.as_str()
    }

    /// Parses a configured value. Unknown values return `None` so the caller can
    /// fall back to the default instead of silently mis-applying a mode.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "plan" => Some(Self::Plan),
            "readonly" | "read-only" | "read_only" => Some(Self::Readonly),
            _ => None,
        }
    }

    /// Cycles auto → plan → readonly → auto.
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Plan,
            Self::Plan => Self::Readonly,
            Self::Readonly => Self::Auto,
        }
    }

    /// Authorize one call. `Ok(())` permits it; `Err(payload)` is the JSON error
    /// returned to the model.
    pub fn authorize(&self, name: &str) -> Result<(), String> {
        let allowed = match self {
            Self::Auto => true,
            // Plan mode: reads run, plan tools run (creating the plan IS the
            // point of the mode), everything else waits for user approval.
            Self::Plan => Risk::of(name) == Risk::Safe || is_plan_tool(name),
            Self::Readonly => Risk::of(name) == Risk::Safe,
        };
        if allowed {
            return Ok(());
        }
        let (code, message) = match self {
            Self::Plan => (
                "plan_mode",
                "plan mode is active: mutating tools are disabled. Create a plan first with \
                 create_plan (allowed in this mode), describe each step and what you would \
                 run, then wait for the user to review it and switch back with /policy.",
            ),
            Self::Readonly => (
                "read_only_policy",
                "read-only policy: this tool changes state and is disabled. Read-only tools \
                 still work.",
            ),
            Self::Auto => unreachable!("auto never refuses"),
        };
        // Ensure error first for backward compatibility.
        Err(format!(
            "{{\"error\":\"{}\",\"code\":\"{}\"}}",
            message.replace('"', "\\\""),
            code
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_classes_are_stable() {
        assert_eq!(Risk::of("read_file"), Risk::Safe);
        assert_eq!(Risk::of("web_search"), Risk::Safe);
        assert_eq!(Risk::of("memory_list"), Risk::Safe);
        assert_eq!(Risk::of("write_file"), Risk::Write);
        assert_eq!(Risk::of("edit_file"), Risk::Write);
        assert_eq!(Risk::of("memory_save"), Risk::Write);
        assert_eq!(Risk::of("propose_self_edit"), Risk::Write);
        assert_eq!(Risk::of("run_command"), Risk::External);
        assert_eq!(Risk::of("ssh_run"), Risk::External);
        assert_eq!(Risk::of("memory_delete"), Risk::External);
        // Git: inspection is safe, history mutation is a write.
        assert_eq!(Risk::of("git_status"), Risk::Safe);
        assert_eq!(Risk::of("git_diff"), Risk::Safe);
        assert_eq!(Risk::of("git_log"), Risk::Safe);
        assert_eq!(Risk::of("git_commit"), Risk::Write);
        assert_eq!(Risk::of("git_branch"), Risk::Write);
        // Index: building writes cache, searching is safe.
        assert_eq!(Risk::of("index_symbols"), Risk::Write);
        assert_eq!(Risk::of("find_symbol"), Risk::Safe);
        assert_eq!(Risk::of("find_references"), Risk::Safe);
        // Plan: reads are safe, mutations are writes.
        assert_eq!(Risk::of("create_plan"), Risk::Write);
        assert_eq!(Risk::of("get_plan"), Risk::Safe);
        assert_eq!(Risk::of("update_task"), Risk::Write);
        assert_eq!(Risk::of("list_plans"), Risk::Safe);
        assert_eq!(Risk::of("delete_plan"), Risk::Write);
        // Unknown names are safe, not blocked.
        assert_eq!(Risk::of("some_future_tool"), Risk::Safe);
    }

    #[test]
    fn auto_allows_everything() {
        for name in ["read_file", "write_file", "run_command", "memory_delete"] {
            assert!(ToolPolicy::Auto.authorize(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn plan_mode_allows_plan_tools_but_not_other_writes() {
        for name in ["create_plan", "get_plan", "update_task", "list_plans"] {
            assert!(
                ToolPolicy::Plan.authorize(name).is_ok(),
                "{name} must run in plan mode"
            );
        }
        // delete_plan is destructive even for plans: still gated.
        assert!(ToolPolicy::Plan.authorize("delete_plan").is_err());
        for name in ["write_file", "edit_file", "git_commit", "run_command"] {
            assert!(
                ToolPolicy::Plan.authorize(name).is_err(),
                "{name} must be gated"
            );
        }
        // Readonly stays strict: plan mutations are refused there too.
        assert!(ToolPolicy::Readonly.authorize("create_plan").is_err());
    }

    #[test]
    fn plan_mode_refusal_points_at_create_plan() {
        let err = ToolPolicy::Plan.authorize("write_file").unwrap_err();
        assert!(
            err.contains("create_plan"),
            "refusal must guide the model: {err}"
        );
        assert!(err.contains("plan_mode"));
    }

    #[test]
    fn restricted_modes_allow_reads_and_refuse_mutations() {
        for policy in [ToolPolicy::Readonly] {
            for name in [
                "read_file",
                "list_files",
                "search",
                "web_fetch",
                "memory_get",
            ] {
                assert!(
                    policy.authorize(name).is_ok(),
                    "{policy:?} should allow {name}"
                );
            }
            for name in ["write_file", "edit_file", "memory_save", "run_command"] {
                let err = match policy.authorize(name) {
                    Ok(_) => panic!("{policy:?} should refuse {name}"),
                    Err(e) => e,
                };
                assert!(err.starts_with("{\"error\""), "model-readable JSON: {err}");
                assert!(err.contains("\"code\""), "{err}");
            }
        }
    }

    #[test]
    fn plan_and_readonly_carry_distinct_codes() {
        assert!(ToolPolicy::Plan
            .authorize("write_file")
            .unwrap_err()
            .contains("plan_mode"));
        assert!(ToolPolicy::Readonly
            .authorize("write_file")
            .unwrap_err()
            .contains("read_only_policy"));
    }

    #[test]
    fn parse_accepts_known_labels_and_rejects_unknown() {
        assert_eq!(ToolPolicy::parse("auto"), Some(ToolPolicy::Auto));
        assert_eq!(ToolPolicy::parse(" Plan "), Some(ToolPolicy::Plan));
        assert_eq!(ToolPolicy::parse("readonly"), Some(ToolPolicy::Readonly));
        assert_eq!(ToolPolicy::parse("read-only"), Some(ToolPolicy::Readonly));
        assert_eq!(ToolPolicy::parse("nope"), None);
    }

    #[test]
    fn cycle_visits_every_mode_once() {
        let mut seen = vec![ToolPolicy::Auto];
        let mut next = ToolPolicy::Auto.next();
        while next != ToolPolicy::Auto {
            assert!(!seen.contains(&next), "cycle repeated {next:?}");
            seen.push(next);
            next = next.next();
        }
        assert_eq!(seen.len(), 3);
    }
}
