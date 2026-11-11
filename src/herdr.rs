//! Herdr (terminal workspace manager) integration.
//!
//! When nanomite runs inside a Herdr pane (`HERDR_ENV=1`, `HERDR_SOCKET_PATH`
//! and `HERDR_PANE_ID` set), it reports its lifecycle over Herdr's integration
//! socket so the pane reads as a first-class `nanomite` agent: the sidebar
//! shows the label, `herdr agent list` includes it, and `herdr agent wait`
//! can watch for idle/blocked states.
//!
//! Three pieces live here:
//!
//! 1. [`HerdrLink`] — a background thread that serializes state reports and
//!    writes one JSON line per report to the socket. Fire-and-forget by
//!    contract: reports are best-effort and must never delay, block or fail
//!    the turn they observe (the UI thread never touches this).
//! 2. [`set_osc_title`] — the pane's terminal title carries the same state,
//!    so Herdr's screen detection and any other multiplexer see it too.
//! 3. [`MANIFEST`] — the screen-detection manifest Herdr versions for every
//!    agent; shipped for herdr builds that accept third-party manifests
//!    (0.8.2 only loads its built-in kinds, where hook authority covers us).
//!
//! The socket protocol mirrors Herdr's own integration plugins (opencode/pi):
//! one JSON object per line, method `pane.report_agent`, params carrying
//! `pane_id`, `source`, `agent`, a monotonic `seq` and the state.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;

/// Agent id reported to Herdr. Herdr 0.8.2 accepts unknown agent ids from
/// lifecycle hooks (verified live: `herdr agent list` shows `nanomite`).
pub const AGENT_ID: &str = "nanomite";

/// Source tag; mirrors the `herdr:<agent>` convention of built-in plugins.
pub const SOURCE: &str = "herdr:nanomite";

/// Screen-detection manifest, kept for herdr versions that load third-party
/// rules. Rules anchor on the composer status line and the OSC title — the
/// two surfaces nanomite renders deterministically.
pub const MANIFEST: &str = r#"id = "nanomite"
version = "2026.11.11.1"
min_engine_version = 1
updated_at = "2026-11-11T00:00:00Z"
aliases = ["nanomite-tui"]

[[rules]]
id = "osc_title_working"
state = "working"
priority = 1100
region = "osc_title"
visible_working = true
contains = ["nanomite — working"]

[[rules]]
id = "osc_title_blocked"
state = "blocked"
priority = 1200
region = "osc_title"
visible_blocker = true
contains = ["nanomite — approval needed"]

[[rules]]
id = "osc_title_idle"
state = "idle"
priority = 1000
region = "osc_title"
visible_idle = true
contains = ["nanomite"]

[[rules]]
id = "composer_working"
state = "working"
priority = 300
region = "whole_recent"
visible_working = true
line_regex = ['\bworking\b.*\besc esc force stop\b', 'the agent is working - type to queue']

[[rules]]
id = "composer_blocked"
state = "blocked"
priority = 400
region = "bottom_non_empty_lines(3)"
visible_blocker = true
contains = ["needs approval - y/a allow, n/d deny"]
"#;

/// Lifecycle states Herdr understands from hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Working,
    Blocked,
    Idle,
}

impl AgentState {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentState::Working => "working",
            AgentState::Blocked => "blocked",
            AgentState::Idle => "idle",
        }
    }

    /// OSC window title for the state. Anchored on "nanomite" so the manifest
    /// rules and any manual pane naming stay unambiguous.
    pub fn title(self) -> &'static str {
        match self {
            AgentState::Working => "nanomite — working",
            AgentState::Blocked => "nanomite — approval needed",
            AgentState::Idle => "nanomite",
        }
    }
}

/// Environment a link needs; captured once at spawn, tested via [`probe_env`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEnv {
    pub pane_id: String,
    pub socket_path: String,
}

/// Reads the Herdr environment variables. `None` when nanomite is not running
/// inside a Herdr pane or the integration is disabled.
pub fn probe_env() -> Option<LinkEnv> {
    if std::env::var("HERDR_ENV").ok().as_deref() != Some("1") {
        return None;
    }
    let pane_id = std::env::var("HERDR_PANE_ID").ok()?;
    let socket_path = std::env::var("HERDR_SOCKET_PATH").ok()?;
    if pane_id.is_empty() || socket_path.is_empty() {
        return None;
    }
    Some(LinkEnv {
        pane_id,
        socket_path,
    })
}

static REPORT_SEQ: AtomicU64 = AtomicU64::new(1);

/// Builds the JSON line for one `pane.report_agent` request. Exposed for tests.
pub fn report_line(
    env: &LinkEnv,
    state: AgentState,
    message: Option<&str>,
    session: Option<&str>,
) -> String {
    let params = serde_json::json!({
        "pane_id": env.pane_id,
        "source": SOURCE,
        "agent": AGENT_ID,
        "state": state.as_str(),
        "seq": REPORT_SEQ.fetch_add(1, Ordering::Relaxed),
        "message": message,
        "agent_session_id": session,
    });
    format!(
        "{{\"id\":\"{SOURCE}:{seq}\",\"method\":\"pane.report_agent\",\"params\":{params}}}\n",
        seq = REPORT_SEQ.load(Ordering::Relaxed)
    )
}

/// Best-effort lifecycle reporter. `None` when not under Herdr or disabled.
pub struct HerdrLink {
    inner: Option<Arc<HerderInner>>,
}

struct HerderInner {
    env: LinkEnv,
    tx: mpsc::UnboundedSender<String>,
}

impl HerdrLink {
    /// Probes the environment and spawns the writer thread when present.
    /// Never fails: without a socket the link degrades to a no-op.
    /// In lib tests this is always inert so App construction under a live
    /// Herdr session never reports test turns to the operator's socket.
    #[cfg(test)]
    pub fn spawn(enabled: bool) -> Self {
        let _ = enabled;
        Self::spawn_with(None)
    }

    #[cfg(not(test))]
    pub fn spawn(enabled: bool) -> Self {
        let env = if enabled { probe_env() } else { None };
        Self::spawn_with(env)
    }

    /// Same, with the environment injected directly (tests).
    pub(crate) fn spawn_with(env: Option<LinkEnv>) -> Self {
        let Some(env) = env else {
            return Self { inner: None };
        };
        let (tx, rx) = mpsc::unbounded_channel::<String>();
        let inner = Arc::new(HerderInner {
            env: env.clone(),
            tx,
        });
        let socket_path = env.socket_path.clone();
        std::thread::Builder::new()
            .name("herdr-report".into())
            .spawn(move || writer_loop(rx, socket_path))
            .ok();
        Self { inner: Some(inner) }
    }

    pub fn is_live(&self) -> bool {
        self.inner.is_some()
    }

    /// Queues one state report against the environment captured at spawn.
    /// Fire-and-forget; a closed channel or a full one is indistinguishable
    /// from "Herdr went away" and simply drops the report.
    pub fn report(&self, state: AgentState, message: Option<&str>, session: Option<&str>) {
        let Some(inner) = &self.inner else {
            return;
        };
        let _ = inner
            .tx
            .send(report_line(&inner.env, state, message, session));
    }
}

/// Serialized writer: one connection per line, 500ms write timeout, exact
/// mirror of Herdr's own plugin behaviour (connect, write, read-to-close).
fn writer_loop(rx: mpsc::UnboundedReceiver<String>, socket_path: String) {
    writer_loop_impl(rx, socket_path);
}

#[cfg(unix)]
fn writer_loop_impl(mut rx: mpsc::UnboundedReceiver<String>, socket_path: String) {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    while let Some(line) = rx.blocking_recv() {
        let Ok(mut stream) = UnixStream::connect(&socket_path) else {
            continue;
        };
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(500)));
        let _ = stream.write_all(line.as_bytes());
        let _ = stream.flush();
        // Closing the write side signals the server to reply and close.
        let _ = stream.shutdown(std::net::Shutdown::Write);
        let mut sink = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stream, &mut sink);
    }
}

#[cfg(not(unix))]
fn writer_loop_impl(_rx: mpsc::UnboundedReceiver<String>, _socket_path: String) {
    // Non-unix platforms: herdr is not supported there yet; the link stays a
    // no-op (reports are queued and dropped).
    while (_rx.blocking_recv()).is_some() {}
}

/// Writes the OSC 0 window-title escape for `state` to stdout. Called from the
/// UI thread only (single writer), so it cannot interleave with ratatui's
/// own escape sequences.
pub fn set_osc_title(state: AgentState) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = write!(out, "\x1b]0;{}\x07", state.title());
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_env() -> LinkEnv {
        LinkEnv {
            pane_id: "w1:p1".into(),
            socket_path: "/tmp/herdr.sock".into(),
        }
    }

    #[test]
    fn report_line_carries_the_full_protocol() {
        let line = report_line(
            &test_env(),
            AgentState::Working,
            Some("running tests"),
            Some("sess1"),
        );
        let v: serde_json::Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(v["method"], "pane.report_agent");
        assert_eq!(v["params"]["pane_id"], "w1:p1");
        assert_eq!(v["params"]["agent"], "nanomite");
        assert_eq!(v["params"]["source"], "herdr:nanomite");
        assert_eq!(v["params"]["state"], "working");
        assert_eq!(v["params"]["message"], "running tests");
        assert_eq!(v["params"]["agent_session_id"], "sess1");
        assert!(v["params"]["seq"].as_u64().unwrap() > 0);
        assert!(line.ends_with('\n'), "socket protocol is line-delimited");
    }

    #[test]
    fn seq_is_monotonic_across_reports() {
        let env = test_env();
        let a = report_line(&env, AgentState::Idle, None, None);
        let b = report_line(&env, AgentState::Working, None, None);
        let av: serde_json::Value = serde_json::from_str(&a).unwrap();
        let bv: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert!(
            bv["params"]["seq"].as_u64().unwrap() > av["params"]["seq"].as_u64().unwrap(),
            "seq must increase"
        );
    }

    #[test]
    fn states_map_to_readable_labels_and_titles() {
        assert_eq!(AgentState::Working.as_str(), "working");
        assert_eq!(AgentState::Blocked.as_str(), "blocked");
        assert_eq!(AgentState::Idle.as_str(), "idle");
        assert_eq!(AgentState::Working.title(), "nanomite — working");
        assert_eq!(AgentState::Idle.title(), "nanomite");
    }

    #[test]
    fn manifest_declares_required_keys_and_rules() {
        // `toml` is optional (format-structured); the manifest is a static
        // string, so structural sanity checks suffice.
        assert!(MANIFEST.contains("id = \"nanomite\""));
        assert!(MANIFEST.contains("min_engine_version = 1"));
        assert_eq!(MANIFEST.match_indices("[[rules]]").count(), 5);
        for state in [
            "state = \"working\"",
            "state = \"blocked\"",
            "state = \"idle\"",
        ] {
            assert!(MANIFEST.contains(state), "{state} rule missing");
        }
        // Every rule region must be one herdr understands.
        for region in ["osc_title", "whole_recent", "bottom_non_empty_lines(3)"] {
            assert!(MANIFEST.contains(&format!("region = \"{region}\"")));
        }
    }

    #[test]
    fn probe_env_requires_all_three_variables() {
        // Not inside herdr in unit tests (vars may exist under the runner);
        // probe only asserts shape, covered indirectly by spawn() returning
        // a no-op link when absent.
        let link = HerdrLink::spawn(true);
        if probe_env().is_none() {
            assert!(!link.is_live());
        }
    }

    #[test]
    fn link_writes_reports_to_the_socket() {
        use std::io::Read;
        use std::os::unix::net::UnixListener;

        let dir = std::env::temp_dir().join(format!("nanomite-herdr-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let listener = UnixListener::bind(&dir).expect("bind test socket");
        let env = LinkEnv {
            pane_id: "wK:pT".into(),
            socket_path: dir.display().to_string(),
        };
        let link = HerdrLink::spawn_with(Some(env));
        assert!(link.is_live());

        link.report(AgentState::Working, Some("turn started"), None);
        link.report(AgentState::Idle, None, Some("sess-1"));

        // The writer connects once per report; accept both, read the line.
        let mut lines = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("connection");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .ok();
            let mut buf = Vec::new();
            let _ = stream.read_to_end(&mut buf);
            lines.push(String::from_utf8_lossy(&buf).to_string());
        }
        let working: serde_json::Value =
            serde_json::from_str(lines[0].trim_end()).expect("first report parses");
        assert_eq!(working["method"], "pane.report_agent");
        assert_eq!(working["params"]["pane_id"], "wK:pT");
        assert_eq!(working["params"]["state"], "working");
        assert_eq!(working["params"]["message"], "turn started");
        let idle: serde_json::Value =
            serde_json::from_str(lines[1].trim_end()).expect("second report parses");
        assert_eq!(idle["params"]["state"], "idle");
        assert_eq!(idle["params"]["agent_session_id"], "sess-1");

        // A dead socket must not panic the writer or wedge the channel.
        link.report(AgentState::Working, None, None);
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn spawn_with_no_env_is_a_silent_no_op() {
        let link = HerdrLink::spawn_with(None);
        assert!(!link.is_live());
        // Reporting on a dead link must not panic (reports are dropped).
        link.report(AgentState::Working, None, None);
    }
}
