//! Session persistence and transcript export.

use crate::app::{Entry, EntryRole, Phase};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub version: u32,
    pub started: u64,
    pub updated: u64,
    pub provider: String,
    pub model: String,
    pub cwd: String,
    pub entries: Vec<StoredEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEntry {
    pub role: String,
    pub text: String,
    pub tools: Vec<String>,
    pub elapsed_ms: Option<u64>,
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
            version: 1,
            started: unix_secs(started),
            updated: now,
            provider: provider.to_string(),
            model: model.to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
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
    let path = dir.join(format!("{stamp}-{slug}.json"));
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
    fn markdown_export_has_headers_and_tool_rows() {
        let md = to_markdown(&sample());
        assert!(md.contains("# nanomite session"));
        assert!(md.contains("## You"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("`list_files src · 3 files`"));
    }
}
