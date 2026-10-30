//! Central tool authorization policy.
//!
//! Every tool call funnels through `runtime::run_tool`, so that one choke point
//! is where a policy is enforced: a call the policy refuses is returned to the
//! model as a JSON error it can read and react to. This is deliberately
//! *subsystem-independent* - unlike the memory engine's own
//! `MemoryPolicy::authorize`, which only guards memory.
//!
//! Four modes, cycling with Shift+Tab, each granting strictly more power than
//! the last (Plan ⊂ Coder ⊂ Build ⊂ Hacker):
//!
//! - **Plan** - reads + plan tools only. The agent researches and builds a
//!   plan; mutations are refused with guidance to `create_plan`.
//! - **Coder** - + file edits (write/edit files, symbol index, self-edit
//!   staging, memory writes). No command execution, no git history changes.
//! - **Build** - + local command execution and git commits/branches. Full
//!   local machine power for building and testing; still no remote access.
//! - **Hacker** - everything, including SSH to remote hosts. No guardrails.

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

/// Asks the user before a risky tool call runs. The UI layer implements
/// this over its event loop; builds without one (headless runs, tests, or
/// `confirm_external = false`) fall back to the mode policy alone, exactly
/// as before this gate existed.
pub trait Confirmer: Send + Sync {
    /// `true` allows the call; `false` refuses it with a user-declined
    /// error the model can read and route around.
    fn confirm(
        &self,
        tool: &str,
        preview: Option<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>;
}

/// Which tools each mode permits. Word-exact matches on the trimmed name.
fn is_plan_tool(name: &str) -> bool {
    matches!(
        name.trim(),
        "create_plan" | "get_plan" | "update_task" | "list_plans"
    )
}

/// The mode ladder: Plan ⊂ Coder ⊂ Build ⊂ Hacker.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolPolicy {
    /// Research and plan; mutations refused.
    Plan,
    /// + file edits and self-edit staging; no command execution or git history.
    Coder,
    /// + local command execution and git commits/branches. Default: full
    ///   local power for everyday building and testing.
    #[default]
    Build,
    /// Everything, including SSH to remote hosts. No guardrails.
    Hacker,
}

impl ToolPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolPolicy::Plan => "plan",
            ToolPolicy::Coder => "coder",
            ToolPolicy::Build => "build",
            ToolPolicy::Hacker => "hacker",
        }
    }

    pub fn label(self) -> &'static str {
        self.as_str()
    }

    /// Parses a configured value. Unknown values return `None` so the caller
    /// can fall back to the default instead of silently mis-applying a mode.
    /// Legacy labels from earlier releases migrate: `auto` -> Hacker (it was
    /// the everything-allowed mode), `readonly` -> Plan (the read-only mode).
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "plan" => Some(Self::Plan),
            "coder" => Some(Self::Coder),
            "build" => Some(Self::Build),
            "hacker" | "auto" => Some(Self::Hacker),
            "readonly" | "read-only" | "read_only" => Some(Self::Plan),
            _ => None,
        }
    }

    /// Cycles Plan -> Coder -> Build -> Hacker -> Plan: every press of
    /// Shift+Tab grants strictly more power, wrapping back to Plan.
    pub fn next(self) -> Self {
        match self {
            Self::Plan => Self::Coder,
            Self::Coder => Self::Build,
            Self::Build => Self::Hacker,
            Self::Hacker => Self::Plan,
        }
    }

    /// Authorize one call. `Ok(())` permits it; `Err(payload)` is the JSON
    /// error returned to the model.
    pub fn authorize(&self, name: &str) -> Result<(), String> {
        let allowed = match self {
            Self::Hacker => true,
            Self::Build => Self::build_allowed(name),
            Self::Coder => Self::coder_allowed(name),
            Self::Plan => Self::plan_allowed(name),
        };
        if allowed {
            return Ok(());
        }
        let (code, message) = match self {
            Self::Hacker => unreachable!("hacker never refuses"),
            Self::Build => (
                "build_mode",
                "build mode: remote (SSH) access is disabled. Everything local works - \\
                 ask the user to press Shift+Tab for hacker mode to run remote commands.",
            ),
            Self::Coder => (
                "coder_mode",
                "coder mode: command execution and git history changes are disabled. \\
                 Edit files freely; ask the user to press Shift+Tab for build mode \\
                 to run commands or commit.",
            ),
            Self::Plan => (
                "plan_mode",
                "plan mode is active: mutating tools are disabled. Create a plan first with \\
                 create_plan (allowed in this mode), describe each step and what you would \\
                 run, then wait for the user to review it and switch back with Shift+Tab.",
            ),
        };
        Err(format!(
            "{{\"error\":\"{}\",\"code\":\"{}\"}}",
            message.replace('"', "\\\""),
            code
        ))
    }

    /// Plan: reads + the plan tools. Building the plan IS the mode's purpose.
    fn plan_allowed(name: &str) -> bool {
        Risk::of(name) == Risk::Safe || is_plan_tool(name)
    }

    /// Coder: plan set + workspace file edits and memory/self-edit writes.
    /// Deliberately excludes run_command (execution), git_commit/git_branch
    /// (history), ssh_run (remote), memory_delete (destructive).
    fn coder_allowed(name: &str) -> bool {
        Self::plan_allowed(name)
            || matches!(
                name.trim(),
                "write_file"
                    | "edit_file"
                    | "index_symbols"
                    | "propose_self_edit"
                    | "memory_save"
                    | "memory_update"
            )
    }

    /// Build: coder set + local execution and git history. Still no SSH.
    fn build_allowed(name: &str) -> bool {
        Self::coder_allowed(name)
            || matches!(
                name.trim(),
                "run_command" | "git_commit" | "git_branch" | "delete_plan"
            )
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
    fn hacker_allows_everything() {
        for name in [
            "read_file",
            "write_file",
            "run_command",
            "ssh_run",
            "memory_delete",
            "git_commit",
        ] {
            assert!(ToolPolicy::Hacker.authorize(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn build_allows_local_power_but_refuses_ssh() {
        for name in [
            "write_file",
            "edit_file",
            "run_command",
            "git_commit",
            "git_branch",
        ] {
            assert!(ToolPolicy::Build.authorize(name).is_ok(), "{name}");
        }
        let err = ToolPolicy::Build.authorize("ssh_run").unwrap_err();
        assert!(err.contains("build_mode"), "{err}");
        assert!(
            err.contains("hacker"),
            "{err} must point at the escape hatch"
        );
    }

    #[test]
    fn coder_allows_file_edits_but_refuses_execution_and_history() {
        for name in [
            "write_file",
            "edit_file",
            "index_symbols",
            "propose_self_edit",
        ] {
            assert!(ToolPolicy::Coder.authorize(name).is_ok(), "{name}");
        }
        for name in ["run_command", "git_commit", "ssh_run", "delete_plan"] {
            let err = ToolPolicy::Coder.authorize(name).unwrap_err();
            assert!(err.contains("coder_mode"), "{name}: {err}");
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
        // Readonly-style strictness is preserved by Plan refusing plan writes
        // to nobody: plan tools are the only writes allowed here.
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
    fn modes_form_a_strict_ladder() {
        // Everything Plan allows, Coder allows; everything Coder allows,
        // Build allows; everything Build allows, Hacker allows.
        let tools = [
            "list_files",
            "read_file",
            "search",
            "write_file",
            "edit_file",
            "run_command",
            "ssh_run",
            "git_commit",
            "git_branch",
            "git_status",
            "run_tests",
            "index_symbols",
            "find_symbol",
            "create_plan",
            "update_task",
            "delete_plan",
            "memory_save",
            "memory_delete",
            "propose_self_edit",
        ];
        for name in tools {
            let plan = ToolPolicy::Plan.authorize(name).is_ok();
            let coder = ToolPolicy::Coder.authorize(name).is_ok();
            let build = ToolPolicy::Build.authorize(name).is_ok();
            let hacker = ToolPolicy::Hacker.authorize(name).is_ok();
            if plan {
                assert!(coder, "{name}: plan-allowed must be coder-allowed");
            }
            if coder {
                assert!(build, "{name}: coder-allowed must be build-allowed");
            }
            if build {
                assert!(hacker, "{name}: build-allowed must be hacker-allowed");
            }
        }
    }

    #[test]
    fn parse_accepts_known_labels_and_migrates_legacy() {
        assert_eq!(ToolPolicy::parse("plan"), Some(ToolPolicy::Plan));
        assert_eq!(ToolPolicy::parse(" Plan "), Some(ToolPolicy::Plan));
        assert_eq!(ToolPolicy::parse("coder"), Some(ToolPolicy::Coder));
        assert_eq!(ToolPolicy::parse("build"), Some(ToolPolicy::Build));
        assert_eq!(ToolPolicy::parse("hacker"), Some(ToolPolicy::Hacker));
        // Legacy labels from pre-0.6 configs.
        assert_eq!(ToolPolicy::parse("auto"), Some(ToolPolicy::Hacker));
        assert_eq!(ToolPolicy::parse("readonly"), Some(ToolPolicy::Plan));
        assert_eq!(ToolPolicy::parse("read-only"), Some(ToolPolicy::Plan));
        assert_eq!(ToolPolicy::parse("nope"), None);
    }

    #[test]
    fn default_is_build() {
        assert_eq!(ToolPolicy::default(), ToolPolicy::Build);
    }

    #[test]
    fn cycle_visits_every_mode_once() {
        let mut seen = vec![ToolPolicy::Plan];
        let mut next = ToolPolicy::Plan.next();
        while next != ToolPolicy::Plan {
            assert!(!seen.contains(&next), "cycle repeated {next:?}");
            seen.push(next);
            next = next.next();
        }
        assert_eq!(seen.len(), 4, "cycle must visit all four modes: {seen:?}");
        assert_eq!(next, ToolPolicy::Plan, "cycle must wrap back to Plan");
    }
}
