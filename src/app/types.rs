//! Pure UI data types extracted from `App`.
//! These types have no dependency on `App` and can be reasoned about
//! independently of the main event loop.

use std::path::PathBuf;
use std::time::SystemTime;



/// One `/memory` row: the projection of a stored record the UI needs.
#[derive(Debug, Clone)]
pub struct MemoryRow {
    pub id: String,
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub project: Option<String>,
    pub content: String,
}

impl MemoryRow {
    pub fn hint(&self) -> String {
        format!("{} · {}", self.category, self.created_at)
    }
}

/// One improvement proposal row for the `/improve` list.
#[derive(Debug, Clone)]
pub struct ImproveRow {
    pub id: u64,
    pub target: String,
    pub path: String,
    pub rationale: String,
    pub created_at: String,
    pub added: usize,
    pub removed: usize,
}

/// Improvement apply status for the overlay status line.
#[derive(Debug, Clone)]
pub struct ImproveStatus {
    pub level: ImproveLevel,
    pub message: String,
}

/// Improvement status level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImproveLevel {
    Success,
    Warn,
    Danger,
}

/// Turn statistics for the performance HUD.
#[derive(Debug, Clone)]
pub struct TurnStat {
    pub model: String,
    pub duration_ms: u64,
    pub tokens_per_sec: f64,
}

/// Turn snapshot for undo/redo time travel.
#[derive(Debug, Clone)]
pub struct TurnSnapshot {
    pub turn_serial: usize,
    pub files: Vec<(PathBuf, String)>,
}

/// Undo step for the undo stack.
#[derive(Debug, Clone)]
pub struct UndoStep {
    pub turn_serial: usize,
    pub snapshot: TurnSnapshot,
}

/// Board state for the `/panel` convene.
#[derive(Debug, Clone)]
pub struct BoardState {
    pub topic: String,
    pub messages: Vec<String>,
    pub round: usize,
    pub max_rounds: usize,
}

/// Provider row for the model picker.
#[derive(Debug, Clone)]
pub struct ProviderRow {
    pub provider: String,
    pub models: Vec<String>,
    pub current: Option<String>,
}

/// Providers state for the model picker.
#[derive(Debug, Clone, Default)]
pub struct ProvidersState {
    pub rows: Vec<ProviderRow>,
    pub selected: usize,
}

/// Hacker state tracked by App.
#[derive(Debug, Clone)]
pub struct HackerState {
    pub active: bool,
    pub auto_approve: bool,
    pub assessment_id: Option<String>,
}

/// Improve state tracked by App.
#[derive(Debug, Clone)]
pub struct ImproveState {
    pub gen: u64,
    pub pending: Vec<ImproveRow>,
}

/// Memory view state.
#[derive(Debug, Clone)]
pub struct MemoryView {
    pub query: String,
    pub results: Vec<MemoryRow>,
}

/// Assessment command parsed from `/hacker assessment`.
#[derive(Debug, Clone)]
pub enum AssessmentCmd {
    List,
    Use(String),
    Complete(Option<String>),
}

/// Findings command parsed from `/hacker findings`.
#[derive(Debug, Clone)]
pub enum FindingsCmd {
    List,
    Show(usize),
    SetStatus(usize, crate::hacker::FindingStatus),
    Verify(usize),
}

/// Roots for the improve engine.
#[derive(Debug, Clone)]
pub struct Roots {
    pub config_path: PathBuf,
    pub prompt_path: PathBuf,
    pub repo_root: PathBuf,
}

/// Hacker event from background `/hacker run` operations.
#[derive(Debug, Clone)]
pub struct HackerEvent {
    pub op: String,
    pub output: String,
}

/// One improvement event.
#[derive(Debug, Clone)]
pub enum ImproveEvent {
    StatusChanged(u64, String),
}

/// Session event from session store operations.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    Saved(String),
    Loaded(String),
}

/// Models result from provider listing.
#[derive(Debug, Clone)]
pub enum ModelsResult {
    Ok(Vec<String>),
    Err(String),
}

/// Memory result from background memory operations.
#[derive(Debug, Clone)]
pub enum MemoryResult {
    Ok(Vec<String>),
    Err(String),
}

/// Board event from background board operations.
#[derive(Debug, Clone)]
pub enum BoardEvent {
    Ok(Vec<String>),
    Err(String),
}

/// Agent control signals sent from the UI to the background tasks.
#[derive(Debug, Clone)]
pub enum Control {
    SudoResult(String),
    OpenLink(String),
    Copy(String),
}

/// Connect form state for the `/connect` overlay.
#[derive(Debug, Clone)]
pub struct ConnectForm {
    pub fields: Vec<String>,
    pub focused: usize,
}

impl ConnectForm {
    pub fn from_config(_cfg: &crate::config::Config) -> Self {
        Self {
            fields: Vec::new(),
            focused: 0,
        }
    }

    pub fn field(&self, index: usize) -> Option<&str> {
        self.fields.get(index).map(|s| s.as_str())
    }

    pub fn field_mut(&mut self, index: usize) -> Option<&mut String> {
        self.fields.get_mut(index)
    }

    pub fn focus_by(&mut self, delta: isize) {
        if self.fields.is_empty() {
            return;
        }
        let new = (self.focused as isize + delta).rem_euclid(self.fields.len() as isize);
        self.focused = new as usize;
    }
}

/// Pending image attachment before it is attached to the next user turn.
#[derive(Debug, Clone)]
pub struct PendingImage {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mime: String,
}

/// One suggestion from the follow-up side-call.
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub text: String,
    pub score: f64,
}

/// Provider models target.
#[derive(Debug, Clone)]
pub struct ProviderTarget {
    pub provider: String,
    pub model: String,
}

/// Sudo password state wrapper.
#[derive(Debug, Clone)]
pub struct SudoPasswordStateWrapper {
    pub state: crate::app::SudoPasswordState,
}

/// Session grants shared between confirmer and hacker bypass.
#[derive(Debug, Default, Clone)]
pub struct SessionGrants {
    pub hacker_active: bool,
    pub expiry: Option<SystemTime>,
}
