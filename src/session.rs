//! Session persistence and transcript export.

use crate::app::{Entry, EntryRole, Phase};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub version: u32,
    pub started: u64,
    pub updated: u64,
    pub provider: String,
    pub model: String,
    pub cwd: String,
    pub entries: Vec<StoredEntry>,
    /// Workspace slug (v2): lets the /sessions picker group by project.
    #[serde(default)]
    pub workspace: Option<String>,
    /// Session settings snapshot (v2): tool policy, disabled tools,
    /// reasoning effort, theme - restored with the conversation.
    #[serde(default)]
    pub settings: Option<StoredSettings>,
    /// Plan snapshots (v2): every plan touched in this session, with task
    /// statuses, so resuming restores the work breakdown too.
    #[serde(default)]
    pub plans: Vec<StoredPlan>,
}

/// Settings that travel with a session (v2). All optional so v1 files load.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredSettings {
    #[serde(default)]
    pub tool_policy: Option<String>,
    #[serde(default)]
    pub disabled_tools: Option<Vec<String>>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
}

/// One plan as persisted inside a session (v2).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredPlan {
    pub name: String,
    pub description: String,
    pub updated_at: String,
    pub tasks: Vec<StoredPlanTask>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredPlanTask {
    pub id: usize,
    pub description: String,
    pub status: String,
    pub depends_on: Vec<usize>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEntry {
    pub role: String,
    pub text: String,
    pub tools: Vec<String>,
    pub elapsed_ms: Option<u64>,
    /// Wall-clock creation time (unix seconds); `None` in files written
    /// before the field existed, so old sessions still restore.
    #[serde(default)]
    pub at: Option<u64>,
}

impl StoredSession {
    pub fn capture(
        entries: &[Entry],
        provider: &str,
        model: &str,
        cwd: &Path,
        started: SystemTime,
    ) -> Self {
        let now = now_secs();
        Self {
            version: 2,
            started: unix_secs(started),
            updated: now,
            provider: provider.to_string(),
            model: model.to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            workspace: Some(workspace_slug(cwd)),
            settings: None,
            plans: Vec::new(),
            entries: entries
                .iter()
                .filter(|e| !e.text.trim().is_empty() || !e.tools.is_empty())
                .map(|e| StoredEntry {
                    role: match e.role {
                        EntryRole::User => "user".into(),
                        EntryRole::Assistant => "assistant".into(),
                        EntryRole::System => "system".into(),
                    },
                    text: e.text.clone(),
                    tools: e
                        .tools
                        .iter()
                        .map(|t| {
                            if t.ok {
                                format!("{} {}", t.name, t.detail)
                            } else {
                                format!("{} {} (failed)", t.name, t.detail)
                            }
                        })
                        .collect(),
                    elapsed_ms: e.elapsed.map(|d| d.as_millis() as u64),
                    at: Some(unix_secs(e.at)),
                })
                .collect(),
        }
    }

    pub fn restore(&self) -> Vec<Entry> {
        self.entries
            .iter()
            .map(|e| {
                let role = match e.role.as_str() {
                    "user" => EntryRole::User,
                    "assistant" => EntryRole::Assistant,
                    _ => EntryRole::System,
                };
                let mut entry = Entry::new(role, e.text.clone());
                entry.phase = match role {
                    EntryRole::Assistant => Phase::Done,
                    _ => Phase::Done,
                };
                entry.elapsed = e.elapsed_ms.map(std::time::Duration::from_millis);
                // Sub-second precision is not stored; unix seconds round-trip
                // exactly, and files without the field fall back to "now".
                entry.at =
                    e.at.and_then(|secs| {
                        SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(secs))
                    })
                    .unwrap_or_else(SystemTime::now);
                // The session-level model applies to every restored entry.
                entry.model = Some(self.model.clone());
                for tool in &e.tools {
                    let (name, detail) = tool.split_once(' ').unwrap_or((tool.as_str(), ""));
                    entry.tools.push(crate::app::ToolRow {
                        name: name.to_string(),
                        detail: detail.trim_end_matches(" (failed)").to_string(),
                        running: false,
                        ok: !tool.ends_with("(failed)"),
                        edit_path: None,
                        edit_replacements: None,
                        preview: None,
                        started: Instant::now(),
                        elapsed: None,
                    });
                }
                entry
            })
            .collect()
    }

    pub fn turns(&self) -> usize {
        self.entries.iter().filter(|e| e.role == "user").count()
    }
}

/// One `/history` match: which saved session, and the line that matched.
#[derive(Debug, Clone)]
pub struct HistoryHit {
    pub path: PathBuf,
    /// `model · turns · workspace` for the picker's hint column.
    pub label: String,
    /// The matched line, trimmed for display.
    pub snippet: String,
}

/// Scans saved sessions (newest first) for `query`, case-insensitively, one
/// hit per session so the results span many conversations. Bounded by
/// `limit` and by file size (sessions are small JSON files).
pub fn search_saved(query: &str, limit: usize) -> Vec<HistoryHit> {
    search_saved_in(sessions_dir(), query, limit)
}

/// The directory-injectable core of [`search_saved`], so tests can point it
/// at a scratch directory instead of the real session store.
pub fn search_saved_in(dir: PathBuf, query: &str, limit: usize) -> Vec<HistoryHit> {
    let needle = query.to_lowercase();
    let mut out: Vec<HistoryHit> = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let metas = {
        let mut metas: Vec<SessionMeta> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|entry| {
                let path = entry.path();
                let raw = std::fs::read(&path).ok()?;
                let stored: StoredSession = serde_json::from_slice(&raw).ok()?;
                let preview = stored
                    .entries
                    .iter()
                    .find(|e| e.role == "user")
                    .map(|e| e.text.trim().replace('\n', " "))
                    .unwrap_or_default();
                let turns = stored.turns();
                Some(SessionMeta {
                    updated: stored.updated,
                    path,
                    label: stored.model.clone(),
                    turns,
                    preview,
                    workspace: stored.workspace.clone(),
                })
            })
            .collect();
        metas.sort_by_key(|meta| std::cmp::Reverse(meta.updated));
        metas
    };
    for meta in metas {
        if out.len() >= limit {
            break;
        }
        let Ok(raw) = std::fs::read(&meta.path) else {
            continue;
        };
        if raw.len() > 4_194_304 {
            continue;
        }
        let Ok(stored) = serde_json::from_slice::<StoredSession>(&raw) else {
            continue;
        };
        let label = format!(
            "{} · {} turns · {}",
            meta.label,
            meta.turns,
            meta.workspace.unwrap_or_else(|| "other".into())
        );
        for entry in &stored.entries {
            if let Some(line) = entry
                .text
                .lines()
                .find(|l| l.to_lowercase().contains(&needle))
            {
                out.push(HistoryHit {
                    path: meta.path.clone(),
                    label: label.clone(),
                    snippet: line.trim().chars().take(80).collect(),
                });
                break;
            }
        }
    }
    out
}

/// Slugified workspace path: the per-project grouping key for the sessions
/// picker. Sessions still live in the data dir (never inside the repo - a
/// `.nanomite/sessions/` directory would risk being committed); grouping is
/// per workspace via this slug.
pub fn workspace_slug(cwd: &Path) -> String {
    cwd.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| cwd.to_string_lossy().into_owned())
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// `~/.local/share/nanomite/sessions` (or the platform equivalent).
pub fn sessions_dir() -> PathBuf {
    let base = dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("nanomite").join("sessions")
}

/// Writes a session and returns the file path.
pub fn save(session: &StoredSession) -> std::io::Result<PathBuf> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)?;
    let stamp = session.updated;
    let slug: String = session
        .model
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(24)
        .collect();
    // Sub-second nanos keep concurrent saves from racing on one filename
    // (two sessions in the same second with the same model would otherwise
    // check-then-write over each other).
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("{stamp}{nanos:09}-{slug}.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(session)?)?;
    Ok(path)
}

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub path: PathBuf,
    pub updated: u64,
    pub label: String,
    pub turns: usize,
    pub preview: String,
    /// Workspace slug the session ran in (v2 sessions only).
    pub workspace: Option<String>,
}

/// Reads every saved session, newest first.
pub fn list() -> Vec<SessionMeta> {
    let Ok(dir) = std::fs::read_dir(sessions_dir()) else {
        return Vec::new();
    };
    let mut out: Vec<SessionMeta> = dir
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|entry| {
            let path = entry.path();
            let raw = std::fs::read(&path).ok()?;
            let session: StoredSession = serde_json::from_slice(&raw).ok()?;
            let preview = session
                .entries
                .iter()
                .find(|e| e.role == "user")
                .map(|e| e.text.trim().replace('\n', " "))
                .unwrap_or_default();
            Some(SessionMeta {
                path,
                updated: session.updated,
                label: session.model.clone(),
                turns: session.turns(),
                preview: preview.chars().take(60).collect(),
                workspace: session.workspace.clone(),
            })
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.updated));
    out
}

pub fn load(path: &Path) -> std::io::Result<StoredSession> {
    let raw = std::fs::read(path)?;
    serde_json::from_slice(&raw)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Renders the transcript as markdown for `/export` and `/share`.
pub fn to_markdown(session: &StoredSession) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "# nanomite session\n");
    let _ = writeln!(
        out,
        "- provider: `{}`\n- model: `{}`\n- workspace: `{}`\n- turns: {}\n",
        session.provider,
        session.model,
        session.cwd,
        session.turns()
    );

    let mut last_role = "";
    for entry in &session.entries {
        if entry.role != last_role {
            let _ = writeln!(out, "\n## {}\n", role_label(&entry.role));
            last_role = &entry.role;
        }
        for tool in &entry.tools {
            let _ = writeln!(out, "- `{tool}`");
        }
        if entry.text.trim().is_empty() {
            continue;
        }
        let _ = writeln!(out, "{}", entry.text.trim());
    }
    out
}

fn role_label(role: &str) -> &'static str {
    match role {
        "user" => "You",
        "assistant" => "Assistant",
        _ => "System",
    }
}

/// Default export location: `./nanomite-session-<stamp>.md`.
pub fn export_path() -> PathBuf {
    let stamp = now_secs();
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(format!("nanomite-session-{stamp}.md"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Starter `AGENTS.md` written by `/init` when the file does not exist yet.
pub const AGENTS_TEMPLATE: &str = "\
# AGENTS.md

Notes for AI agents working in this repository.

## Project
- Language / runtime:
- Build: 
- Test: 
- Lint: 

## Conventions
- Follow the style of the surrounding code.
- Keep changes minimal and focused.

## Gotchas
- <!-- things that repeatedly surprise you -->
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Entry;

    fn sample() -> StoredSession {
        let entries = vec![Entry::new(EntryRole::User, "list the files"), {
            let mut a = Entry::new(EntryRole::Assistant, "Here they are.");
            a.tools.push(crate::app::ToolRow {
                name: "list_files".into(),
                detail: "src · 3 files".into(),
                running: false,
                ok: true,
                edit_path: None,
                edit_replacements: None,
                preview: None,
                started: Instant::now(),
                elapsed: None,
            });
            a
        }];
        StoredSession::capture(
            &entries,
            "scripted",
            "scripted-model",
            Path::new("/tmp"),
            SystemTime::now(),
        )
    }

    #[test]
    fn round_trips_through_json() {
        let session = sample();
        let json = serde_json::to_string(&session).unwrap();
        let back: StoredSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back.turns(), 1);
        assert_eq!(back.entries.len(), 2);
        assert_eq!(back.entries[1].tools[0], "list_files src · 3 files");
    }

    #[test]
    fn restore_keeps_roles_and_tools() {
        let restored = sample().restore();
        assert_eq!(restored[0].role, EntryRole::User);
        assert_eq!(restored[1].role, EntryRole::Assistant);
        assert_eq!(restored[1].tools[0].name, "list_files");
        assert!(restored[1].tools[0].ok);
    }

    #[test]
    fn history_search_finds_matches_across_sessions() {
        let dir = crate::improve::testing::temp_dir("history-search");
        let mut stored = StoredSession {
            version: 2,
            started: 1_700_000_000,
            updated: 1_700_000_100,
            provider: "scripted".into(),
            model: "scripted-model".into(),
            cwd: "/tmp".into(),
            workspace: Some("demo".into()),
            settings: None,
            plans: Vec::new(),
            entries: vec![StoredEntry {
                role: "user".into(),
                text: "deploy on fridays please".into(),
                tools: vec![],
                elapsed_ms: None,
                at: Some(1_700_000_050),
            }],
        };
        std::fs::write(
            dir.path.clone().join("a.json"),
            serde_json::to_vec(&stored).unwrap(),
        )
        .unwrap();
        stored.entries[0].text = "unrelated note".into();
        std::fs::write(
            dir.path.clone().join("b.json"),
            serde_json::to_vec(&stored).unwrap(),
        )
        .unwrap();

        let hits = search_saved_in(dir.path.clone(), "fridays", 10);
        assert_eq!(hits.len(), 1, "only the matching session: {hits:?}");
        assert!(hits[0].snippet.contains("deploy on fridays"));
        assert_eq!(hits[0].label, "scripted-model · 1 turns · demo");
    }

    #[test]
    fn capture_round_trips_entry_wall_clock() {
        let at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let mut entry = Entry::new(EntryRole::User, "timestamped");
        entry.at = at;
        let session = StoredSession::capture(
            &[entry],
            "scripted",
            "scripted-model",
            Path::new("/tmp"),
            SystemTime::now(),
        );
        assert_eq!(session.entries[0].at, Some(1_700_000_000));
        let restored = session.restore();
        assert_eq!(restored[0].at, at, "wall-clock time must survive restore");
    }

    #[test]
    fn old_sessions_without_timestamps_restore() {
        let raw = r#"{
            "version": 2, "started": 0, "updated": 0,
            "provider": "p", "model": "m", "cwd": "/tmp",
            "entries": [
                {"role": "user", "text": "legacy", "tools": [], "elapsed_ms": null}
            ]
        }"#;
        let session: StoredSession = serde_json::from_str(raw).expect("legacy session loads");
        let restored = session.restore();
        assert_eq!(restored[0].text, "legacy");
        // No stored time: the badge falls back to the restore moment.
        assert!(restored[0].at.elapsed().is_ok());
    }

    #[test]
    fn markdown_export_has_headers_and_tool_rows() {
        let md = to_markdown(&sample());
        assert!(md.contains("# nanomite session"));
        assert!(md.contains("## You"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("`list_files src · 3 files`"));
    }
}
