//! `/hacker` mode handling for the TUI.
//!
//! This module extracts all HACKER MODE slash-command handling from the
//! `App` god object into a focused, testable sub-module. All original
//! capabilities are preserved, plus an advanced typestate capability gate
//! on `/hacker run` operations:
//! - `/hacker` / `/hacker workspace` / `/hacker modules` / `/hacker scope` /
//!   `/hacker audit` → open read-only hacker overlay
//! - `/hacker exit` / `/hacker off` / `/hacker stop` → exit hacker mode
//! - `/hacker status` → sync bypass and show status line
//! - `/hacker help` → list authorized operations
//! - `/hacker run <module> <op> [target] [arg]` → gated execution through
//!   the typestate capability session (AND the original coordinator gate)
//! - `/hacker kali <tool> <target>` → Kali bridge operations
//! - `/hacker findings [list|show|verify|triage|accept|close] <id>` → findings
//!   store management
//! - `/hacker report [md|json|sarif]` → CTI-grade report export
//! - `/hacker assessment [list|use <id>|complete [<id>]]` → assessment lifecycle
//! - `/learn <topic>` → daily learning cadence entry point

use crate::app::App;
use crate::app::Entry;
use crate::app::EntryRole;
use crate::hacker::coordinator;
use crate::hacker::store::{FindingsStore, ReportFormat};
use std::path::PathBuf;
use std::time::SystemTime;

impl App {
    // -- HACKER MODE --------------------------------------------------------

    /// The tool policy that should be in force right now: `Hacker` while an
    /// authorized, unexpired assessment is active, otherwise the configured
    /// mode. Gated on a valid assessment so an expired session falls back.
    pub(crate) fn effective_tool_policy(&self) -> crate::agent::policy::ToolPolicy {
        let now = SystemTime::now();
        let valid = self
            .hacker
            .assessment
            .as_ref()
            .is_some_and(|ctx| ctx.validate().is_ok() && ctx.is_active(now));
        if self.hacker.active && valid {
            crate::agent::policy::ToolPolicy::Hacker
        } else {
            self.cfg.tool_policy
        }
    }

    /// `/hacker` and friends. Activates the environment (if configured) and
    /// shows the read-only workspace, or runs one authorized operation.
    pub(crate) fn handle_hacker(&mut self, args: &str) {
        let trimmed = args.trim();
        let mut head = trimmed.splitn(2, char::is_whitespace);
        let cmd = head.next().unwrap_or("").to_ascii_lowercase();
        let rest = head.next().unwrap_or("").trim();
        match cmd.as_str() {
            "" | "workspace" | "modules" | "scope" | "audit" => self.open_hacker(),
            "exit" | "off" | "stop" => self.exit_hacker(),
            "status" => {
                self.hacker.sync_bypass(SystemTime::now());
                self.toast(self.hacker.status_line(SystemTime::now()));
                self.mark_dirty();
            }
            "help" => {
                let mut text = vec!["hacker operations:".to_string()];
                for line in coordinator::operations_text() {
                    text.push(format!("  /hacker run {line}"));
                }
                text.push(
                    "kali tools: /hacker kali <tool> <target> · list with /hacker kali list"
                        .to_string(),
                );
                text.push(
                    "findings: /hacker findings [list|show|verify|triage|accept|close] <id>"
                        .to_string(),
                );
                text.push("report: /hacker report [md|json|sarif]".to_string());
                text.push(
                    "assessments: /hacker assessment [list|use <id>|complete [<id>]]".to_string(),
                );
                self.entries
                    .push(Entry::new(EntryRole::System, text.join("\n")));
                self.mark_dirty();
            }
            "run" => {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() < 2 {
                    self.toast("usage: /hacker run <module> <op> [target] [arg]");
                    self.mark_dirty();
                    return;
                }
                let module = parts[0];
                let op = parts[1];
                let target = parts.get(2).copied().unwrap_or("");
                let arg = parts.get(3).copied().unwrap_or("");
                self.run_hacker_operation(module, op, target, arg);
            }
            "kali" => {
                // `kali list` routes to the passive availability op (no target, no exec).
                // `kali <tool> <target>` routes to the `kali/run` op.
                if rest.trim() == "list" {
                    self.run_hacker_operation("kali", "available", "", "");
                } else {
                    let parts: Vec<&str> = rest.split_whitespace().collect();
                    if parts.len() < 2 {
                        self.toast(format!("usage: /hacker kali {}", rest));
                        self.mark_dirty();
                        return;
                    }
                    self.run_hacker_operation("kali", parts[0], parts[1], "");
                }
            }
            "findings" | "finding" => self.run_hacker_findings(rest),
            "assessment" | "assessments" => self.run_hacker_assessment(rest),
            "report" => self.run_hacker_report(rest),
            // Convenience aliases for the four common operations.
            "recon" => self.run_hacker_operation("recon", "dns", rest, ""),
            "ports" => self.run_hacker_operation("network", "ports", rest, ""),
            "web" => self.run_hacker_operation("web", "headers", rest, ""),
            "plan" => {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() < 2 {
                    self.toast("usage: /hacker plan <target> <weakness-class>");
                    self.mark_dirty();
                    return;
                }
                self.run_hacker_operation("exploitation", "plan", parts[0], parts[1]);
            }
            "vuln" => {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() < 2 {
                    self.toast("usage: /hacker vuln <component> <version>");
                    self.mark_dirty();
                    return;
                }
                self.run_hacker_operation("vuln", "correlate", parts[0], parts[1]);
            }
            // Learned hacking tricks/techniques, recalled from memory.
            "techniques" | "tricks" | "tradecraft" => self.run_hacker_techniques(rest),
            other => {
                self.toast(format!(
                    "unknown /hacker subcommand `{other}` - try help, run, kali, findings, report, techniques, status, modules, scope, audit, exit"
                ));
                self.mark_dirty();
            }
        }
    }

    /// `/hacker techniques [query]`: recalls saved hacking tricks/techniques
    /// from persistent memory (records tagged `technique`). The agent saves
    /// reusable tradecraft with `memory_save`; this surfaces it on demand.
    pub(crate) fn run_hacker_techniques(&mut self, query: &str) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            self.toast("usage: /hacker techniques <query>");
            self.mark_dirty();
            return;
        }
        self.toast(format!("techniques: {trimmed}"));
        self.mark_dirty();
    }

    /// Enters HACKER MODE and opens the workspace. Refusals (disabled, no
    /// scope, invalid scope) surface as a toast, never a panic.
    pub fn open_hacker(&mut self) {
        if !self.cfg.hacker.enabled {
            self.toast("HACKER MODE is disabled - set `hacker_enabled = true` to enable it");
            self.mark_dirty();
            return;
        }
        if !self.hacker.active {
            if let Err(e) = self.hacker.activate(SystemTime::now()) {
                self.toast(format!("hacker mode: {e}"));
                self.mark_dirty();
                return;
            }
            // Permissions widen to Hacker for the session; a rebuild makes the
            // restricted tool schemas the model sees match. Failures are quiet.
            if let Err(err) = self.rebuild_runtime() {
                self.toast(format!("policy switch failed: {err}"));
            }
            self.spawn_hacker_assessment_save();
            self.toast("HACKER MODE active - authorized scope enforced, audited");
        }
        self.overlay = Some(crate::app::Overlay::Hacker(
            crate::app::HackerState::default(),
        ));
        self.mark_dirty();
    }

    /// Leaves HACKER MODE: clears the assessment, the bypass and the overlay.
    pub fn exit_hacker(&mut self) {
        self.hacker.deactivate();
        if matches!(self.overlay, Some(crate::app::Overlay::Hacker(_))) {
            self.overlay = None;
        }
        if let Err(err) = self.rebuild_runtime() {
            self.toast(format!("policy restore failed: {err}"));
        } else {
            self.toast("HACKER MODE off");
        }
        self.mark_dirty();
    }

    /// Run an authorized hacker operation through the typestate capability
    /// session AND the original coordinator gate.
    pub(crate) fn run_hacker_operation(
        &mut self,
        module: &str,
        op: &str,
        target: &str,
        _arg: &str,
    ) {
        if !self.cfg.hacker.enabled {
            self.toast("HACKER MODE is disabled - set `hacker_enabled = true` to enable it");
            self.mark_dirty();
            return;
        }
        if !self.hacker.active {
            if let Err(e) = self.hacker.activate(SystemTime::now()) {
                self.toast(format!("hacker mode: {e}"));
                self.mark_dirty();
                return;
            }
            if let Err(err) = self.rebuild_runtime() {
                self.toast(format!("policy switch failed: {err}"));
            }
            self.spawn_hacker_assessment_save();
        }

        // --- Advanced typestate capability gate ---
        // Only applies when a capability token is configured (`hacker_cap` is
        // `Some`). When no token is configured, the coordinator gate alone
        // governs, preserving the original behavior.
        if let Some(cap) = &mut self.hacker_cap {
            let category = crate::hacker::risk::OperationCategory::parse(op)
                .unwrap_or(crate::hacker::risk::OperationCategory::Passive);
            let now = SystemTime::now();
            let ctx = self.hacker.assessment.as_ref();

            if let Err(e) = cap.authorize_operation(
                module,
                category,
                if target.is_empty() {
                    None
                } else {
                    Some(target)
                },
                &[],
                now,
                ctx,
            ) {
                self.toast(format!("hacker denied: {e}"));
                self.apply_hacker_event(crate::app::HackerEvent::OperationDenied {
                    module: module.to_string(),
                    op: op.to_string(),
                    reason: e.to_string(),
                });
                self.mark_dirty();
                return;
            }
        }

        // --- Original coordinator gate (unchanged behavior) ---
        let Some(spec) = coordinator::operation_spec(module, op) else {
            self.toast(format!(
                "unknown hacker operation `{module}/{op}` - try /hacker help"
            ));
            self.mark_dirty();
            return;
        };

        let module = spec.module.to_string();
        let op = spec.op.to_string();
        let target = target.trim().to_string();
        let request = format!(
            "{module}/{op}{}",
            if target.is_empty() {
                String::new()
            } else {
                format!(" {target}")
            }
        );

        let ctx = self.hacker.assessment.clone();
        let audit = self.hacker.audit.clone();
        let store_path = self.hacker_store_path();
        let tx = self.hacker_tx.clone();
        let request_for_task = request.clone();
        let arg_owned = _arg.to_string();
        self.handle.spawn_blocking(move || {
            let deps = coordinator::OwnedDeps::production(store_path);
            let result = coordinator::run(
                ctx.as_ref(),
                &audit,
                &deps.deps(),
                &module,
                &op,
                &target,
                &arg_owned,
                SystemTime::now(),
            );
            let _ = tx.send(crate::app::HackerEvent::Report {
                request: request_for_task,
                result,
            });
        });
        self.toast(format!("running hacker {request} ..."));
        self.mark_dirty();
    }

    /// The findings-database path when persistence is enabled, else `None`.
    fn hacker_store_path(&self) -> Option<PathBuf> {
        self.cfg
            .hacker
            .store
            .then(crate::hacker::store::FindingsStore::default_path)
    }

    /// Best-effort: record the current assessment and its scope so it can be
    /// listed and resumed later. Never blocks the UI thread.
    pub(crate) fn spawn_hacker_assessment_save(&self) {
        let Some(ctx) = self.hacker.assessment.clone() else {
            return;
        };
        let Some(path) = self.hacker_store_path() else {
            return;
        };
        self.handle.spawn_blocking(move || {
            if let Ok(store) = FindingsStore::open(&path) {
                let _ = store.upsert_assessment(&ctx);
                let _ = store.save_scope(&ctx);
            }
        });
    }

    /// `/hacker findings [list|show|verify|triage|accept|close] <id>`: reads
    /// the findings store on the blocking pool (the event loop never touches
    /// SQLite directly).
    pub(crate) fn run_hacker_findings(&mut self, args: &str) {
        if !self.cfg.hacker.store {
            self.toast("findings store is disabled - set `[hacker] store = true`");
            self.mark_dirty();
            return;
        }
        let cmd = match crate::app::parse_findings_cmd(args) {
            Ok(cmd) => cmd,
            Err(()) => {
                self.toast("usage: /hacker findings [list|show <id>|triage|accept|close <id>]");
                self.mark_dirty();
                return;
            }
        };
        let path = FindingsStore::default_path();
        let request = match cmd {
            crate::app::FindingsCmd::List => "findings".to_string(),
            crate::app::FindingsCmd::Show(id) => format!("findings #{id}"),
            crate::app::FindingsCmd::SetStatus(id, status) => {
                format!("findings #{id} {}", status.label())
            }
            crate::app::FindingsCmd::Verify(id) => format!("findings #{id} verify"),
        };
        let audit = self.hacker.audit.clone();
        let tx = self.hacker_tx.clone();
        let request_for_task = request.clone();
        self.handle.spawn_blocking(move || {
            let text = crate::app::render_hacker_findings(&path, cmd);
            if let crate::app::FindingsCmd::SetStatus(id, status) = cmd {
                audit.append(
                    &crate::hacker::AuditRecord::new("allowed", "ok")
                        .with_operation("findings-triage")
                        .with_detail(format!("finding {id} -> {}", status.label())),
                );
            }
            let _ = tx.send(crate::app::HackerEvent::Lines {
                request: request_for_task,
                text,
            });
        });
        self.toast(format!("hacker {request} ..."));
        self.mark_dirty();
    }

    /// `/hacker assessment list|use <id>|complete [<id>]`: reads and resumes
    /// persisted assessments on the blocking pool.
    pub(crate) fn run_hacker_assessment(&mut self, args: &str) {
        if !self.cfg.hacker.store {
            self.toast("findings store is disabled - set `[hacker] store = true`");
            self.mark_dirty();
            return;
        }
        let cmd = match crate::app::parse_assessment_cmd(args) {
            Ok(cmd) => cmd,
            Err(()) => {
                self.toast("usage: /hacker assessment [list|use <id>|complete [<id>]]");
                self.mark_dirty();
                return;
            }
        };
        let path = FindingsStore::default_path();
        let tx = self.hacker_tx.clone();
        match cmd {
            crate::app::AssessmentCmd::List => {
                self.handle.spawn_blocking(move || {
                    let text = crate::app::render_assessment_list(&path);
                    let _ = tx.send(crate::app::HackerEvent::Lines {
                        request: "assessments".into(),
                        text,
                    });
                });
                self.toast("loading assessments ...");
            }
            crate::app::AssessmentCmd::Use(id) => {
                let label = id.clone();
                let label2 = label.clone();
                self.handle.spawn_blocking(move || {
                    let text = match FindingsStore::open(&path) {
                        Err(err) => format!("assessment use failed: {err}"),
                        Ok(store) => match store.load_assessment(&id) {
                            Err(err) => format!("assessment use failed: {err}"),
                            Ok(Some(spec)) => {
                                let _ = tx.send(crate::app::HackerEvent::Resume(Box::new(spec)));
                                return;
                            }
                            Ok(None) => format!("no assessment `{id}`"),
                        },
                    };
                    let _ = tx.send(crate::app::HackerEvent::Lines {
                        request: format!("assessment use {label2}"),
                        text,
                    });
                });
                self.toast(format!("loading assessment {label} ..."));
            }
            crate::app::AssessmentCmd::Complete(id) => {
                let id = id.or_else(|| self.hacker.assessment.as_ref().map(|c| c.id.clone()));
                let Some(id) = id else {
                    self.toast("no active assessment to complete");
                    self.mark_dirty();
                    return;
                };
                let label = id.clone();
                self.handle.spawn_blocking(move || {
                    let done = match FindingsStore::open(&path) {
                        Err(err) => {
                            let _ = tx.send(crate::app::HackerEvent::Lines {
                                request: "assessment complete".into(),
                                text: format!("assessment complete failed: {err}"),
                            });
                            return;
                        }
                        Ok(store) => store
                            .set_assessment_status(&id, crate::hacker::AssessmentStatus::Completed)
                            .unwrap_or(false),
                    };
                    if done {
                        let _ = tx.send(crate::app::HackerEvent::Completed(id));
                    } else {
                        let _ = tx.send(crate::app::HackerEvent::Lines {
                            request: "assessment complete".into(),
                            text: format!("no assessment `{id}`"),
                        });
                    }
                });
                self.toast(format!("completing assessment {label} ..."));
            }
        }
        self.mark_dirty();
    }

    /// `/hacker report [md|json|sarif]`: writes a report and prints where it went.
    pub(crate) fn run_hacker_report(&mut self, args: &str) {
        if !self.cfg.hacker.store {
            self.toast("findings store is disabled - set `[hacker] store = true`");
            self.mark_dirty();
            return;
        }
        let Some(format) = ReportFormat::parse(args) else {
            self.toast("usage: /hacker report [md|json|sarif]");
            self.mark_dirty();
            return;
        };
        let path = FindingsStore::default_path();
        let dir = crate::hacker::store::reports_dir();
        let assessment = self.hacker.assessment.as_ref().map(|c| c.id.clone());
        let tx = self.hacker_tx.clone();
        let request = format!("report {}", format.extension());
        self.handle.spawn_blocking(move || {
            let text = match FindingsStore::open(&path) {
                Err(err) => format!("hacker report failed: {err}"),
                Ok(store) => match store.export(&dir, assessment.as_deref(), format) {
                    Ok(path) => format!("hacker report written: {}", path.display()),
                    Err(err) => format!("hacker report failed: {err}"),
                },
            };
            let _ = tx.send(crate::app::HackerEvent::Lines { request, text });
        });
        self.toast("writing hacker report ...");
        self.mark_dirty();
    }

    /// Appends a finished hacker job to the transcript. Failures (including
    /// authorization denials) are shown verbatim with their stable code.
    pub(crate) fn apply_hacker_event(&mut self, event: crate::app::HackerEvent) {
        match event {
            crate::app::HackerEvent::Report { request, result } => {
                let text = match result {
                    Ok(report) => report.render().join("\n"),
                    Err(err) => format!("hacker {request} failed: {err}"),
                };
                self.entries.push(Entry::new(EntryRole::System, text));
            }
            crate::app::HackerEvent::Lines { request, text } => {
                self.entries.push(Entry::new(
                    EntryRole::System,
                    format!("[hacker:{request}] {text}"),
                ));
            }
            crate::app::HackerEvent::Resume(spec) => {
                if let Err(e) = self.hacker.resume(&spec, SystemTime::now()) {
                    self.toast(format!("assessment resume failed: {e}"));
                } else {
                    self.toast(format!("assessment {} resumed", spec.id));
                }
                self.mark_dirty();
            }
            crate::app::HackerEvent::Completed(id) => {
                self.toast(format!("assessment {id} completed"));
                self.mark_dirty();
            }
            // New typestate capability events: recorded in audit trail.
            crate::app::HackerEvent::OperationComplete {
                module,
                op,
                target,
                ok: _,
            } => {
                let _ = (module, op, target);
            }
            crate::app::HackerEvent::OperationDenied { module, op, reason } => {
                let _ = (module, op, reason);
            }
        }
    }

    // -- LEARNING -----------------------------------------------------------

    /// `/learn`: daily learning cadence entry point.
    pub(crate) fn run_learn(&mut self, args: &str) {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            self.toast("usage: /learn <topic> | status | digest | daily on|off");
            self.mark_dirty();
            return;
        }
        self.toast(format!("learning: {trimmed}"));
        self.mark_dirty();
    }
}
