//! Self-improvement tools: the agent-facing surface of the self-edit engine.
//!
//! Two tools, deliberately not three: `propose_self_edit` stages a change and
//! `list_self_edits` reports what is staged. There is **no** agent-callable
//! apply — approval is a human action in the `/improve` overlay, and the engine
//! refuses to apply anything the user has not seen. Argument parsing, path
//! confinement and the size cap live in the engine; this module
//! only adapts JSON in and bounded JSON out.
//!
//! Sync by design: the runtime dispatches every tool call through
//! `tokio::task::spawn_blocking`, so the staging work never touches the UI
//! thread.

use crate::agent::provider::Tool;
use crate::improve::{
    unified_diff, Actor, ImproveEngine, ImproveError, Proposal, Roots, Status, Target,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Bounds on the diff echoed back to the model. They mirror the transcript's
/// tool-preview bounds (`PREVIEW_MAX_LINES` / `PREVIEW_MAX_CHARS` in
/// `agent::runtime`) so a proposal payload can never flood the context window.
const DIFF_PREVIEW_MAX_LINES: usize = 12;
const DIFF_PREVIEW_MAX_CHARS: usize = 1_600;

/// Longest rationale echoed back per proposal in the list payload.
const RATIONALE_PREVIEW_CHARS: usize = 160;

/// Hard cap on how many proposals one `list_self_edits` call reports.
const MAX_LISTED: usize = 50;

// -- arguments ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposeSelfEditArgs {
    /// One of: config, prompt, source.
    pub target: String,
    /// File to edit. Relative paths are resolved against the workspace for
    /// `source`; `config` and `prompt` must name the app's own anchor files.
    pub path: String,
    /// The complete replacement file content, not a patch.
    pub new_content: String,
    /// One line on why the change helps; shown to the user in `/improve`.
    pub rationale: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListSelfEditsArgs {}

// -- results ------------------------------------------------------------------

/// One staged proposal as the model sees it: identity, target, size and the
/// bounded diff preview the user will be asked to review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposeResult {
    pub id: String,
    pub status: String,
    pub target: String,
    pub path: String,
    pub added: usize,
    pub removed: usize,
    /// Bounded unified diff of the staged change.
    pub diff_preview: String,
    /// Always set: the truth about what happens next.
    pub notice: String,
}

/// One row of the pending-proposal list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalSummary {
    pub id: String,
    pub target: String,
    pub path: String,
    pub rationale: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResult {
    pub proposals: Vec<ProposalSummary>,
    pub total: usize,
}

// -- operations ---------------------------------------------------------------

/// `propose_self_edit`: stage a self-edit for human review. Applies every
/// safety rule the engine knows; a refusal is returned as an error payload the
/// model can read and adapt to.
pub fn propose(
    engine: &ImproveEngine,
    args: ProposeSelfEditArgs,
    roots: &Roots,
) -> Result<ProposeResult, ImproveError> {
    let target = parse_target(&args.target)?;
    if args.rationale.trim().is_empty() {
        return Err(ImproveError::invalid("a rationale is required"));
    }
    let path = std::path::PathBuf::from(&args.path);
    let proposal = engine.propose(
        Actor::Agent,
        target,
        path,
        args.new_content,
        args.rationale,
        roots,
    )?;
    // The engine already persisted the diff; re-derive a bounded preview of the
    // same change so the model sees what the user will be shown.
    let current = std::fs::read_to_string(&proposal.path).unwrap_or_default();
    let full = unified_diff(
        &current,
        &proposal.new_content,
        &proposal.path.to_string_lossy(),
    );
    let (added, removed) = count_changes(&full);
    Ok(ProposeResult {
        id: proposal.id,
        status: proposal.status.as_str().to_string(),
        target: proposal.target.as_str().to_string(),
        path: proposal.path.to_string_lossy().to_string(),
        added,
        removed,
        diff_preview: bounded_preview(&full),
        notice: "staged for user review - the user approves changes in /improve; nothing is applied automatically.".to_string(),
    })
}

/// `list_self_edits`: the staged proposals still waiting for a decision.
pub fn list_pending(engine: &ImproveEngine) -> Result<ListResult, ImproveError> {
    let proposals = engine.list(Some(Status::Pending))?;
    let total = proposals.len();
    let rows = proposals
        .into_iter()
        .take(MAX_LISTED)
        .map(|p| summary(&p))
        .collect();
    Ok(ListResult {
        proposals: rows,
        total,
    })
}

/// Compacts a proposal into the bounded row the list payload (and the `/improve`
/// overlay header) uses.
pub fn summary(proposal: &Proposal) -> ProposalSummary {
    ProposalSummary {
        id: proposal.id.clone(),
        target: proposal.target.as_str().to_string(),
        path: proposal.path.to_string_lossy().to_string(),
        rationale: bounded_line(&proposal.rationale, RATIONALE_PREVIEW_CHARS),
        created_at: proposal.created_at.clone(),
    }
}

// -- helpers ------------------------------------------------------------------

pub fn parse_target(raw: &str) -> Result<Target, ImproveError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "config" => Ok(Target::Config),
        "prompt" => Ok(Target::Prompt),
        "source" => Ok(Target::Source),
        other => Err(ImproveError::invalid(format!(
            "unknown target {other:?} (expected one of: config, prompt, source)"
        ))),
    }
}

/// Counts added/removed content lines, ignoring the `---`/`+++` file headers.
pub fn count_changes(diff: &str) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    for line in diff.lines() {
        if line.starts_with("+++ ") || line.starts_with("--- ") {
            continue;
        }
        if line.starts_with('+') {
            added += 1;
        } else if line.starts_with('-') {
            removed += 1;
        }
    }
    (added, removed)
}

/// Trims a diff to the preview bounds, marking what was hidden.
pub fn bounded_preview(diff: &str) -> String {
    let lines: Vec<&str> = diff.lines().collect();
    let mut out = String::new();
    for line in lines.iter().take(DIFF_PREVIEW_MAX_LINES) {
        if out.len() >= DIFF_PREVIEW_MAX_CHARS {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        let room = DIFF_PREVIEW_MAX_CHARS - out.len();
        out.push_str(&crate::agent::runtime::shorten(
            line,
            room.saturating_sub(1),
        ));
    }
    let hidden = lines.len().saturating_sub(DIFF_PREVIEW_MAX_LINES);
    if hidden > 0 {
        out.push_str(&format!("\n… +{hidden} more lines"));
    }
    out
}

fn bounded_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!(
            "{}…",
            flat.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

// -- schemas ------------------------------------------------------------------

/// JSON schemas for the two self-improvement tools, appended to the runtime's
/// tool list only when the engine is attached.
pub fn improve_tool_schemas() -> Vec<Tool> {
    vec![
        Tool::new(
            "propose_self_edit",
            "Stage a proposed edit to your own config, prompt file or a workspace source file for the user to review in /improve. This does not change anything: the user approves or rejects it. Send the complete replacement file content plus a one-line rationale.",
            json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "enum": ["config", "prompt", "source"],
                        "description": "Which file kind is being edited"
                    },
                    "path": {
                        "type": "string",
                        "description": "File to edit; relative paths resolve against the workspace for a source target"
                    },
                    "new_content": {
                        "type": "string",
                        "description": "The complete replacement file content"
                    },
                    "rationale": {
                        "type": "string",
                        "description": "One line on why this change helps the user"
                    }
                },
                "required": ["target", "path", "new_content", "rationale"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "list_self_edits",
            "List self-edit proposals still waiting for the user's decision, newest first by id.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::improve::testing::temp_dir;
    use std::fs;

    fn roots(root: &std::path::Path) -> Roots {
        Roots {
            config_path: root.join("config.toml"),
            prompt_path: root.join("prompt.md"),
            repo_root: root.to_path_buf(),
        }
    }

    #[test]
    fn schemas_are_the_two_agent_visible_tools() {
        let names: Vec<String> = improve_tool_schemas()
            .into_iter()
            .map(|t| t.function.name)
            .collect();
        assert_eq!(names, vec!["propose_self_edit", "list_self_edits"]);
    }

    #[test]
    fn target_parsing_is_total_and_strict() {
        assert_eq!(parse_target("CONFIG").unwrap(), Target::Config);
        assert_eq!(parse_target(" prompt ").unwrap(), Target::Prompt);
        assert_eq!(parse_target("source").unwrap(), Target::Source);
        assert!(parse_target("memory").is_err());
        assert!(parse_target("").is_err());
    }

    #[test]
    fn count_changes_ignores_file_headers() {
        let diff = "--- a/x.rs\n+++ b/x.rs\n@@ -1 +1 @@\n-old\n+new\n";
        assert_eq!(count_changes(diff), (1, 1));
    }

    #[test]
    fn propose_stages_a_source_edit_without_touching_the_file() {
        let temp = temp_dir("propose-source");
        let root = &temp.path;
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        let engine = ImproveEngine::new(root.join(".improve"));

        let result = propose(
            &engine,
            ProposeSelfEditArgs {
                target: "source".into(),
                path: "src/lib.rs".into(),
                new_content: "pub fn b() {}\n".into(),
                rationale: "rename the entry point".into(),
            },
            &roots(root),
        )
        .unwrap();

        assert_eq!(result.target, "source");
        assert_eq!((result.added, result.removed), (1, 1));
        assert!(result.diff_preview.contains("-pub fn a() {}"));
        assert!(result.diff_preview.contains("+pub fn b() {}"));
        assert!(
            result.notice.contains("staged for user review"),
            "the model must be told nothing is applied automatically"
        );
        // The file itself is untouched until the user approves.
        assert_eq!(
            fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "pub fn a() {}\n"
        );
    }

    #[test]
    fn propose_requires_a_rationale() {
        let temp = temp_dir("propose-no-rationale");
        let root = &temp.path;
        fs::write(root.join("prompt.md"), "hi\n").unwrap();
        let engine = ImproveEngine::new(root.join(".improve"));
        let err = propose(
            &engine,
            ProposeSelfEditArgs {
                target: "prompt".into(),
                path: root.join("prompt.md").display().to_string(),
                new_content: "bye\n".into(),
                rationale: "   ".into(),
            },
            &roots(root),
        )
        .unwrap_err();
        assert!(matches!(err, ImproveError::Invalid(_)), "{err:?}");
    }

    #[test]
    fn safety_layer_targets_stage_after_denylist_removal() {
        // Owner decision 2026-10-30: the self-edit denylist is gone. The tool
        // stages the proposal; applying still requires the /improve gate.
        let temp = temp_dir("propose-safety-stage");
        let root = &temp.path;
        fs::create_dir_all(root.join("src/memory")).unwrap();
        fs::write(root.join("src/memory/policy.rs"), "pub fn safe() {}\n").unwrap();
        let engine = ImproveEngine::new(root.join(".improve"));
        let _staged = propose(
            &engine,
            ProposeSelfEditArgs {
                target: "source".into(),
                path: "src/memory/policy.rs".into(),
                new_content: "pub fn edited() {}\n".into(),
                rationale: "owner-approved policy change".into(),
            },
            &roots(root),
        )
        .unwrap();
        assert_eq!(list_pending(&engine).unwrap().proposals.len(), 1);
    }

    #[test]
    fn list_reports_pending_only_and_is_empty_to_begin_with() {
        let temp = temp_dir("list-pending");
        let root = &temp.path;
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        let engine = ImproveEngine::new(root.join(".improve"));
        assert!(list_pending(&engine).unwrap().proposals.is_empty());

        let first = propose(
            &engine,
            ProposeSelfEditArgs {
                target: "source".into(),
                path: "src/lib.rs".into(),
                new_content: "pub fn b() {}\n".into(),
                rationale: "first\nsecond line".into(),
            },
            &roots(root),
        )
        .unwrap();
        let listed = list_pending(&engine).unwrap();
        assert_eq!(listed.total, 1);
        assert_eq!(listed.proposals[0].id, first.id);
        // Only the first rationale line survives.
        assert_eq!(listed.proposals[0].rationale, "first second line");

        engine.reject(&first.id, Actor::User).unwrap();
        assert!(list_pending(&engine).unwrap().proposals.is_empty());
    }

    #[test]
    fn diff_preview_is_bounded_and_marked() {
        let long: String = (0..50).map(|i| format!("+line {i}\n")).collect();
        let preview = bounded_preview(&long);
        assert!(preview.lines().count() <= DIFF_PREVIEW_MAX_LINES + 1);
        assert!(preview.len() <= DIFF_PREVIEW_MAX_CHARS + 32);
        assert!(preview.contains("more lines"), "{preview}");
    }
}
