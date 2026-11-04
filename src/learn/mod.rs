//! Daily learning loop: run a research turn on a topic, persist the durable
//! lessons in memory, and stage concrete improvements for review.
//!
//! This module is the *pure* side of the loop: cadence state, topic selection
//! and the instruction text handed to the agent. It performs no network I/O and
//! never touches the model itself — the app starts a normal turn with
//! [`learning_prompt`], and the agent's existing `memory_*` and
//! `propose_self_edit` tools do the rest. `propose_self_edit` only stages;
//! approval still happens in `/improve` (invariant: no agent-callable apply).
//!
//! State is a small JSON file inside a caller-supplied data directory, so tests
//! never touch real user dirs.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::TimeZone;

/// Memory tag marking a stored lesson. `/learn digest` and the hacker
/// `techniques` recall read records by tag.
pub const LEARNING_TAG: &str = "learning";
/// Secondary tag for reusable tradecraft, shared with the hacker knowledge base.
pub const TECHNIQUE_TAG: &str = "technique";

/// Curated fallback topics, rotated one per day. Used when the user enables
/// daily learning without pinning a topic.
pub const DEFAULT_TOPICS: &[&str] = &[
    "AI agent architectures and tool-use patterns",
    "context engineering and memory for long-running coding agents",
    "agent evaluation, regression testing and reliability",
    "prompt and instruction design for long-horizon autonomous tasks",
    "safe autonomous tool execution and permission models",
    "multi-agent collaboration, review and critique workflows",
];

/// Persisted cadence state. `last_day` is a local calendar date (`YYYY-MM-DD`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LearningState {
    #[serde(default)]
    pub last_day: Option<String>,
    #[serde(default)]
    pub streak: u32,
    #[serde(default)]
    pub total_sessions: u32,
    #[serde(default)]
    pub last_topic: Option<String>,
}

/// Cadence engine rooted at one data directory.
#[derive(Debug, Clone)]
pub struct LearningEngine {
    data_dir: PathBuf,
}

impl LearningEngine {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn state_path(&self) -> PathBuf {
        self.data_dir.join("state.json")
    }

    /// Loads the cadence state; a missing or corrupt file is simply "never ran".
    pub fn state(&self) -> LearningState {
        fs::read_to_string(self.state_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save_state(&self, state: &LearningState) -> io::Result<()> {
        fs::create_dir_all(&self.data_dir)?;
        let path = self.state_path();
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// True when no session has run on `now`'s local calendar day.
    pub fn due(&self, now_secs: i64) -> bool {
        self.state().last_day.as_deref() != Some(day(now_secs).as_str())
    }

    /// Records that a session started now. The streak advances when the last
    /// session was the previous calendar day, and resets otherwise.
    pub fn mark_session(&self, now_secs: i64, topic: &str) -> LearningState {
        let today = day(now_secs);
        let yesterday = day(now_secs - 86_400);
        let mut state = self.state();
        if state.last_day.as_deref() == Some(today.as_str()) {
            // Already counted today: refresh the topic but not the counters.
            state.last_topic = Some(topic.to_string());
            let _ = self.save_state(&state);
            return state;
        }
        state.streak = if state.last_day.as_deref() == Some(yesterday.as_str()) {
            state.streak.saturating_add(1)
        } else {
            1
        };
        state.total_sessions = state.total_sessions.saturating_add(1);
        state.last_day = Some(today);
        state.last_topic = Some(topic.to_string());
        let _ = self.save_state(&state);
        state
    }

    /// The topic for today: the pinned topic when set, else a deterministic
    /// rotation through [`DEFAULT_TOPICS`] by calendar day.
    pub fn topic_for(&self, now_secs: i64, pinned: Option<&str>) -> Option<String> {
        if let Some(topic) = pinned.map(str::trim).filter(|t| !t.is_empty()) {
            return Some(topic.to_string());
        }
        if DEFAULT_TOPICS.is_empty() {
            return None;
        }
        let index = (now_secs.div_euclid(86_400)).rem_euclid(DEFAULT_TOPICS.len() as i64) as usize;
        Some(DEFAULT_TOPICS[index].to_string())
    }
}

/// Local calendar date (`YYYY-MM-DD`) for a unix timestamp.
pub fn day(now_secs: i64) -> String {
    chrono::Local
        .timestamp_opt(now_secs, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| now_secs.to_string())
}

/// Lowercase, dash-separated slug safe for a memory tag (bounded length).
pub fn slug(topic: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in topic.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 32 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "general".to_string()
    } else {
        out
    }
}

/// The tags every lesson saved by a learning session carries.
pub fn lesson_tags(topic: &str) -> Vec<String> {
    vec![
        LEARNING_TAG.to_string(),
        TECHNIQUE_TAG.to_string(),
        slug(topic),
    ]
}

/// One-line status for `/learn status`.
pub fn status_line(state: &LearningState) -> String {
    let last = state.last_day.as_deref().unwrap_or("never");
    let topic = state.last_topic.as_deref().unwrap_or("-");
    format!(
        "daily learning: last {last} · streak {} day(s) · {} session(s) · last topic: {topic}",
        state.streak, state.total_sessions
    )
}

/// The instruction text that starts a learning turn. It is a normal user
/// message: the agent uses its own tools (`memory_search`, `memory_save`,
/// `propose_self_edit`) to research, persist and stage improvements.
pub fn learning_prompt(topic: &str) -> String {
    format!(
        "# Daily learning session\n\n\
Topic: {topic}\n\n\
Goal: extract durable, actionable knowledge that makes nanomite a better coding \
agent, then turn the strongest ideas into concrete, reviewable improvements.\n\n\
Workflow:\n\
1. Recall first. Use memory_search with tags `{learning}` (and `{technique}`) so \
you build on prior lessons instead of repeating them.\n\
2. Research the topic with the tools available to you (web_fetch, browse, \
read_document, run_command for local docs). Prefer primary sources and concrete \
engineering write-ups over summaries.\n\
3. Extract 3-7 lessons. For each, call memory_save with:\n\
   - category: project\n\
   - tags: [\"{learning}\", \"{technique}\", \"{slug}\"]\n\
   - content: \"Technique: ...\\nWhy: ...\\nApply to nanomite: ...\\nSource: ...\"\n\
   Save durable technique only. Never store credentials, secrets or raw page dumps.\n\
4. For each idea with a concrete code or config change, call propose_self_edit to \
stage it. Proposals are inert until the user approves them in /improve; never claim \
a change is applied.\n\
5. Finish with a short digest: what you learned, what you staged, and the single \
highest-value next experiment.\n\n\
If the topic cannot be researched with the available tools, say so plainly and \
still record any lessons you can support.",
        topic = topic.trim(),
        learning = LEARNING_TAG,
        technique = TECHNIQUE_TAG,
        slug = slug(topic),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("nanomite-learn-{tag}-{}-{n}", std::process::id()));
            fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn now() -> i64 {
        chrono::Local::now().timestamp()
    }

    #[test]
    fn due_is_true_until_a_session_runs_today() {
        let dir = TempDir::new("due");
        let engine = LearningEngine::new(&dir.0);
        let ts = now();
        assert!(engine.due(ts), "never ran");
        engine.mark_session(ts, "topic a");
        assert!(!engine.due(ts), "ran today");
        assert!(engine.due(ts + 86_400), "next day is due again");
    }

    #[test]
    fn streak_advances_across_consecutive_days_and_resets_on_a_gap() {
        let dir = TempDir::new("streak");
        let engine = LearningEngine::new(&dir.0);
        let today = now();
        let yesterday = today - 86_400;

        // Seed yesterday, then today → streak 2.
        engine.mark_session(yesterday, "day 1");
        let state = engine.mark_session(today, "day 2");
        assert_eq!(state.streak, 2);
        assert_eq!(state.total_sessions, 2);

        // A session on the same day refreshes the topic but not the counters.
        let state = engine.mark_session(today, "day 2 again");
        assert_eq!(state.streak, 2);
        assert_eq!(state.total_sessions, 2);
        assert_eq!(state.last_topic.as_deref(), Some("day 2 again"));

        // A two-day gap resets the streak.
        let state = engine.mark_session(today + 2 * 86_400, "day 4");
        assert_eq!(state.streak, 1);
        assert_eq!(state.total_sessions, 3);
    }

    #[test]
    fn pinned_topic_wins_and_rotation_is_deterministic() {
        let dir = TempDir::new("topic");
        let engine = LearningEngine::new(&dir.0);
        let ts = now();
        assert_eq!(
            engine.topic_for(ts, Some("rust async")).as_deref(),
            Some("rust async")
        );
        assert_eq!(
            engine.topic_for(ts, Some("   ")),
            engine.topic_for(ts, None)
        );
        assert_eq!(engine.topic_for(ts, None), engine.topic_for(ts, None));
        assert!(engine.topic_for(ts, None).is_some());
    }

    #[test]
    fn slug_is_tag_safe_and_bounded() {
        assert_eq!(slug("AI Agents: tools & memory!"), "ai-agents-tools-memory");
        assert_eq!(slug("  ---  "), "general");
        assert!(slug(&"x".repeat(200)).len() <= 32);
        assert_eq!(
            lesson_tags("AI Agents"),
            vec!["learning", "technique", "ai-agents"]
        );
    }

    #[test]
    fn prompt_carries_topic_and_tags() {
        let prompt = learning_prompt("agent evaluation");
        assert!(prompt.contains("agent evaluation"));
        assert!(prompt.contains("learning"));
        assert!(prompt.contains("technique"));
        assert!(prompt.contains("agent-evaluation"));
        assert!(prompt.contains("propose_self_edit"));
        assert!(prompt.contains("memory_save"));
    }

    #[test]
    fn status_line_distinguishes_never_ran() {
        assert!(status_line(&LearningState::default()).contains("never"));
        let state = LearningState {
            last_day: Some("2026-11-04".into()),
            streak: 3,
            total_sessions: 7,
            last_topic: Some("agents".into()),
        };
        let line = status_line(&state);
        assert!(line.contains("2026-11-04"));
        assert!(line.contains("streak 3"));
    }
}
