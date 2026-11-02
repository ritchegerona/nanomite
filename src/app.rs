use crate::agent::openai::OpenAiProvider;
use crate::agent::provider::{AgentProvider, ChatMessage, Usage};
use crate::agent::runtime::{AgentEvent, AgentRuntime, Cancel};
use crate::agent::suggest::{self, SuggestResult};
use crate::agent::swarm::Swarm;
#[cfg(test)]
use crate::agent::testdouble::ScriptedProvider;
use crate::board::engine::{BoardEngine, CancelFlag};
use crate::board::store::{BoardMessage, BoardStore};
use crate::commands::{self, Command};
use crate::config::{find_preset, host_of, Config, ProviderKind, ProviderPreset, PRESETS};
use crate::improve::{
    Actor, ApplyOutcome, CargoTestRunner, ImproveEngine, RegressionRunner, Roots, Status, Target,
};
use crate::memory::policy::MemoryPolicy;
use crate::memory::MemoryEngine;
use crate::session::{self, StoredSession};
use crate::ui::chat;
use crate::ui::composer::Composer;
use crate::ui::theme::Theme;
use crate::ui::theme::ThemeId;
use crate::ui::View;
use ratatui::text::Line;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

const MAX_HISTORY_MESSAGES: usize = 40;
/// Follow-up suggestions one finished turn shows above the composer.
const SUGGESTION_COUNT: usize = 3;
const TOAST_TTL: Duration = Duration::from_secs(2);
/// Discussion rounds a `/panel` convene runs (engine caps at `MAX_ROUNDS`).
const BOARD_ROUNDS: usize = 2;
/// Records `/memory` lists: newest first, bounded so the picker stays readable.
const MEMORY_LIST_ROWS: usize = 100;
/// Hard cap on the body one `/memory` record overlay will render.
const MEMORY_BODY_CHARS: usize = 40_000;
/// Proposals one `/improve` list shows; the engine listing is already bounded.
const IMPROVE_LIST_ROWS: usize = 50;
/// Hard cap on diff lines one `/improve` review view renders.
const IMPROVE_REVIEW_LINES: usize = 400;
/// Hard cap on the prompt file appended to the system prompt.
const PROMPT_FILE_MAX_BYTES: usize = 16 * 1024;
/// Failure-detail lines kept under a rolled-back proposal.
pub const IMPROVE_DETAIL_LINES: usize = 8;
/// What the production regression gate runs; shown while an approve is in
/// flight so the user knows what they are waiting for.
const REGRESSION_COMMAND: &str = "cargo test --lib";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Streaming,
    Done,
    Interrupted,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ToolRow {
    pub name: String,
    pub detail: String,
    pub running: bool,
    pub ok: bool,
    /// File edit specific fields (populated for edit_file tool)
    pub edit_path: Option<String>,
    pub edit_replacements: Option<usize>,
    /// Bounded code/command preview built from the call arguments (shown live
    /// under the row while the tool runs; collapsed by `/details` off).
    pub preview: Option<String>,
    /// When this tool call started; the source for its per-pill duration.
    pub started: Instant,
    /// Set once the tool finishes; pills show this, not the turn total.
    pub elapsed: Option<Duration>,
}

/// Cached, width-dependent rendering of one transcript entry.
#[derive(Debug, Clone, Default)]
pub struct Rendered {
    pub rev: u64,
    pub width: u16,
    /// Tool-display level this cache was built for.
    pub tool_display: ToolDisplay,
    /// Density the cache was built for.
    pub compact: bool,
    /// Tool-mode colour the cache was built for: the user-message bar and the
    /// assistant footer follow the active mode, like opencode's per-agent
    /// colouring.
    pub mode_color: ratatui::style::Color,
    pub mode_label: String,
    /// Fenced code blocks as `(first body row, last border row, raw source)`,
    /// rows indexing `lines` — the click-to-copy target list.
    pub code_blocks: Vec<(u16, u16, String)>,
    /// Click-to-open links as `(row, char_start, char_end_exclusive, url)`,
    /// rows indexing `lines` — discovered at cache build time by
    /// [`crate::ui::text::link_regions`] so a mouse click resolves a URL
    /// without re-rendering.
    pub links: Vec<(u16, u16, u16, String)>,
    pub lines: Vec<Line<'static>>,
    pub height: u16,
}

#[derive(Debug)]
pub struct Entry {
    pub role: EntryRole,
    pub phase: Phase,
    pub text: String,
    pub tools: Vec<ToolRow>,
    pub error: Option<String>,
    pub started: Instant,
    /// Wall-clock creation time; the YOU/AI badges render this as HH:MM.
    /// `started` is a monotonic `Instant` and must never be formatted as a
    /// clock time.
    pub at: SystemTime,
    pub elapsed: Option<Duration>,
    pub usage: Option<Usage>,
    /// Agent steps taken this turn (one per tool round-trip). Shown live in
    /// the status strip so long runs are visibly making progress.
    pub steps: usize,
    /// Model that produced this entry; the message footer shows it
    /// (`Build · model · 8.4s · 45.2 tok/s`), like opencode's footer.
    pub model: Option<String>,
    /// A reasoning model's thought tokens (transient display data: streamed
    /// as a dim tail while working, collapsed to one line when done; never
    /// persisted with the session).
    pub reasoning: String,
    /// Full error text starts collapsed to a couple of lines; a plain click
    /// on the entry expands it (opencode's error expansion).
    pub error_expanded: bool,
    pub(crate) rev: u64,
    pub(crate) cache: Option<Rendered>,
}

impl Entry {
    pub fn new(role: EntryRole, text: impl Into<String>) -> Self {
        Self {
            role,
            phase: Phase::Done,
            text: text.into(),
            tools: Vec::new(),
            error: None,
            started: Instant::now(),
            at: SystemTime::now(),
            elapsed: None,
            usage: None,
            steps: 0,
            model: None,
            reasoning: String::new(),
            error_expanded: false,
            rev: 1,
            cache: None,
        }
    }

    pub fn streaming() -> Self {
        Self {
            phase: Phase::Streaming,
            ..Self::new(EntryRole::Assistant, "")
        }
    }

    pub fn touch(&mut self) {
        self.rev += 1;
    }
}

/// Local actions reachable from the palette (slash commands use `Command`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Interrupt,
    CycleReasoning,
    Noop,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerMode {
    Commands,
    Model,
    Sessions,
    SessionsSearch,
    Themes,
    Memory,
    History,
}

/// What a picker row selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerValue {
    Action(Action),
    Command(Command),
    Model(String),
    Session(std::path::PathBuf),
    Theme(crate::ui::theme::ThemeId),
    Memory(MemoryRow),
    History(String), // The selected history entry text
}

/// One `/memory` row: the projection of a stored record the UI needs. Kept
/// separate from `MemoryRecord` so the picker never holds a whole database row
/// and the overlay only carries what it renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRow {
    pub id: String,
    /// Headline: the record summary when present, else its first line.
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    /// Unix seconds.
    pub created_at: i64,
    pub project: Option<String>,
    pub content: String,
}

impl MemoryRow {
    /// Compact "how long ago" label: `4m`, `3h`, `5d`, then `YYYY-MM-DD`.
    pub fn age_label(&self) -> String {
        let now = crate::memory::model::now();
        let delta = (now - self.created_at).max(0);
        if delta < 60 {
            format!("{delta}s")
        } else if delta < 3_600 {
            format!("{}m", delta / 60)
        } else if delta < 86_400 {
            format!("{}h", delta / 3_600)
        } else if delta < 30 * 86_400 {
            format!("{}d", delta / 86_400)
        } else {
            // Far enough back that a calendar date is more useful than a count.
            let (y, m, d) = civil_from_days(self.created_at.div_euclid(86_400));
            format!("{y:04}-{m:02}-{d:02}")
        }
    }

    /// `core · 2 tags · 3h ago`, the picker's right-hand hint.
    pub fn hint(&self) -> String {
        let mut hint = self.category.clone();
        if let Some(project) = &self.project {
            hint.push_str(&format!(" · {project}"));
        }
        if !self.tags.is_empty() {
            hint.push_str(&format!(" · {}", self.tags.len()));
        }
        hint.push_str(&format!(" · {}", self.age_label()));
        hint
    }
}

/// Days since the Unix epoch to a proleptic-Gregorian civil date (Howard Hinnant's
/// algorithm). Avoids pulling a date library in for the `/memory` age column.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[derive(Debug, Clone)]
pub struct PickerItem {
    pub label: String,
    pub hint: String,
    pub value: PickerValue,
    /// Marks the active provider/model.
    pub current: bool,
}

impl PickerItem {
    pub fn action(label: &str, hint: &str, action: Action) -> Self {
        Self {
            label: label.to_string(),
            hint: hint.to_string(),
            value: PickerValue::Action(action),
            current: false,
        }
    }
}

/// Overlay list with a filter box. Commands, `/provider` and `/model` all use it.
#[derive(Debug, Clone)]
pub struct Picker {
    pub mode: PickerMode,
    pub filter: Composer,
    pub cursor: usize,
    /// Unfiltered rows; `items` is the current view of them.
    pub source: Vec<PickerItem>,
    pub items: Vec<PickerItem>,
    pub loading: bool,
}

impl Picker {
    pub fn new(mode: PickerMode, source: Vec<PickerItem>) -> Self {
        let mut picker = Self {
            mode,
            filter: Composer::new(),
            cursor: 0,
            source,
            items: Vec::new(),
            loading: false,
        };
        picker.apply_filter();
        picker
    }

    pub fn title(&self) -> &'static str {
        match self.mode {
            PickerMode::Commands => "commands",
            PickerMode::Model => "model",
            PickerMode::Sessions => "sessions",
            PickerMode::SessionsSearch => "history search",
            PickerMode::Themes => "themes",
            PickerMode::Memory => "memory entries",
            PickerMode::History => "history",
        }
    }

    pub fn placeholder(&self) -> &'static str {
        match self.mode {
            PickerMode::Commands => "type to filter",
            PickerMode::Model => "filter, or type a custom model id",
            PickerMode::Sessions => "pick a session to resume",
            PickerMode::SessionsSearch => "matches across saved sessions",
            PickerMode::Themes => "pick a colour theme",
            PickerMode::Memory => "filter stored memories",
            PickerMode::History => "search prompt history",
        }
    }

    /// True when the filter box doubles as a free-text value (custom model ids).
    pub fn accepts_free_text(&self) -> bool {
        self.mode == PickerMode::Model
    }

    pub fn set_source(&mut self, source: Vec<PickerItem>, keep_cursor: Option<usize>) {
        self.source = source;
        self.loading = false;
        self.apply_filter();
        if let Some(idx) = keep_cursor {
            if idx < self.items.len() {
                self.cursor = idx;
            }
        }
        if let Some(current) = self.items.iter().position(|i| i.current) {
            self.cursor = current;
        }
    }

    pub fn apply_filter(&mut self) {
        let query = self.filter.value().trim().to_lowercase();
        self.items = if query.is_empty() {
            self.source.clone()
        } else {
            self.source
                .iter()
                .filter(|item| {
                    let hay = format!(
                        "{} {} {}",
                        item.label,
                        item.hint,
                        match &item.value {
                            PickerValue::Model(m) => m.clone(),
                            PickerValue::Session(path) => path.display().to_string(),
                            PickerValue::Theme(t) => t.label().to_string(),
                            PickerValue::Memory(row) => {
                                format!("{} {}", row.title, row.content)
                            }
                            PickerValue::Action(a) => format!("{a:?}"),
                            PickerValue::History(h) => h.clone(),
                            PickerValue::Command(c) => {
                                // Aliases stay searchable even though they are not displayed.
                                let mut v = format!("{c:?}");
                                if let Some(entry) =
                                    crate::commands::COMMANDS.iter().find(|e| e.kind == *c)
                                {
                                    v.push(' ');
                                    v.push_str(&entry.aliases.join(" "));
                                }
                                v
                            }
                        }
                    )
                    .to_lowercase();
                    // Every query token must appear in the row. (The old predicate
                    // `hay-word in query` made `/themes` match `/connect` because the
                    // hint word "the" is a substring of the query.)
                    query.split_whitespace().all(|q| hay.contains(q))
                })
                .cloned()
                .collect()
        };
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
    }

    /// The value that `enter` should commit.
    pub fn selection(&self) -> Option<PickerValue> {
        if let Some(item) = self.items.get(self.cursor) {
            return Some(item.value.clone());
        }
        if self.accepts_free_text() {
            let typed = self.filter.value().trim();
            if !typed.is_empty() {
                return Some(PickerValue::Model(typed.to_string()));
            }
        }
        None
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.items.is_empty() {
            self.cursor = 0;
            return;
        }
        let n = self.items.len() as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, n - 1) as usize;
    }
}

/// One row of the inline `/` list shown above the composer.
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Literal text shown and inserted by `tab`.
    pub command: String,
    pub hint: String,
}

impl Suggestion {
    pub fn parse(&self) -> Option<Command> {
        commands::parse(&self.command).map(|(kind, _)| kind)
    }
}

/// `/connect` form: shows the active provider, edits base URL and API key.
#[derive(Debug, Clone)]
pub struct ConnectForm {
    pub base_url: Composer,
    pub api_key: Composer,
    pub focus: usize,
}

/// Editable rows. The provider name above them is read-only.
pub const CONNECT_FIELDS: [&str; 2] = ["base url", "api key"];

impl ConnectForm {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            base_url: Composer::filled(cfg.base_url.clone()),
            api_key: Composer::filled(cfg.api_key.clone().unwrap_or_default()),
            focus: 0,
        }
    }

    pub fn field(&self, index: usize) -> &Composer {
        match index {
            0 => &self.base_url,
            _ => &self.api_key,
        }
    }

    pub fn field_mut(&mut self, index: usize) -> &mut Composer {
        match index {
            0 => &mut self.base_url,
            _ => &mut self.api_key,
        }
    }

    pub fn focus_by(&mut self, delta: isize) {
        let n = CONNECT_FIELDS.len() as isize;
        self.focus = (self.focus as isize + delta).rem_euclid(n) as usize;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    Picker,
    Providers,
    Connect,
    Help,
    Tools,
    Board,
    Memory,
    Improve,
    SudoPassword,
    Hacker,
}

impl Overlay {
    #[cfg(test)]
    pub fn kind(&self) -> OverlayKind {
        match self {
            Overlay::Picker(_) => OverlayKind::Picker,
            Overlay::Providers(_) => OverlayKind::Providers,
            Overlay::Connect(_) => OverlayKind::Connect,
            Overlay::Help(_) => OverlayKind::Help,
            Overlay::Tools(_) => OverlayKind::Tools,
            Overlay::Board(_) => OverlayKind::Board,
            Overlay::Memory(_) => OverlayKind::Memory,
            Overlay::Improve(_) => OverlayKind::Improve,
            Overlay::SudoPassword(_) => OverlayKind::SudoPassword,
            Overlay::Hacker(_) => OverlayKind::Hacker,
        }
    }
}

/// Cursor for the read-only HACKER MODE workspace overlay.
#[derive(Debug, Clone, Default)]
pub struct HackerState {
    pub cursor: usize,
}

#[derive(Debug, Clone)]
pub enum Overlay {
    Picker(Picker),
    Providers(ProvidersState),
    Connect(ConnectForm),
    Help(HelpState),
    Tools(ToolsState),
    Board(BoardState),
    Memory(MemoryView),
    /// `/improve`: pending self-edit proposals, with a second-level review view
    /// (`enter`) where the user approves (`a`) or rejects (`r`).
    Improve(ImproveState),
    /// Modal for entering sudo password when a command requires elevation.
    SudoPassword(SudoPasswordState),
    /// `/hacker`: the read-only HACKER MODE workspace (asssessment, modules).
    Hacker(HackerState),
}

/// One staged proposal as the `/improve` list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImproveRow {
    pub id: String,
    /// `config`, `prompt` or `source`.
    pub target: String,
    pub path: String,
    /// One line: why the agent wants this change.
    pub rationale: String,
    pub created_at: String,
    pub added: usize,
    pub removed: usize,
}

/// Severity of the last `/improve` decision, mapped to a theme colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImproveLevel {
    Info,
    Success,
    Warn,
    Danger,
}

/// One status line under the `/improve` view (outcome, or the in-flight verify).
#[derive(Debug, Clone)]
pub struct ImproveStatus {
    pub level: ImproveLevel,
    pub message: String,
}

/// `/improve` overlay state. The list is loaded by a blocking task; `review`
/// holds the diff of the highlighted proposal once the user opens it.
#[derive(Debug, Clone, Default)]
pub struct ImproveState {
    pub rows: Vec<ImproveRow>,
    pub cursor: usize,
    /// `Some` while the diff of one proposal is open for review.
    pub review: Option<ImproveReview>,
    /// Diff rows hidden from the top of the review view.
    pub offset: usize,
    /// Id of the proposal whose regression run is in flight.
    pub applying: Option<String>,
    /// Outcome of the last decision, or the in-flight verify note.
    pub status: Option<ImproveStatus>,
    /// True until the first listing lands, so the view can say "loading".
    pub loading: bool,
}

/// State for the sudo password input overlay.
#[derive(Debug, Clone, Default)]
pub struct SudoPasswordState {
    /// The command that requires sudo.
    pub command: String,
    /// The working directory for the command.
    pub cwd: Option<String>,
    /// The timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// The password being typed (masked in UI).
    pub password: String,
    /// Whether the password submission is in progress.
    pub submitting: bool,
    /// Error message if the previous attempt failed.
    pub error: Option<String>,
}

/// The diff of one proposal, already bounded and split into renderable lines.
#[derive(Debug, Clone)]
pub struct ImproveReview {
    pub id: String,
    pub path: String,
    pub target: String,
    pub rationale: String,
    pub added: usize,
    pub removed: usize,
    /// Raw unified-diff lines; `ui::diff::DiffView` styles them.
    pub lines: Vec<String>,
}

/// Read-only `/memory` record view: one stored memory, opened from the picker.
#[derive(Debug, Clone)]
pub struct MemoryView {
    pub record: MemoryRow,
    /// Rows hidden from the top, so `up`/`pgup` move toward older body text.
    pub offset: usize,
}

/// One `/undo` step: the transcript rows that were removed, the exact history
/// messages they owned, and how many messages a `/redo` has to put back.
#[derive(Debug)]
struct UndoStep {
    entries: Vec<Entry>,
    history: Vec<ChatMessage>,
    /// `turn_history` entry this turn owned.
    delta: usize,
}

/// One finished turn's footprint, feeding the ETA estimator.
#[derive(Debug, Clone)]
pub struct TurnStat {
    pub model: String,
    pub duration: Duration,
    pub completion_tokens: u32,
    pub steps: usize,
}

/// How tool activity renders in the transcript (`/details`, `ctrl+o`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolDisplay {
    /// No tool rows at all - just the response text. The default: the
    /// status strip still shows live step/tool progress while working.
    #[default]
    Hidden,
    /// One muted row per tool call, no preview bodies.
    Rows,
    /// Tool rows with their preview bodies (code, command output, diffs).
    Full,
}

/// One turn's pre-change file contents for `/revert` (the time machine).
#[derive(Debug)]
pub(crate) struct TurnSnapshot {
    /// Monotonic turn serial, incremented per prompt.
    turn: usize,
    /// Workspace-relative path -> pre-change bytes (`None` = file was new).
    files: Vec<(String, Option<Vec<u8>>)>,
}

/// `/tools` overlay: the tool library. Rows are derived from the catalog at
/// render time; the highlighted index is the only state.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolsState {
    pub cursor: usize,
}

/// `/panel <topic>` overlay: one board thread while it convenes.
#[derive(Debug, Clone, Default)]
pub struct BoardState {
    pub topic: String,
    pub thread_id: Option<String>,
    pub messages: Vec<BoardMessage>,
    /// True until the convene task posts its done marker.
    pub running: bool,
    /// Rendered rows hidden from the bottom (Up scrolls toward older messages).
    pub offset: usize,
    /// The on-disk store could not be opened; this thread is memory-only.
    pub in_memory: bool,
}

/// One row of the `/connect` provider menu.
#[derive(Debug, Clone)]
pub struct ProviderRow {
    pub id: String,
    pub label: String,
    /// Shown in parentheses, opencode-style: what the provider needs.
    pub hint: String,
    /// Rows above the `Providers` section: the curated short list.
    pub popular: bool,
    pub current: bool,
}

/// Searchable, grouped provider menu (opencode-style "connect a provider").
#[derive(Debug, Clone, Default)]
pub struct ProvidersState {
    pub filter: Composer,
    pub cursor: usize,
}

/// Scroll offset for the help sheet, which is taller than most terminals.
#[derive(Debug, Clone, Copy, Default)]
pub struct HelpState {
    pub offset: usize,
}

/// Transcript search (`ctrl+f`): a footer-hosted query over the rendered
/// entries. `matches` holds entry indices in transcript order; `current`
/// points at the one jumped to and scrolls via `View::scroll_to_entry_index`.
/// The list is refreshed when the query changes — text streamed in while the
/// bar is open is picked up on the next edit or jump.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub query: String,
    /// Entries whose text or tool details contain the query, in order.
    pub matches: Vec<usize>,
    /// Index into `matches` of the match currently jumped to.
    pub current: usize,
}

/// An image attachment queued by `/image` and consumed by the next turn.
#[derive(Debug, Clone)]
pub struct PendingImage {
    /// Base file name, shown in the transcript badge.
    pub name: String,
    /// `data:<mime>;base64,<payload>` — sent as an `image_url` content part.
    pub data_url: String,
    /// Raw byte size, for the badge.
    pub bytes: usize,
}

/// Extensions `/image` accepts, with their wire MIME types.
const IMAGE_MIME: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
];

/// Hard cap on one image attachment so a stray ISO cannot blow up the
/// request body (raw bytes, before base64).
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// At most this many images may sit queued for one message.
const MAX_PENDING_IMAGES: usize = 4;

/// Resolves and validates an `/image <path>` argument without touching the
/// UI thread: extension allow-list, size cap, then read + base64. All I/O
/// happens on the blocking pool; the result travels back over the control
/// channel.
/// Splits a bracketed paste into candidate file paths. Terminals drop files
/// as pasted paths in several shapes: single-quoted (`'/a b.png'`), backslash-
/// escaped (`/a\ b.png`, kitty/WezTerm), double-quoted, backtick-wrapped,
/// `file://` URIs with %20 escapes, or one path per line. A shell-style
/// tokenizer keeps quoted and escaped whitespace inside a token.
///
/// A token counts as path-like only when it contains a `/` or ends in a dotted
/// extension; if any token fails the check the whole paste is prose and returns
/// empty (verbatim insert). Before tokenizing we try the payload as a single
/// path: an unquoted filename containing spaces (`/a b.png`, which no tokenizer
/// can split correctly) resolves on disk and is kept whole.
fn paste_paths(text: &str, cwd: &Path) -> Vec<String> {
    // Fast path: the entire payload (unescaped/unquoted) is one existing path.
    let whole = normalize_token(text.trim());
    if !whole.is_empty() && resolve_against(&whole, cwd).exists() {
        return vec![whole];
    }

    let tokens: Vec<String> = shell_tokens(text)
        .into_iter()
        .map(|t| normalize_token(&t))
        .filter(|t| !t.is_empty())
        .collect();
    if !tokens.is_empty() && tokens.iter().all(|t| is_path_like(t)) {
        tokens
    } else {
        Vec::new() // prose (or mixed) → the paste is text, not paths
    }
}

/// Shell-style word splitting: whitespace separates tokens, but `'…'`, `"…"`
/// and `` `…` `` quotes group, and a backslash escapes the next character
/// (`\ ` keeps the space inside the token). Quotes are stripped by
/// [`normalize_token`], so the returned tokens are still wrapped here.
fn shell_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut quote: Option<char> = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else if c == '\\' && q != '\'' {
                    // Inside double/backtick quotes a backslash escapes the next
                    // char; inside single quotes it is literal.
                    if let Some(next) = chars.next() {
                        cur.push(next);
                    }
                } else {
                    cur.push(c);
                }
                has = true;
            }
            None => match c {
                '\'' | '"' | '`' => {
                    quote = Some(c);
                    has = true;
                }
                '\\' => {
                    if let Some(next) = chars.next() {
                        cur.push(next);
                        has = true;
                    }
                }
                c if c.is_whitespace() => {
                    if has {
                        out.push(std::mem::take(&mut cur));
                        has = false;
                    }
                }
                _ => {
                    cur.push(c);
                    has = true;
                }
            },
        }
    }
    if has {
        out.push(cur);
    }
    out
}

/// Strips one layer of matching wrapper quotes, decodes a `file://` URI and
/// trims trailing separators so a token is a clean filesystem path.
fn normalize_token(tok: &str) -> String {
    let mut t = tok.trim();
    for q in ['\'', '"', '`'] {
        if t.len() >= 2 && t.starts_with(q) && t.ends_with(q) {
            t = &t[1..t.len() - 1];
            break;
        }
    }
    let decoded = match t.strip_prefix("file://") {
        Some(rest) => percent_decode(rest),
        None => t.to_string(),
    };
    decoded.trim_end_matches('/').to_string()
}

/// Resolves a token against the workspace: `~` expands to home, relative paths
/// join `cwd`. Used only for the on-disk existence check.
fn resolve_against(tok: &str, cwd: &Path) -> PathBuf {
    let expanded = expand_tilde(tok);
    if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    }
}

fn expand_tilde(tok: &str) -> PathBuf {
    if tok == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    } else if let Some(rest) = tok.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(tok)
}

fn is_path_like(tok: &str) -> bool {
    tok.contains('/')
        || std::path::Path::new(tok)
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| f.contains('.'))
}

/// True when a fast burst of typed characters is a file dropped onto the
/// terminal. Terminals that do not wrap a drop in a bracketed paste type the
/// path out as ordinary keystrokes; this recovers it. Deliberately
/// conservative: the burst must be an absolute or `~` path that resolves on
/// disk, so ordinary typing is never mistaken for a drop.
pub fn is_dropped_path_burst(burst: &str) -> bool {
    let b = burst.trim();
    if b.len() < 4 || !(b.starts_with('/') || b.starts_with("~/")) {
        return false;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    resolve_against(b, &cwd).exists()
}

/// Minimal percent-decoding for `file://` URIs (%20 and friends); invalid
/// escapes pass through untouched.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            let hex = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(b) = hex {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// True when the path's extension matches the attachable image list
/// (`IMAGE_MIME`), so a drop routes through the `/image` pipeline.
fn is_image_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            IMAGE_MIME
                .iter()
                .any(|(ext, _)| e.eq_ignore_ascii_case(ext))
        })
}

fn build_image_attachment(arg: &str, cwd: &std::path::Path) -> Result<PendingImage, String> {
    let path = arg.trim();
    if path.is_empty() {
        return Err("usage: /image <path-to-image>".into());
    }
    let p = std::path::Path::new(path);
    let mime = IMAGE_MIME
        .iter()
        .find(|(ext, _)| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        })
        .map(|(_, mime)| *mime)
        .ok_or_else(|| "only png, jpg, jpeg, gif or webp can be attached".to_string())?;
    let full = if p.is_absolute() {
        std::path::PathBuf::from(path)
    } else {
        cwd.join(path)
    };
    let meta = full
        .metadata()
        .map_err(|e| format!("cannot read {}: {e}", full.display()))?;
    if meta.len() as usize > MAX_IMAGE_BYTES {
        return Err(format!(
            "{} is over the {} MB limit",
            full.display(),
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(&full).map_err(|e| format!("cannot read {}: {e}", full.display()))?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "{} is over the {} MB limit",
            full.display(),
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    let name = full
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".into());
    Ok(PendingImage {
        name,
        data_url: format!("data:{mime};base64,{}", base64_encode(&bytes)),
        bytes: bytes.len(),
    })
}

/// Human label for the transcript badge: KB below 1 MB, MB above.
fn image_size_label(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

/// Badge line prepended to a user entry carrying `image`.
fn image_badge(image: &PendingImage) -> String {
    format!(
        "[image: {} · {}]",
        image.name,
        image_size_label(image.bytes)
    )
}

/// One interactive tool-confirmation request waiting on the user.
pub struct ConfirmRequest {
    pub(crate) tool: String,
    pub(crate) preview: Option<String>,
    pub(crate) respond: tokio::sync::oneshot::Sender<bool>,
}

/// Session-scoped answers from the confirm prompt ("always allow/deny this
/// tool"), consulted before any new request reaches the user.
#[derive(Default)]
struct SessionGrants {
    allow: std::collections::HashSet<String>,
    deny: std::collections::HashSet<String>,
}

/// The UI side of `agent::policy::Confirmer`: granted tools fast-path,
/// everything else queues a `ConfirmRequest` for the y/n/a/d prompt. The
/// runtime only consults this gate in Plan mode.
pub(crate) struct UiConfirmer {
    tx: tokio::sync::mpsc::UnboundedSender<ConfirmRequest>,
    grants: Arc<std::sync::Mutex<SessionGrants>>,
    /// HACKER MODE bypass: set exactly while an authorized, unexpired,
    /// scope-validated assessment is active (see `HackerSession`). Retained as
    /// defense-in-depth; with the Plan-only gate the confirmer is already
    /// skipped while HACKER MODE is in force. The model can never set this
    /// flag - only the user entering HACKER MODE with a configured scope can.
    bypass: Arc<std::sync::atomic::AtomicBool>,
}

impl crate::agent::policy::Confirmer for UiConfirmer {
    fn confirm(
        &self,
        tool: &str,
        preview: Option<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> {
        if self.bypass.load(std::sync::atomic::Ordering::SeqCst) {
            return Box::pin(async move { true });
        }
        let granted = {
            let grants = self.grants.lock().expect("confirm grants");
            if grants.allow.contains(tool) {
                Some(true)
            } else if grants.deny.contains(tool) {
                Some(false)
            } else {
                None
            }
        };
        if let Some(ok) = granted {
            return Box::pin(async move { ok });
        }
        let tx = self.tx.clone();
        let tool = tool.to_string();
        Box::pin(async move {
            let (respond, reply) = tokio::sync::oneshot::channel();
            let sent = tx.send(ConfirmRequest {
                tool,
                preview,
                respond,
            });
            if sent.is_err() {
                return false; // UI gone: refuse rather than run unasked.
            }
            reply.await.unwrap_or(false)
        })
    }
}

/// Work the main loop must do on the UI thread (it owns the terminal).
#[derive(Debug, Clone)]
pub enum Control {
    /// Open $EDITOR seeded with the composer buffer.
    EditComposer(String),
    /// Open $EDITOR on a file.
    EditFile(std::path::PathBuf),
    /// Write `text` to the terminal clipboard with an OSC 52 sequence.
    Copy(String),
    /// A sudo-elevated command finished; append its output to the transcript.
    SudoResult(String),
    /// `/revert` finished restoring files on the blocking pool.
    RevertDone {
        turn: usize,
        restored: usize,
        failed: usize,
    },
    /// `/doctor` finished its pre-flight checks; one line per check.
    DoctorReport(Vec<String>),
    /// `/history` finished scanning saved sessions.
    HistoryResults {
        query: String,
        hits: Vec<crate::session::HistoryHit>,
    },
    /// The welcome screen's resume hint, queried off the UI thread.
    ResumeHint(String),
    /// `/fork` finished writing the original session to disk.
    ForkSaved { path: Option<std::path::PathBuf> },
    /// `/image` finished reading + encoding an attachment on the blocking
    /// pool; `Err` carries the refusal reason for the toast.
    ImageAttached(Result<PendingImage, String>),
    /// A clicked chat link; the main loop hands it to the system browser.
    OpenLink(String),
    /// `/export` (markdown or HTML) finished writing on the blocking pool.
    ExportDone(Result<std::path::PathBuf, String>),
}

enum ModelsResult {
    Ok(Vec<String>),
    Err(String),
}

/// Result of the background `/memory` query, delivered on `memory_rx`.
enum MemoryResult {
    Rows(Vec<MemoryRow>),
    Err(String),
}

/// Board events sent by the convene task; drained alongside `ModelsResult`.
/// Each channel item is tagged with the convene generation so a cancelled
/// thread's stragglers can never leak into a newer one.
enum BoardEvent {
    Message(BoardMessage),
    /// The on-disk store could not be opened; the thread runs in memory.
    StoreFallback,
    Done {
        thread_id: Option<String>,
        error: Option<String>,
    },
}

/// `/improve` background results, all tagged with the overlay generation so a
/// decision made in an older view can never repaint a newer one.
enum ImproveEvent {
    /// A freshly loaded pending list, optionally carrying the outcome of the
    /// decision that triggered the reload.
    Rows {
        rows: Vec<ImproveRow>,
        status: Option<ImproveStatus>,
    },
    /// The listing failed; `message` is shown verbatim.
    Err(String),
    /// One proposal's diff, loaded when the user opened the review view.
    Review(ImproveReview),
}

/// What a `/hacker findings` invocation asks the store to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FindingsCmd {
    List,
    Show(i64),
    SetStatus(i64, crate::hacker::FindingStatus),
    Verify(i64),
}

/// What a `/hacker assessment` invocation asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AssessmentCmd {
    List,
    Use(String),
    Complete(Option<String>),
}

/// Parses `/hacker assessment` arguments. `Err(())` means "show usage".
fn parse_assessment_cmd(args: &str) -> Result<AssessmentCmd, ()> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    match parts.as_slice() {
        [] | ["list"] => Ok(AssessmentCmd::List),
        ["use", id] => Ok(AssessmentCmd::Use((*id).to_string())),
        ["complete"] => Ok(AssessmentCmd::Complete(None)),
        ["complete", id] => Ok(AssessmentCmd::Complete(Some((*id).to_string()))),
        _ => Err(()),
    }
}

/// Renders the persisted assessment list for the transcript.
fn render_assessment_list(path: &Path) -> String {
    let store = match crate::hacker::store::FindingsStore::open(path) {
        Ok(store) => store,
        Err(err) => return format!("findings store unavailable: {err}"),
    };
    match store.assessments(100) {
        Err(err) => format!("assessment list failed: {err}"),
        Ok(rows) if rows.is_empty() => "no assessments recorded yet".to_string(),
        Ok(rows) => {
            let mut out = format!("{} assessment(s):\n", rows.len());
            for a in rows {
                out.push_str(&format!(
                    "  {} · {} · status {} · {} finding(s)\n    window {} → {}\n    authorization: {}\n    resume with: /hacker assessment use {}\n",
                    a.id,
                    a.name,
                    a.status,
                    a.finding_count,
                    a.started_at,
                    a.expiry_at,
                    a.authorization_ref.as_deref().unwrap_or("(none recorded)"),
                    a.id
                ));
            }
            out
        }
    }
}

/// Parses `/hacker findings` arguments. `Err(())` means "show usage".
fn parse_findings_cmd(args: &str) -> Result<FindingsCmd, ()> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    let id = |raw: &str| raw.parse::<i64>().map_err(|_| ());
    match parts.as_slice() {
        [] | ["list"] => Ok(FindingsCmd::List),
        ["show", raw] => id(raw).map(FindingsCmd::Show),
        ["verify", raw] => id(raw).map(FindingsCmd::Verify),
        [verb @ ("triage" | "accept" | "close"), raw] => {
            let status = match *verb {
                "triage" => crate::hacker::FindingStatus::Triaged,
                "accept" => crate::hacker::FindingStatus::Accepted,
                _ => crate::hacker::FindingStatus::Closed,
            };
            id(raw).map(|id| FindingsCmd::SetStatus(id, status))
        }
        _ => Err(()),
    }
}

/// Formats findings for the transcript. Pure formatting around the store, run
/// on the blocking pool. All text is display-only (never fed back to a tool).
fn render_hacker_findings(path: &Path, cmd: FindingsCmd) -> String {
    let store = match crate::hacker::store::FindingsStore::open(path) {
        Ok(store) => store,
        Err(err) => return format!("findings store unavailable: {err}"),
    };
    if let FindingsCmd::SetStatus(id, status) = cmd {
        return match store.set_status(id, status) {
            Err(err) => format!("findings failed: {err}"),
            Ok(false) => format!("no finding #{id}"),
            Ok(true) => format!("finding #{id} -> {}", status.label()),
        };
    }
    if let FindingsCmd::Verify(id) = cmd {
        return match store.verify_evidence(id) {
            Err(err) => format!("findings failed: {err}"),
            Ok(checks) if checks.is_empty() => format!("finding #{id} has no evidence"),
            Ok(checks) => {
                let mut out = format!("evidence for finding #{id}:\n");
                for c in checks {
                    out.push_str(&format!(
                        "  #{} {}\n",
                        c.id,
                        if c.ok {
                            "ok (sha256 matches)"
                        } else {
                            "MISMATCH (content changed since capture)"
                        }
                    ));
                }
                out
            }
        };
    }
    let show = match cmd {
        FindingsCmd::Show(id) => Some(id),
        _ => None,
    };
    match show {
        Some(id) => match store.finding(id) {
            Err(err) => format!("findings failed: {err}"),
            Ok(None) => format!("no finding #{id}"),
            Ok(Some(f)) => {
                let mut out = format!(
                    "#{} [{}] {}\n  assessment: {}\n  module: {}/{}\n  target: {}\n  confidence: {} · status: {}\n  at: {}\n",
                    f.id,
                    f.severity,
                    f.title,
                    f.assessment_id,
                    f.module,
                    f.operation,
                    f.target,
                    f.confidence,
                    f.status,
                    f.created_at
                );
                if !f.detail.is_empty() {
                    out.push_str(&format!("  detail: {}\n", f.detail));
                }
                match store.evidence(f.id) {
                    Err(err) => out.push_str(&format!("  evidence error: {err}\n")),
                    Ok(rows) => {
                        for e in rows {
                            let digest: String = e.sha256.chars().take(12).collect();
                            out.push_str(&format!(
                                "  evidence[{}] {} · {} bytes · sha256 {digest}…\n    {}\n",
                                e.id, e.kind, e.bytes, e.excerpt
                            ));
                        }
                    }
                }
                out
            }
        },
        None => match store.findings(None, 100) {
            Err(err) => format!("findings failed: {err}"),
            Ok(rows) if rows.is_empty() => "no findings recorded yet".to_string(),
            Ok(rows) => {
                let mut out = format!("{} finding(s):\n", rows.len());
                for f in rows {
                    out.push_str(&format!(
                        "#{:<4} [{}] {} · {} · {}/{}\n",
                        f.id, f.severity, f.title, f.target, f.module, f.operation
                    ));
                }
                out
            }
        },
    }
}

/// A finished `/hacker` background job, delivered on `hacker_rx`.
enum HackerEvent {
    /// A finished module operation.
    Report {
        /// `module/op target` as typed, for the transcript header.
        request: String,
        result: Result<crate::hacker::modules::ModuleReport, String>,
    },
    /// Pre-rendered transcript text from a findings/report query.
    Lines { request: String, text: String },
    /// A persisted assessment loaded for `/hacker assessment use`.
    Resume(Box<crate::hacker::StoredAssessmentSpec>),
    /// `/hacker assessment complete` finished for this assessment id.
    Completed(String),
}

/// opencode-style follow-up suggestions: after a turn finishes, a tiny
/// side-call asks the model for a few short prompts the user would most
/// plausibly send next. The strip above the composer shows them; `Tab`
/// cycles, `Enter` sends, `Esc` dismisses, typing anything dismisses.
#[derive(Debug, Clone)]
pub struct SuggestionStrip {
    /// Cleaned-up prompts (see `agent::suggest::parse_reply`).
    pub items: Vec<String>,
    /// Highlighted row; `Tab` moves it, `Enter` submits it.
    pub selected: usize,
}

pub struct App {
    pub cfg: Config,
    /// HACKER MODE session: registry, assessment, audit and the gated
    /// auto-approve bypass. Disabled unless configured.
    pub hacker: crate::hacker::HackerSession,
    pub theme: Theme,
    pub cwd: PathBuf,
    pub entries: Vec<Entry>,
    pub composer: Composer,
    pub overlay: Option<Overlay>,
    pub usage: Usage,
    pub turn_started: Option<Instant>,
    pub view: View,
    pub toast: Option<(String, Instant)>,
    /// Toggles behind `/details` and `/thinking`.
    pub show_tool_details: bool,
    pub show_thinking: bool,
    pub pending_editor: bool,
    session_started: Instant,
    undo_stack: Vec<UndoStep>,
    control_tx: Option<UnboundedSender<Control>>,

    runtime: AgentRuntime,
    /// Attached persistent memory, when the engine opened. `None` means the
    /// subsystem is off, disabled by policy, or its location was unusable.
    memory: Option<Arc<MemoryEngine>>,
    /// One-time footer hint: why memory is absent (never an error toast).
    memory_hint: Option<String>,
    /// One-time footer hint: why the custom prompt file was not loaded.
    prompt_hint: Option<String>,
    /// Queued user prompts submitted while the agent is busy. Processed FIFO
    /// after each turn completes.
    prompt_queue: VecDeque<String>,
    /// Interactive tool-confirmation gate (see `UiConfirmer`); `None` when
    /// `confirm_external = false` - risky calls then run as before.
    confirm_rx: Option<tokio::sync::mpsc::UnboundedReceiver<ConfirmRequest>>,
    confirmer: Option<Arc<dyn crate::agent::policy::Confirmer>>,
    pub(crate) pending_confirm: Option<ConfirmRequest>,
    confirm_grants: Arc<std::sync::Mutex<SessionGrants>>,
    /// Images queued by `/image`, attached to the next user turn and then
    /// cleared. Bounded by `MAX_PENDING_IMAGES`.
    pending_images: Vec<PendingImage>,
    /// Self-improvement engine. Opening it only creates a data directory, so it
    /// is always attached in production.
    improve: Option<Arc<ImproveEngine>>,
    /// The engine's data directory, where staged `.diff` files live. Kept so the
    /// UI can render a proposal's diff without reaching into engine internals.
    improve_dir: PathBuf,
    /// Anchor paths handed to the engine: which config and prompt files are
    /// allowed, and which directory source edits must stay inside.
    improve_roots: Roots,
    /// Indirection over the regression gate, so tests never shell out to cargo.
    regression_runner: Arc<dyn RegressionRunner + Send + Sync>,
    improve_rx: UnboundedReceiver<(u64, ImproveEvent)>,
    improve_tx: UnboundedSender<(u64, ImproveEvent)>,
    /// Bumped on every `/improve` open and on every decision inside it.
    improve_gen: u64,
    cancel: Option<Cancel>,
    rx: UnboundedReceiver<AgentEvent>,
    tx: UnboundedSender<AgentEvent>,
    models_rx: UnboundedReceiver<ModelsResult>,
    models_tx: UnboundedSender<ModelsResult>,
    memory_rx: UnboundedReceiver<(u64, MemoryResult)>,
    memory_tx: UnboundedSender<(u64, MemoryResult)>,
    /// Bumped on every `/memory` open: stale rows from an older query are dropped.
    memory_gen: u64,
    board_rx: UnboundedReceiver<(u64, BoardEvent)>,
    board_tx: UnboundedSender<(u64, BoardEvent)>,
    /// Results of `/hacker run` operations, executed on the blocking pool.
    hacker_rx: UnboundedReceiver<HackerEvent>,
    hacker_tx: UnboundedSender<HackerEvent>,
    /// Bumped on every `/panel`: stale events from an older convene are dropped.
    board_gen: u64,
    /// Shared with the in-flight convene task so `esc` can stop the engine.
    pub(crate) board_cancel: Option<Arc<CancelFlag>>,
    handle: tokio::runtime::Handle,
    pub(crate) history: Vec<ChatMessage>,
    /// Per user turn, how many `ChatMessage`s that turn appended (a user
    /// message plus its assistant/tool-bearing reply). `/undo` and `/redo`
    /// cut on these exact counts instead of guessing from user-message counts.
    pub(crate) turn_history: Vec<usize>,
    /// Index of the in-flight transcript entry while a turn runs.
    pub(crate) active: Option<usize>,
    /// Index of the current turn for browsing (0 = latest, increments toward older).
    turn_index: usize,
    dirty: bool,
    quit: bool,
    /// Painted composer text width, synced each frame by `ui::render`.
    pub composer_layout_width: usize,
    /// Text selection state for chat area
    pub selection: Option<Selection>,
    /// A scrollbar drag is in progress (press started on the scrollbar track);
    /// drags then jump instead of extending a text selection.
    scrollbar_drag: bool,
    /// Performance hud (`/hud`): draws the last frame time, peak and
    /// transcript stats into the footer.
    pub hud: bool,
    /// Tool activity display level (`/details`, `ctrl+o`).
    pub tool_display: ToolDisplay,
    /// Compact transcript density (`/density`, `ctrl+g`).
    pub compact: bool,
    /// Footprints of the last finished turns (per-model ETA source).
    turn_stats: VecDeque<TurnStat>,
    /// Time machine: pre-change file contents per turn, newest last.
    pub(crate) snapshots: Vec<TurnSnapshot>,
    /// Monotonic turn serial, incremented per prompt.
    turn_serial: usize,
    /// Focus mode (f2, `/focus`): render only the latest exchange.
    pub focus: bool,
    /// First entry index of the focus window; set during render so
    /// global->slice index conversions stay in sync.
    pub focus_base: usize,
    /// The welcome card's resume hint, queried from memory off-thread.
    pub resume_hint: Option<String>,
    /// The resume query fires at most once per session.
    resume_checked: bool,
    /// Frame timing recorded by the main loop: (last us, peak us, draws).
    pub frame_us: u64,
    pub frame_peak_us: u64,
    pub frame_draws: u64,
    /// Transcript search state (`ctrl+f`); `None` while the bar is closed.
    pub search: Option<SearchState>,
    /// Follow-up suggestions currently on screen, if any.
    pub suggestions: Option<SuggestionStrip>,
    /// Generation of the last fired suggestion side-call; a straggler reply
    /// from an older turn carries a stale gen and is dropped.
    suggest_gen: u64,
    suggest_rx: UnboundedReceiver<SuggestResult>,
    suggest_tx: UnboundedSender<SuggestResult>,
}

impl App {
    pub fn new(
        cfg: Config,
        handle: tokio::runtime::Handle,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let theme = Theme::with_id(ThemeId::from_label(&cfg.theme).unwrap_or(ThemeId::Umbrella));
        let cwd = cfg.workspace.clone();

        let provider: Arc<dyn crate::agent::provider::AgentProvider> = match cfg.provider {
            ProviderKind::OpenAi => Arc::new(OpenAiProvider::new(&cfg)?),
            #[cfg(test)]
            ProviderKind::Scripted => Arc::new(ScriptedProvider::default()),
        };

        // HACKER MODE session: built from configuration, disabled by default.
        // Its bypass flag is shared with the confirmer so an active, authorized
        // assessment auto-approves risky calls (owner decision) without the
        // model ever being able to set it.
        let hacker = crate::hacker::HackerSession::new(
            cfg.hacker.enabled,
            cfg.hacker.scope.clone(),
            cfg.hacker.assessment_id.clone(),
            "Configured assessment".to_string(),
            cfg.hacker.session_minutes,
            cfg.hacker.auto_approve,
            cfg.hacker.audit,
        );
        let hacker_bypass = hacker.bypass_flag();

        // Interactive tool confirmations: on by default, but the runtime only
        // consults the gate in Plan mode - Coder, Build and Hacker
        // auto-approve (owner decision). Test presets and
        // `confirm_external = false` keep the old always-allow behavior.
        let (confirmer, confirm_rx, confirm_grants) = if cfg.confirm_external {
            let grants = Arc::new(std::sync::Mutex::new(SessionGrants::default()));
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ConfirmRequest>();
            (
                Some(Arc::new(UiConfirmer {
                    tx,
                    grants: Arc::clone(&grants),
                    bypass: Arc::clone(&hacker_bypass),
                })
                    as Arc<dyn crate::agent::policy::Confirmer>),
                Some(rx),
                grants,
            )
        } else {
            (
                None,
                None,
                Arc::new(std::sync::Mutex::new(SessionGrants::default())),
            )
        };
        let runtime = Self::build_runtime(provider, &cfg, &cwd, confirmer.clone(), cfg.tool_policy);

        let (tx, rx) = unbounded_channel();
        let (models_tx, models_rx) = unbounded_channel();
        let (board_tx, board_rx) = unbounded_channel();
        let (hacker_tx, hacker_rx) = unbounded_channel();
        let (memory_tx, memory_rx) = unbounded_channel();
        let (improve_tx, improve_rx) = unbounded_channel();
        let (suggest_tx, suggest_rx) = unbounded_channel();

        // Persistent memory is opened before the runtime so the six memory tools
        // are live schemas and turn retrieval/capture run from the first prompt.
        // A failure here is not an error: the app runs exactly as before.
        let (memory, memory_hint) = Self::open_memory();
        let runtime = match &memory {
            Some(engine) => runtime.with_memory(Arc::clone(engine)),
            None => runtime,
        };

        // Self-improvement is attached unconditionally (opening the engine only
        // creates a data directory), then the custom prompt file is appended so
        // the model sees: base prompt → memory notice → user instructions. The
        // prompt file is the same file a `prompt` self-edit rewrites, so the two
        // are one loop: hand-written rules and agent-proposed rules land in the
        // same place and the same review gate.
        let (prompt_file, prompt_hint) = Self::read_prompt_file(&cfg);
        let runtime = runtime.with_prompt_file(prompt_file);
        let improve_dir = Self::improve_dir();
        let improve_roots = Self::improve_roots(&cfg, &cwd);
        let improve = Arc::new(ImproveEngine::new(improve_dir.clone()));
        let runtime = runtime.with_improve(Arc::clone(&improve), improve_roots.clone());

        let mut app = Self {
            view: View::new(theme),
            theme,
            cwd,
            entries: Vec::new(),
            composer: Composer::new(),
            overlay: None,
            usage: Usage::default(),
            turn_started: None,
            toast: None,
            show_tool_details: true,
            show_thinking: true,
            pending_editor: false,
            session_started: Instant::now(),
            undo_stack: Vec::new(),
            control_tx: None,
            runtime,
            memory,
            memory_hint,
            prompt_hint,
            prompt_queue: VecDeque::new(),
            confirm_rx,
            confirmer,
            pending_confirm: None,
            confirm_grants,
            pending_images: Vec::new(),
            improve: Some(improve),
            improve_dir,
            improve_roots,
            regression_runner: Arc::new(CargoTestRunner),
            improve_rx,
            improve_tx,
            improve_gen: 0,
            cancel: None,
            rx,
            tx,
            models_rx,
            models_tx,
            memory_rx,
            memory_tx,
            memory_gen: 0,
            board_rx,
            board_tx,
            hacker_rx,
            hacker_tx,
            board_gen: 0,
            board_cancel: None,
            handle,
            history: Vec::new(),
            turn_history: Vec::new(),
            active: None,
            turn_index: 0,
            dirty: true,
            quit: false,
            // Pre-first-frame fallback; `ui::render` syncs it to the painted width.
            composer_layout_width: 80,
            selection: None,
            scrollbar_drag: false,
            hud: false,
            tool_display: ToolDisplay::Hidden,
            compact: cfg.compact,
            turn_stats: VecDeque::new(),
            snapshots: Vec::new(),
            turn_serial: 0,
            focus: false,
            focus_base: 0,
            resume_hint: None,
            resume_checked: false,
            frame_us: 0,
            frame_peak_us: 0,
            frame_draws: 0,
            search: None,
            suggestions: None,
            suggest_gen: 0,
            suggest_rx,
            suggest_tx,
            hacker,
            cfg,
        };

        if let Some(notice) = app.cfg.notice.clone() {
            app.entries.push(Entry::new(EntryRole::System, notice));
        }
        app.mark_dirty();
        Ok(app)
    }

    /// Wired up by the main loop so `/editor` and `/export` can shell out.
    pub fn set_control_channel(&mut self, tx: UnboundedSender<Control>) {
        self.control_tx = Some(tx);
    }

    /// Opens persistent memory at the platform default location.
    ///
    /// Policy is read from the environment exactly as the subsystem documents it:
    /// `NANOMITE_MEMORY=0` disables everything, `NANOMITE_MEMORY_DIR` /
    /// `NANOMITE_MEMORY_DB` move the markdown tree and the database, and
    /// `NANOMITE_MEMORY_READONLY` / `NANOMITE_MEMORY_AUTO` tighten writes and
    /// capture. Nothing here bypasses `MemoryPolicy` - the engine authorizes
    /// every operation on its own.
    ///
    /// Returns the engine, or `None` plus a quiet one-line hint for the footer.
    /// Tests never open the user's real memory tree: the harness attaches an
    /// in-memory engine explicitly instead.
    fn open_memory() -> (Option<Arc<MemoryEngine>>, Option<String>) {
        if cfg!(test) {
            return (None, None);
        }
        let policy = MemoryPolicy::from_env(true);
        if !policy.enabled {
            return (
                None,
                Some("memory off · NANOMITE_MEMORY=1 enables it".to_string()),
            );
        }
        match MemoryEngine::open() {
            Ok(engine) => (Some(Arc::new(engine)), None),
            // A read-only config directory is a normal state on shared machines,
            // not a failure worth a red toast.
            Err(e) => (
                None,
                Some(format!(
                    "memory unavailable · {}",
                    crate::ui::text::truncate(&e.to_string(), 60)
                )),
            ),
        }
    }

    /// Where staged proposals, their diffs and the audit trail live:
    /// `~/.local/share/nanomite/improve`, alongside the session store.
    fn improve_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("nanomite")
            .join("improve")
    }

    /// Anchor paths for the engine. `config`/`prompt` self-edits must name these
    /// exact files; `source` edits must stay inside `repo_root`.
    fn improve_roots(cfg: &Config, workspace: &std::path::Path) -> Roots {
        Roots {
            config_path: cfg
                .store_path
                .clone()
                .unwrap_or_else(crate::config::default_store_path),
            prompt_path: cfg
                .prompt_file
                .clone()
                .unwrap_or_else(crate::config::default_prompt_path),
            repo_root: workspace.to_path_buf(),
        }
    }

    /// Reads the user's custom prompt file, bounded to
    /// [`PROMPT_FILE_MAX_BYTES`]. A missing file is silent (the common case: the
    /// user never wrote one); an unreadable file produces a quiet footer hint,
    /// never a toast.
    ///
    /// Tests skip the read entirely so no test ever touches the real
    /// `~/.config/nanomite/prompt.md`.
    fn read_prompt_file(cfg: &Config) -> (String, Option<String>) {
        if cfg!(test) {
            return (String::new(), None);
        }
        let Some(path) = cfg.prompt_file.as_ref() else {
            return (String::new(), None);
        };
        match std::fs::read(path) {
            Ok(bytes) => {
                let mut text =
                    String::from_utf8_lossy(&bytes[..bytes.len().min(PROMPT_FILE_MAX_BYTES)])
                        .into_owned();
                text.push('\n');
                (text, None)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), None),
            Err(e) => (
                String::new(),
                Some(format!(
                    "prompt file unavailable · {}",
                    crate::ui::text::truncate(&e.to_string(), 60)
                )),
            ),
        }
    }

    /// Attaches an engine the caller already owns. Tests use this with
    /// `MemoryEngine::open_in_memory`, so no test ever reads or writes the
    /// real `~/.config/nanomite/memory` tree.
    #[cfg(test)]
    pub fn attach_memory_for_test(&mut self, engine: Arc<MemoryEngine>) {
        self.memory_hint = None;
        self.memory = Some(engine.clone());
        self.runtime = self.runtime.clone().with_memory(engine);
    }

    /// Attaches a self-improvement engine rooted in a temp directory, plus the
    /// regression runner the review view will use. Production wires
    /// [`CargoTestRunner`]; tests pass a [`FakeRunner`] so no test ever shells
    /// out to cargo.
    #[cfg(test)]
    pub fn attach_improve_for_test(
        &mut self,
        engine: Arc<ImproveEngine>,
        roots: Roots,
        runner: impl RegressionRunner + Send + Sync + 'static,
    ) {
        self.improve_dir = engine.data_dir().to_path_buf();
        self.improve_roots = roots.clone();
        self.improve = Some(Arc::clone(&engine));
        self.regression_runner = Arc::new(runner);
        self.runtime = self
            .runtime
            .clone()
            .with_improve(Arc::clone(&engine), roots);
    }

    /// The context the next prompt is built from. Test-only: it is the exact
    /// boundary `/undo` and `/redo` have to agree on.
    #[cfg(test)]
    fn history_snapshot(&self) -> Vec<ChatMessage> {
        self.history.clone()
    }

    /// True when the memory subsystem is live.
    pub fn memory_attached(&self) -> bool {
        self.memory.is_some()
    }

    /// Quiet footer hint when memory is not attached, so the reason is visible
    /// once without a toast the user would have to catch.
    pub fn memory_hint(&self) -> Option<&str> {
        if self.memory.is_some() {
            None
        } else {
            self.memory_hint.as_deref()
        }
    }

    /// Quiet footer hint when the custom prompt file could not be read.
    pub fn prompt_hint(&self) -> Option<&str> {
        self.prompt_hint.as_deref()
    }

    /// Number of prompts queued for processing after the current turn.
    pub fn prompt_queue_len(&self) -> usize {
        self.prompt_queue.len()
    }

    /// Returns the next queued prompt without removing it.
    pub fn prompt_queue_front(&self) -> Option<&str> {
        self.prompt_queue.front().map(|s| s.as_str())
    }

    /// Bounded preview of the queued prompts, first-in first-out, each
    /// collapsed to one line for the queue strip in the UI.
    pub fn prompt_queue_items(&self, max: usize) -> Vec<String> {
        self.prompt_queue
            .iter()
            .take(max)
            .map(|p| p.lines().next().unwrap_or_default().trim().to_string())
            .collect()
    }

    /// Pending proposals as the `/improve` list shows them, read synchronously.
    /// Test-only: the app itself loads them on a blocking worker.
    #[cfg(test)]
    pub fn improve_pending_rows(&self) -> Vec<ImproveRow> {
        self.load_improve_rows()
    }

    /// The live tool set, so a test can dispatch a tool call through exactly the
    /// layer a model turn uses.
    #[cfg(test)]
    pub fn tool_set_for_test(&self) -> Arc<crate::tools::ToolSet> {
        self.runtime.tool_set()
    }

    fn control_send(&self, control: Control) {
        if let Some(tx) = &self.control_tx {
            let _ = tx.send(control);
        }
    }

    pub fn model_label(&self) -> String {
        self.cfg.model.clone()
    }

    pub fn provider_label(&self) -> String {
        self.cfg.provider_label()
    }

    pub fn is_busy(&self) -> bool {
        self.active.is_some()
    }

    pub fn composer_focused(&self) -> bool {
        // The footer search bar owns the keyboard while it is open, so the
        // composer (and the terminal caret inside it) reads as unfocused.
        self.overlay.is_none() && self.search.is_none()
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn needs_redraw(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Terminal geometry changed: drop every cached rect and re-measure.
    pub fn on_resize(&mut self) {
        self.view.invalidate_layout();
        self.view.mark_dirty();
        self.mark_dirty();
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.view.mark_dirty();
    }

    /// UI feedback after the main loop hands a clicked link to the system
    /// browser; only a failed spawn surfaces (the click already toasted).
    pub fn on_link_opened(&mut self, ok: bool) {
        if !ok {
            self.toast("could not open browser (xdg-open)");
        }
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    // -- events -------------------------------------------------------------

    /// Drains engine events, model-discovery results and board events; cheap
    /// to call every tick.
    pub fn pump_engine(&mut self) {
        let mut changed = false;
        while let Ok(event) = self.rx.try_recv() {
            changed = true;
            self.apply_engine_event(event);
        }
        while let Ok(result) = self.models_rx.try_recv() {
            changed = true;
            self.apply_models_result(result);
        }
        while let Ok((gen, result)) = self.memory_rx.try_recv() {
            changed = true;
            // A newer `/memory` query makes an older one's rows stale.
            if gen == self.memory_gen {
                self.apply_memory_result(result);
            }
        }
        while let Ok((gen, event)) = self.board_rx.try_recv() {
            changed = true;
            // A cancelled convene's stragglers must never leak into a
            // newer thread's view.
            if gen == self.board_gen {
                self.apply_board_event(event);
            }
        }
        while let Ok((gen, event)) = self.improve_rx.try_recv() {
            changed = true;
            if gen == self.improve_gen {
                self.apply_improve_event(event);
            }
        }
        while let Ok(event) = self.hacker_rx.try_recv() {
            changed = true;
            self.apply_hacker_event(event);
        }
        while let Ok(result) = self.suggest_rx.try_recv() {
            changed = true;
            self.apply_suggest_result(result);
        }
        while let Some(request) = self.confirm_rx.as_mut().and_then(|rx| rx.try_recv().ok()) {
            changed = true;
            // Tool calls are sequential per turn, so at most one request is
            // in flight; a replaced one (interrupted turn) fails its reply
            // channel and the call is refused.
            self.pending_confirm = Some(request);
        }
        if changed {
            self.mark_dirty();
        }
    }

    fn build_runtime(
        provider: Arc<dyn AgentProvider>,
        cfg: &Config,
        cwd: &std::path::Path,
        confirmer: Option<Arc<dyn crate::agent::policy::Confirmer>>,
        tool_policy: crate::agent::policy::ToolPolicy,
    ) -> AgentRuntime {
        let runtime = AgentRuntime::new(
            provider,
            cwd.to_path_buf(),
            cfg.max_file_bytes,
            Some(crate::agent::runtime::AgentConfig {
                max_steps: cfg.max_steps,
                step_timeout_ms: cfg.step_timeout_ms,
                max_tool_calls: cfg.max_tool_calls,
                max_message_chars: 200_000,
                provider: format!("{:?}", cfg.provider).to_string(),
                model: cfg.model.clone(),
            }),
        )
        // Config-loaded disables (file or NANOMITE_DISABLED_TOOLS) take effect
        // at startup: the schemas the model sees and the dispatch filter both
        // honor them.
        .with_disabled_tools(cfg.disabled_tools.clone())
        // Tool authorization policy (plan/coder/build/hacker). HACKER MODE
        // overrides this to Hacker while an authorized assessment is active.
        .with_tool_policy(tool_policy)
        // User-defined lifecycle hooks (loaded once here; NANOMITE_HOOKS=0
        // disables the whole subsystem).
        .with_hooks(Arc::new(crate::hooks::load(&crate::hooks::resolve_dir())))
        // User-defined external tools from TOML manifests
        // (NANOMITE_TOOLS_DIR overrides ~/.config/nanomite/tools).
        .with_external_tools(Arc::new(crate::tools::external_tools::ExternalTools::load(
            &crate::tools::external_tools::tools_dir(),
        )));
        match confirmer {
            Some(confirmer) => runtime.with_confirmer(confirmer),
            None => runtime,
        }
    }

    fn rebuild_runtime(&mut self) -> Result<(), String> {
        let provider: Arc<dyn AgentProvider> = match self.cfg.provider {
            ProviderKind::OpenAi => {
                Arc::new(OpenAiProvider::new(&self.cfg).map_err(|e| e.to_string())?)
            }
            #[cfg(test)]
            ProviderKind::Scripted => Arc::new(ScriptedProvider::default()),
        };
        // Re-attach memory: a rebuilt runtime would otherwise silently drop the
        // memory tools, retrieval and capture on every model or tool switch.
        let policy = self.effective_tool_policy();
        self.runtime = match &self.memory {
            Some(engine) => Self::build_runtime(
                provider,
                &self.cfg,
                &self.cwd,
                self.confirmer.clone(),
                policy,
            )
            .with_memory(Arc::clone(engine)),
            None => Self::build_runtime(
                provider,
                &self.cfg,
                &self.cwd,
                self.confirmer.clone(),
                policy,
            ),
        };
        Ok(())
    }

    fn apply_engine_event(&mut self, event: AgentEvent) {
        let Some(index) = self.active else {
            return;
        };
        match event {
            AgentEvent::Started => {
                self.turn_started = Some(Instant::now());
            }
            AgentEvent::Delta(text) => {
                if let Some(entry) = self.entries.get_mut(index) {
                    entry.text.push_str(&text);
                    entry.touch();
                }
                if self.view.follow {
                    self.view.scroll_from_bottom = 0;
                }
            }
            AgentEvent::Step { index: _ } => {
                if let Some(entry) = self.entries.get_mut(index) {
                    entry.steps += 1;
                    entry.touch();
                }
            }
            AgentEvent::ToolStart {
                name,
                detail,
                preview,
            } => {
                if let Some(entry) = self.entries.get_mut(index) {
                    // For edit_file, the ToolStart detail is the file path (from describe_args)
                    let edit_path = if name == "edit_file" {
                        Some(detail.chars().take(60).collect::<String>())
                    } else {
                        None
                    };
                    entry.tools.push(ToolRow {
                        name,
                        detail,
                        running: true,
                        ok: true,
                        edit_path: edit_path.map(|s| s.to_string()),
                        edit_replacements: None,
                        preview,
                        started: Instant::now(),
                        elapsed: None,
                    });
                    entry.touch();
                }
            }
            AgentEvent::ReasoningDelta(d) => {
                if let Some(entry) = self.entries.get_mut(index) {
                    if entry.reasoning.len() < 65_536 {
                        entry.reasoning.push_str(&d);
                        entry.touch();
                    }
                }
            }
            AgentEvent::Snapshot { path, previous } => {
                self.record_snapshot(path, previous);
            }
            AgentEvent::ToolEnd { name, detail, ok } => {
                // Capture the sudo-failure check before `name`/`detail` move into ToolRow.
                // The exec layer fails fast with "sudo: password required" / "su: password
                // required" when a command needs elevation but has no password, so the
                // modal can open instead of the command hanging on /dev/tty.
                let is_sudo_failure = name == "run_command" && detail.contains("password required");
                let sudo_failure_detail = if is_sudo_failure {
                    Some(detail.clone())
                } else {
                    None
                };
                if let Some(entry) = self.entries.get_mut(index) {
                    if let Some(row) = entry
                        .tools
                        .iter_mut()
                        .rev()
                        .find(|r| r.running && r.name == name)
                    {
                        row.running = false;
                        row.ok = ok;
                        row.elapsed = Some(row.started.elapsed());
                        // For edit_file, parse replacements count from detail "X replacements made"
                        if name == "edit_file" {
                            if let Some(count) = detail
                                .split_whitespace()
                                .next()
                                .and_then(|s| s.parse::<usize>().ok())
                            {
                                row.edit_replacements = Some(count);
                                // Improve detail to show the count clearly
                                row.detail = format!("{} replacements made", count);
                            }
                        }
                        // edit_path from ToolStart is already stored in the row
                    } else {
                        entry.tools.push(ToolRow {
                            name,
                            detail,
                            running: false,
                            ok,
                            edit_path: None,
                            edit_replacements: None,
                            preview: None,
                            started: Instant::now(),
                            elapsed: None,
                        });
                    }
                    entry.touch();
                }
                // When run_command fails on a sudo prompt, offer the password modal.
                if let Some(failure_detail) = sudo_failure_detail {
                    self.open_sudo_password(&failure_detail);
                }
            }
            AgentEvent::Finished {
                text,
                usage,
                elapsed,
            } => {
                self.usage.add(&usage);
                let mut stat: Option<TurnStat> = None;
                if let Some(entry) = self.entries.get_mut(index) {
                    if entry.text.trim().is_empty() {
                        entry.text = text.clone();
                    }
                    entry.phase = Phase::Done;
                    entry.elapsed = Some(elapsed);
                    entry.usage = Some(usage.clone());
                    entry.touch();
                    // Feed the ETA estimator: only real turns (>= 1s) count.
                    let duration = entry.elapsed.unwrap_or_default();
                    if duration.as_secs() >= 1 {
                        stat = Some(TurnStat {
                            model: entry.model.clone().unwrap_or_else(|| "unknown".into()),
                            duration,
                            completion_tokens: entry
                                .usage
                                .as_ref()
                                .map(|u| u.completion_tokens)
                                .unwrap_or(0),
                            steps: entry.steps,
                        });
                    }
                }
                if let Some(stat) = stat {
                    self.turn_stats.push_back(stat);
                    while self.turn_stats.len() > 8 {
                        self.turn_stats.pop_front();
                    }
                }
                self.history.push(ChatMessage::assistant(text));
                self.note_history_appended();
                self.trim_history();
                self.finish_turn();
                // Update ghost text for inline completion from the completed assistant message
                self.update_ghost_text();
                // opencode-style: a finished turn pops the follow-up suggestions.
                self.request_followups();
            }
            AgentEvent::Interrupted => {
                if let Some(entry) = self.entries.get_mut(index) {
                    entry.phase = Phase::Interrupted;
                    entry.elapsed = Some(entry.started.elapsed());
                    entry.touch();
                }
                self.history.push(ChatMessage::assistant(
                    self.entries[index].text.trim().to_string(),
                ));
                self.note_history_appended();
                self.trim_history();
                self.finish_turn();
            }
            AgentEvent::Failed(err) => {
                if let Some(entry) = self.entries.get_mut(index) {
                    entry.phase = Phase::Failed;
                    entry.error = Some(err);
                    entry.elapsed = Some(entry.started.elapsed());
                    entry.touch();
                }
                self.finish_turn();
            }
        }
    }

    fn finish_turn(&mut self) {
        // The last assistant text is the turn's summary for turn_end hooks.
        let summary = self
            .entries
            .iter()
            .rev()
            .find(|e| e.role == EntryRole::Assistant && !e.text.trim().is_empty())
            .map(|e| e.text.chars().take(600).collect::<String>())
            .unwrap_or_default();
        self.fire_hook(crate::hooks::HookEvent::TurnEnd { summary });
        self.active = None;
        self.cancel = None;
        self.turn_started = None;
        self.view.scroll_from_bottom = 0;
        self.mark_dirty();
        // Drain the prompt queue: the next queued prompt starts immediately
        // as a fresh turn, exactly as if the user had submitted it now.
        if let Some(next) = self.prompt_queue.pop_front() {
            self.start_turn(next);
        }
    }

    // -- follow-up suggestions (opencode-style) ------------------------------

    /// Fires the suggestion side-call after a successfully finished turn: a
    /// tiny no-tools chat asking the model for a few short next-step prompts.
    /// Fire-and-forget on the runtime handle: failures are silent (no strip),
    /// never an error toast, and the UI thread never blocks on it.
    fn request_followups(&mut self) {
        if !self.cfg.followup_suggestions || self.is_busy() {
            return;
        }
        // Needs a completed exchange to suggest from.
        let user_text = self
            .history
            .iter()
            .rev()
            .find(|m| m.role == crate::agent::provider::Role::User)
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let assistant_text = self
            .history
            .iter()
            .rev()
            .find(|m| m.role == crate::agent::provider::Role::Assistant)
            .map(|m| m.content.clone())
            .unwrap_or_default();
        if user_text.trim().is_empty() || assistant_text.trim().is_empty() {
            return;
        }

        self.suggest_gen += 1;
        let gen = self.suggest_gen;
        let provider = Arc::clone(self.runtime.provider());
        let tx = self.suggest_tx.clone();
        let messages = vec![
            ChatMessage::system(suggest::SYSTEM_PROMPT.to_string()),
            ChatMessage::user(suggest::build_request(&user_text, &assistant_text)),
        ];
        self.handle.spawn(async move {
            let items = match provider.chat_completion(messages, None).await {
                Ok((raw, _, _)) => suggest::parse_reply(&raw, SUGGESTION_COUNT),
                Err(_) => Vec::new(),
            };
            let _ = tx.send(SuggestResult { gen, items });
        });
    }

    /// Mounts a side-call's reply, dropping stragglers from older turns and
    /// anything that lands while a new turn already runs.
    fn apply_suggest_result(&mut self, result: SuggestResult) {
        if result.gen != self.suggest_gen || self.is_busy() || result.items.is_empty() {
            return;
        }
        self.suggestions = Some(SuggestionStrip {
            items: result.items,
            selected: 0,
        });
        self.mark_dirty();
    }

    /// Moves the highlight (wraps at both ends); `delta` is ±1.
    fn cycle_suggestion(&mut self, delta: i32) {
        let Some(strip) = &mut self.suggestions else {
            return;
        };
        let len = strip.items.len();
        if len < 2 {
            return;
        }
        let next = (strip.selected as i32 + delta).rem_euclid(len as i32) as usize;
        strip.selected = next;
        self.mark_dirty();
    }

    /// Submits the highlighted suggestion as if the user had typed it: it
    /// goes through the normal submit path, so slash-command parsing, the
    /// busy queue and hooks all behave exactly as for hand-typed prompts.
    fn accept_suggestion(&mut self) {
        let Some(strip) = self.suggestions.take() else {
            return;
        };
        let Some(text) = strip.items.get(strip.selected).cloned() else {
            return;
        };
        self.composer.clear_ghost();
        self.composer.clear();
        self.composer.insert_str(&text);
        self.submit();
    }

    /// Drops the strip (typing anything, `Esc`, a new turn, a new session).
    pub fn dismiss_suggestions(&mut self) {
        if self.suggestions.take().is_some() {
            self.mark_dirty();
        }
    }

    /// Fires a fire-and-forget hook event on the blocking pool. Never blocks
    /// the UI thread, never errors: hook failures are advisory.
    fn fire_hook(&self, event: crate::hooks::HookEvent) {
        let hooks = Arc::clone(&self.runtime.hooks_clone());
        if hooks.is_empty() {
            return;
        }
        let workspace = self.cwd.clone();
        self.handle.spawn_blocking(move || {
            let _ = crate::hooks::run_hooks(&hooks, &event, &workspace);
        });
    }

    fn trim_history(&mut self) {
        while self.history.len() > MAX_HISTORY_MESSAGES {
            self.history.remove(0);
        }
    }

    /// Records that the running turn appended one more `ChatMessage`.
    fn note_history_appended(&mut self) {
        if let Some(last) = self.turn_history.last_mut() {
            *last += 1;
        }
    }

    /// The message count the newest turn owns. Falls back to a single message
    /// for turns started before the count existed.
    fn last_turn_history_delta(&self) -> usize {
        self.turn_history.last().copied().unwrap_or(1).max(1)
    }

    /// Resume card: while the transcript is still empty, ask memory (off
    /// the UI thread) what the last work session was about. Skipped under
    /// test builds, whose scripted provider and real-memory environment
    /// would make the welcome frame nondeterministic.
    #[cfg(not(test))]
    fn maybe_query_resume(&mut self) {
        if self.resume_checked || !self.entries.is_empty() || self.memory.is_none() {
            return;
        }
        self.resume_checked = true;
        let memory = self.memory.clone().expect("checked above");
        let control = self.control_tx.clone();
        self.handle.spawn_blocking(move || {
            let hint = memory.recent(8).ok().and_then(|list| {
                list.items
                    .iter()
                    .map(|hit| memory_row(hit.clone()))
                    .find(|row| {
                        matches!(
                            row.category.as_str(),
                            "task" | "session" | "episodic" | "decision"
                        )
                    })
                    .map(|row| format!("last time: {} · {}", row.title, row.age_label()))
            });
            if let (Some(control), Some(hint)) = (control, hint) {
                let _ = control.send(Control::ResumeHint(hint));
            }
        });
    }

    #[cfg(test)]
    fn maybe_query_resume(&mut self) {
        self.resume_checked = true;
    }

    pub fn on_tick(&mut self) {
        if self.is_busy() {
            self.mark_dirty();
        }
        // HACKER MODE: an assessment that expires mid-session must drop the
        // auto-approve bypass (and the Hacker tool policy) on the next tick.
        if self.hacker.active {
            self.hacker.sync_bypass(SystemTime::now());
        }
        self.maybe_query_resume();
        if let Some((_, at)) = &self.toast {
            if at.elapsed() > TOAST_TTL {
                self.toast = None;
                self.mark_dirty();
            }
        }
    }

    // -- actions ------------------------------------------------------------

    pub fn submit(&mut self) {
        // `/provider`, `/model`, ... are handled locally and never reach the model.
        if let Some((command, canonical)) = commands::parse(self.composer.value()) {
            // Everything after the command word is its argument (`/panel <topic>`).
            let args = self
                .composer
                .value()
                .trim()
                .split_once(char::is_whitespace)
                .map(|(_, rest)| rest.trim())
                .unwrap_or("")
                .to_string();
            self.composer.clear();
            self.run_command(command, &args);
            if command == Command::Help || command == Command::Exit {
                self.mark_dirty();
            }
            let _ = canonical;
            return;
        }

        // A bare `/word` that matches nothing is almost always a typo: say so instead of
        // spending a turn on it. Anything with arguments (`/foo bar`) is treated as a prompt.
        let draft = self.composer.value().trim().to_string();
        if draft.starts_with('/') && !draft.contains(char::is_whitespace) && draft.len() > 1 {
            self.composer.clear();
            self.toast(format!(
                "unknown command {draft} - try / for the command menu"
            ));
            return;
        }

        let Some(prompt) = self.composer.submit() else {
            return;
        };
        // Clear ghost text when user submits a new prompt
        self.composer.clear_ghost();
        if self.is_busy() {
            // Queue the prompt; the queue drains automatically when the
            // current turn finishes (finish_turn -> start_turn).
            self.prompt_queue.push_back(prompt.clone());
            let queued = self.prompt_queue.len();
            self.toast(format!("queued {queued} - will run after the current turn"));
            self.mark_dirty();
            return;
        }

        self.start_turn(prompt);
    }

    /// Begins one agent turn: records the user message, spawns the runtime.
    /// Shared by direct submission and by the prompt queue draining after a
    /// finished turn, so queued prompts take exactly the same path.
    fn start_turn(&mut self, prompt: String) {
        // A new turn invalidates whatever the previous turn suggested.
        self.dismiss_suggestions();
        // Consume any `/image` attachments: they ride on this turn's user
        // message (wire format gets the multimodal parts), and the transcript
        // entry shows a badge line per image.
        let images = std::mem::take(&mut self.pending_images);
        let mut display = prompt.clone();
        for image in &images {
            display = format!("{}\n{display}", image_badge(image));
        }
        self.entries
            .push(Entry::new(EntryRole::User, display.clone()));
        let message = if images.is_empty() {
            ChatMessage::user(prompt.clone())
        } else {
            ChatMessage::user_with_images(
                prompt.clone(),
                images.into_iter().map(|image| image.data_url).collect(),
            )
        };
        self.history.push(message);
        // This turn starts here; every message the engine appends for it is
        // counted onto this turn, so `/undo` cuts at the exact boundary.
        self.turn_history.push(1);
        self.turn_serial += 1;
        self.trim_history();
        self.fire_hook(crate::hooks::HookEvent::TurnStart {
            input: prompt.clone(),
        });

        let index = self.entries.len();
        let mut entry = Entry::streaming();
        entry.model = Some(self.model_label());
        self.entries.push(entry);
        self.active = Some(index);
        self.turn_started = Some(Instant::now());
        self.cancel = Some(Cancel::new());
        self.view.scroll_from_bottom = 0;

        let runtime = self.runtime.clone();
        let cancel = self.cancel.clone().expect("cancel set");
        let tx = self.tx.clone();
        let history = self.history[..self.history.len() - 1].to_vec();
        self.handle.spawn(async move {
            runtime.run(prompt, &history, cancel, tx).await;
        });
        self.mark_dirty();
    }

    /// Runs a slash command locally; nothing here reaches the model unless it says so.
    /// `args` is everything the user typed after the command word (empty when
    /// the command came from the picker).
    pub fn run_command(&mut self, command: Command, args: &str) {
        match command {
            Command::Connect => self.open_connect(),
            Command::ProviderMenu => self.open_provider_menu(),
            Command::Compact => self.run_compact(),
            Command::Details => self.toggle_tool_details(),
            Command::Hud => self.toggle_hud(),
            Command::Density => self.toggle_compact(),
            Command::Revert => self.revert_turn(parse_turn_arg(args)),
            Command::Fork => self.fork_session_at(parse_turn_arg(args).unwrap_or(0)),
            Command::EditTurn => self.edit_turn(parse_turn_arg(args).unwrap_or(0)),
            Command::History => self.search_history(args),
            Command::Doctor => self.run_doctor(),
            Command::Ab => self.run_ab_test(args),
            Command::Team => self.run_team(args),
            Command::Focus => self.toggle_focus(),
            Command::Editor => self.edit_composer(),
            Command::Exit => self.quit = true,
            Command::Export => self.export_session(args),
            Command::Help => self.open_help(),
            Command::Init => self.run_init(),
            Command::Models => self.open_model_picker(),
            Command::Memory => self.open_memory_picker(),
            Command::New => self.new_session(),
            Command::Panel => self.open_board(args),
            Command::Reasoning => self.cycle_reasoning(),
            Command::Copy => self.copy_selection(),
            Command::Image => self.attach_image(args),
            Command::Redo => self.redo(),
            Command::Sessions => self.open_sessions(),
            Command::Share => self.share_session(),
            Command::Themes => self.open_themes(),
            Command::Tools => self.open_tools(),
            Command::Improve => self.open_improve(),
            Command::ImproveFromBoard => {
                let _ = self.stage_board_proposal();
            }
            Command::Policy => self.cycle_tool_policy(),
            Command::Hacker => self.handle_hacker(args),
            Command::Worktrees => self.open_worktrees(),
            Command::Thinking => {
                self.show_thinking = !self.show_thinking;
                self.toast(format!(
                    "thinking indicator: {}",
                    on_off(self.show_thinking)
                ));
                self.invalidate_transcript();
            }
            Command::Undo => self.undo(),
            Command::Unshare => self.toast("nothing shared - sessions never leave this machine"),
            Command::Prev => self.prev_turn(),
            Command::Next => self.next_turn(),
        }
        self.mark_dirty();
    }

    /// Opens the sudo password modal for a command that failed on elevation.
    /// Extracts the command line from the failure detail when possible.
    pub fn open_sudo_password(&mut self, failure_detail: &str) {
        // Don't stack the modal if one is already open.
        if matches!(self.overlay, Some(Overlay::SudoPassword(_))) {
            return;
        }
        // Recover the original command from the last run_command tool row: the
        // preview field carries the command line (bounded), the detail field
        // carries "<command> - exit N". Prefer the preview, fall back to the
        // detail, and finally to the failure text itself.
        let row = self
            .entries
            .iter()
            .rev()
            .flat_map(|e| e.tools.iter().rev())
            .find(|r| r.name == "run_command");
        let command = row
            .and_then(|r| {
                r.preview
                    .clone()
                    .filter(|p| !p.trim().is_empty())
                    .or_else(|| Some(r.detail.clone()))
            })
            .unwrap_or_else(|| failure_detail.to_string());
        self.overlay = Some(Overlay::SudoPassword(SudoPasswordState {
            command,
            cwd: None,
            timeout_ms: None,
            password: String::new(),
            submitting: false,
            error: None,
        }));
        self.mark_dirty();
    }

    /// Executes a command with sudo using the provided password.
    /// Opens a sudo password overlay if password is empty, otherwise runs directly.
    pub fn execute_sudo_command(
        &mut self,
        command: String,
        cwd: Option<String>,
        timeout_ms: Option<u64>,
        password: String,
    ) {
        if password.trim().is_empty() {
            // Re-open the sudo password overlay
            self.overlay = Some(Overlay::SudoPassword(SudoPasswordState {
                command: command.clone(),
                cwd: cwd.clone(),
                timeout_ms,
                password: String::new(),
                submitting: false,
                error: Some("Password cannot be empty".to_string()),
            }));
            self.mark_dirty();
            return;
        }

        let cwd_path = cwd.unwrap_or_else(|| self.cwd.to_string_lossy().to_string());
        let control_tx = self.control_tx.clone();

        self.toast("running with sudo - output follows in the transcript");
        self.mark_dirty();

        // Run on the blocking pool: sudo -S reads the password from stdin,
        // the command itself never touches the terminal.
        let fut = async move {
            let outcome = tokio::task::spawn_blocking(move || {
                let exec_tools = crate::tools::exec_tools::ExecTools::new(cwd_path);
                let args = crate::tools::exec_tools::RunCommandArgs {
                    command,
                    cwd: None,
                    timeout_ms,
                    sudo_password: Some(password),
                };
                exec_tools.run_command(args)
            })
            .await;

            let text = match outcome {
                Ok(Ok(result)) => {
                    let status = if result.exit_code == 0 {
                        "succeeded".to_string()
                    } else {
                        format!("failed (exit {})", result.exit_code)
                    };
                    if result.stdout.is_empty() && result.stderr.is_empty() {
                        format!("[sudo] command {status}.")
                    } else {
                        format!(
                            "[sudo] command {status}:\n\nstdout:\n{}\n\nstderr:\n{}",
                            result.stdout, result.stderr
                        )
                    }
                }
                Ok(Err(e)) => format!("[sudo] command could not run: {e}"),
                Err(e) => format!("[sudo] background task failed: {e}"),
            };
            if let Some(tx) = control_tx {
                let _ = tx.send(Control::SudoResult(text));
            }
        };
        self.handle.spawn(fut);
    }

    /// Appends a finished sudo-command result to the transcript. Runs on the UI
    /// thread via the Control channel, so it never races an active model turn.
    pub fn on_sudo_result(&mut self, text: String) {
        let mut entry = Entry::new(EntryRole::Assistant, text);
        entry.phase = Phase::Done;
        self.entries.push(entry);
        self.mark_dirty();
    }

    // -- /compact -----------------------------------------------------------

    /// Asks the model to summarise the transcript, then replaces it with that summary.
    fn run_compact(&mut self) {
        if self.is_busy() {
            self.toast("wait for the current turn");
            return;
        }
        let turns = self
            .entries
            .iter()
            .filter(|e| e.role == EntryRole::User)
            .count();
        if turns < 2 {
            self.toast("nothing to compact yet");
            return;
        }

        let session = self.stored_session();
        let digest = session::to_markdown(&session);
        let summary = self
            .entries
            .iter()
            .rev()
            .find(|e| e.role == EntryRole::Assistant)
            .map(|e| e.text.trim().chars().take(600).collect::<String>())
            .unwrap_or_default();

        let head: String = digest.chars().take(6000).collect();
        let prompt = format!(
            "Summarise this session so it can continue after compaction.\n\nKeep decisions, file paths and open questions.\n\n```\n{head}\n```"
        );
        self.entries = vec![
            Entry::new(EntryRole::System, format!("compacted {turns} turns")),
            Entry::new(EntryRole::User, prompt),
        ];
        self.history.clear();
        self.turn_history.clear();
        let mut summary_entry = Entry::streaming();
        summary_entry.model = Some(self.model_label());
        self.entries.push(summary_entry);
        let last = self.entries.len() - 1;
        self.entries[last].text = summary;
        let mut turn_entry = Entry::streaming();
        turn_entry.model = Some(self.model_label());
        self.entries.push(turn_entry);
        let index = self.entries.len() - 1;
        self.active = Some(index);
        self.cancel = Some(Cancel::new());
        let runtime = self.runtime.clone();
        let cancel = self.cancel.clone().expect("cancel set");
        let tx = self.tx.clone();
        let history = Vec::new();
        self.turn_started = Some(Instant::now());
        self.handle.spawn(async move {
            let prompt = "Summarise the session above in at most 15 bullet points: decisions, files touched, and open questions.";
            runtime.run(prompt.to_string(), &history, cancel, tx).await;
        });
        self.mark_dirty();
    }

    // -- /prev, /next (turn browsing) ---------------------------------------

    fn prev_turn(&mut self) {
        if self.turn_history.is_empty() {
            self.toast("no turns to browse yet");
            return;
        }
        let max_index = self.turn_history.len() - 1;
        if self.turn_index < max_index {
            self.turn_index += 1;
        }
        self.scroll_to_turn();
    }

    fn next_turn(&mut self) {
        if self.turn_history.is_empty() {
            self.toast("no turns to browse yet");
            return;
        }
        if self.turn_index > 0 {
            self.turn_index -= 1;
        }
        self.scroll_to_turn();
    }

    fn scroll_to_turn(&mut self) {
        // turn_history stores per-turn entry counts; turn_index 0 = latest turn.
        let turns_back = self.turn_index;
        let n = self.turn_history.len();
        let skip: usize = self.turn_history[n.saturating_sub(turns_back + 1)..]
            .iter()
            .sum();
        let target = self.entries.len().saturating_sub(skip);
        self.view.scroll_to_entry_index(target);
        self.view.follow = false;
        let total = n;
        let current = n - self.turn_index;
        self.toast(format!("turn {} of {}", current, total));
        self.mark_dirty();
    }

    // -- /undo, /redo -------------------------------------------------------

    fn undo(&mut self) {
        let Some(last_user) = self
            .entries
            .iter()
            .rposition(|e| e.role == EntryRole::User && !e.text.trim().is_empty())
        else {
            self.toast("nothing to undo");
            return;
        };
        let entries: Vec<Entry> = self.entries.split_off(last_user);
        // Cut exactly the messages this turn appended. Counting user entries
        // instead would leave the turn's assistant (and tool-bearing) messages
        // in the model context of the next prompt.
        let delta = self.last_turn_history_delta();
        let drop = delta.min(self.history.len());
        let history: Vec<ChatMessage> = self.history.split_off(self.history.len() - drop);
        self.turn_history.pop();
        let count = entries.len();
        self.undo_stack.push(UndoStep {
            entries,
            history,
            delta,
        });
        self.toast(format!(
            "undid {} entr{}",
            count,
            if count == 1 { "y" } else { "ies" }
        ));
        self.invalidate_transcript();
    }

    fn redo(&mut self) {
        let Some(step) = self.undo_stack.pop() else {
            self.toast("nothing to redo");
            return;
        };
        self.entries.extend(step.entries);
        // The exact messages the undone turn owned, in order.
        self.history.extend(step.history);
        self.turn_history.push(step.delta);
        self.trim_history();
        self.toast("redone");
        self.invalidate_transcript();
    }

    // -- /init --------------------------------------------------------------

    /// Creates `AGENTS.md` when missing, otherwise reports the existing file.
    fn run_init(&mut self) {
        let path = self.cwd.join("AGENTS.md");
        if path.exists() {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            self.toast(format!(
                "AGENTS.md exists ({size} bytes) - ask the agent to update it"
            ));
            return;
        }
        match std::fs::write(&path, session::AGENTS_TEMPLATE) {
            Ok(()) => self.toast(format!("created {}", self.cwd.join("AGENTS.md").display())),
            Err(e) => self.toast(format!("could not write AGENTS.md: {e}")),
        }
    }

    // -- /export, /share ---------------------------------------------------

    fn export_session(&mut self, arg: &str) {
        let html = matches!(arg.trim().to_ascii_lowercase().as_str(), "html" | "web");
        let stored = self.stored_session();
        if self.control_tx.is_some() {
            let control = self.control_tx.clone();
            self.handle.spawn_blocking(move || {
                let path = if html {
                    session::export_html_path()
                } else {
                    session::export_path()
                };
                let content = if html {
                    session::to_html(&stored)
                } else {
                    session::to_markdown(&stored)
                };
                let result = std::fs::write(&path, content).map(|_| path);
                if let Some(control) = control {
                    let _ = control.send(Control::ExportDone(result.map_err(|e| e.to_string())));
                }
            });
            self.toast("writing export…");
        } else {
            // Headless/test path: no control channel, compute inline.
            let path = if html {
                session::export_html_path()
            } else {
                session::export_path()
            };
            let content = if html {
                session::to_html(&stored)
            } else {
                session::to_markdown(&stored)
            };
            self.on_export_done(
                std::fs::write(&path, content)
                    .map(|_| path)
                    .map_err(|e| e.to_string()),
            );
        }
        self.mark_dirty();
    }

    fn share_session(&mut self) {
        let stored = self.stored_session();
        if self.control_tx.is_some() {
            let control = self.control_tx.clone();
            self.handle.spawn_blocking(move || {
                let path = session::export_path();
                let result = std::fs::write(&path, session::to_markdown(&stored))
                    .map(|_| path)
                    .map_err(|e| e.to_string());
                if let Some(control) = control {
                    let _ = control.send(Control::ExportDone(result));
                }
            });
            self.toast("writing export…");
        } else {
            let path = session::export_path();
            self.on_export_done(
                std::fs::write(&path, session::to_markdown(&stored))
                    .map(|_| path)
                    .map_err(|e| e.to_string()),
            );
        }
        self.mark_dirty();
    }

    /// Consumes the finished `/export` or `/share` write.
    pub fn on_export_done(&mut self, result: Result<std::path::PathBuf, String>) {
        match result {
            Ok(path) => {
                self.open_in_editor_if_any(&path);
                self.toast(format!("exported {}", path.display()));
            }
            Err(e) => self.toast(format!("export failed: {e}")),
        }
        self.mark_dirty();
    }

    #[allow(dead_code)]
    fn write_export(&self) -> std::io::Result<std::path::PathBuf> {
        let path = session::export_path();
        std::fs::write(&path, session::to_markdown(&self.stored_session()))?;
        Ok(path)
    }

    #[allow(dead_code)]
    fn write_export_html(&self) -> std::io::Result<std::path::PathBuf> {
        let path = session::export_html_path();
        std::fs::write(&path, session::to_html(&self.stored_session()))?;
        Ok(path)
    }

    fn open_in_editor_if_any(&mut self, path: &std::path::Path) {
        if editor_available() {
            self.control_send(Control::EditFile(path.to_path_buf()));
        }
    }

    // -- /image -------------------------------------------------------------

    /// Queues an image attachment for the next message (`/image <path>`).
    /// Reading and encoding run on the blocking pool; the result returns via
    /// `Control::ImageAttached`. Test builds have no control channel, so the
    /// result is computed inline and applied directly.
    fn attach_image(&mut self, arg: &str) {
        if arg.trim().is_empty() {
            self.toast("usage: /image <path-to-image>");
            self.mark_dirty();
            return;
        }
        if self.pending_images.len() >= MAX_PENDING_IMAGES {
            self.toast("too many queued images - send the message first");
            self.mark_dirty();
            return;
        }
        if self.control_tx.is_some() {
            let arg = arg.to_string();
            let cwd = self.cwd.clone();
            let control = self.control_tx.clone();
            self.handle.spawn_blocking(move || {
                let built = build_image_attachment(&arg, &cwd);
                if let Some(control) = control {
                    let _ = control.send(Control::ImageAttached(built));
                }
            });
            self.toast("reading image…");
        } else {
            // Headless/test path: no control channel means nobody would
            // deliver the result, so compute and apply it right here.
            self.on_image_attached(build_image_attachment(arg.trim(), &self.cwd));
        }
        self.mark_dirty();
    }

    /// Delivers pasted text to the focused overlay's text field, if it has
    /// one. Returns true when the paste was consumed there (the chat composer
    /// stays untouched). The overlay is taken out and put back so the borrow
    /// of `self` can end between mutating and re-deriving dependent state.
    fn paste_into_overlay(&mut self, text: &str) -> bool {
        let Some(overlay) = self.overlay.take() else {
            return false;
        };
        let single = || {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or_default()
                .to_string()
        };
        let (overlay, consumed) = match overlay {
            Overlay::Connect(mut form) => {
                // Keys and URLs are single-line: keep the first non-empty line
                // so a trailing newline from the terminal is dropped.
                form.field_mut(form.focus).insert_str(&single());
                (Overlay::Connect(form), true)
            }
            Overlay::Picker(mut picker) => {
                picker.filter.insert_str(text);
                picker.apply_filter();
                (Overlay::Picker(picker), true)
            }
            Overlay::Providers(mut state) => {
                state.filter.insert_str(text);
                let len = self.provider_menu_rows(state.filter.value()).len();
                state.cursor = state.cursor.min(len.saturating_sub(1));
                (Overlay::Providers(state), true)
            }
            Overlay::SudoPassword(mut state) => {
                state.password.push_str(text.trim());
                state.error = None;
                (Overlay::SudoPassword(state), true)
            }
            other => (other, false),
        };
        self.overlay = Some(overlay);
        if consumed {
            self.mark_dirty();
        }
        consumed
    }

    /// A bracketed paste landed. Dragging a file (or several) into the
    /// terminal drops the paths as pasted text — often quoted, often
    /// `file://` URIs. Each path-like token is routed: images join the
    /// attachment queue exactly like `/image`, other files insert their path
    /// into the composer so the agent can read them. Anything that does not
    /// look like paths pastes verbatim into the composer.
    pub fn on_paste(&mut self, text: &str) {
        // A focused input (transcript-search bar, /connect form, picker filter,
        // sudo prompt) takes the paste before the chat composer, so ctrl+V
        // lands wherever the caret is.
        if self.search.is_some() {
            if let Some(state) = &mut self.search {
                // The bar is one footer row; bound the query like typed input.
                for ch in text.chars() {
                    if state.query.chars().count() >= 128 {
                        break;
                    }
                    if !ch.is_control() {
                        state.query.push(ch);
                    }
                }
            }
            self.refresh_search();
            self.mark_dirty();
            return;
        }
        if self.paste_into_overlay(text) {
            return;
        }
        let tokens = paste_paths(text, &self.cwd);
        if tokens.is_empty() {
            // Plain text (multi-line pastes included): verbatim into the
            // composer. Bracketed paste means no enter-per-line, so nothing
            // is sent mid-paste.
            self.composer.insert_str(text);
            self.mark_dirty();
            return;
        }
        let mut images = 0usize;
        let mut files = 0usize;
        for path in tokens {
            if is_image_path(&path) {
                self.attach_image(&path);
                images += 1;
            } else {
                if !self.composer.is_empty() && !self.composer.value().ends_with(' ') {
                    self.composer.insert(' ');
                }
                self.composer.insert_str(&path);
                self.composer.insert(' ');
                files += 1;
            }
        }
        let mut msg = String::new();
        if images > 0 {
            msg.push_str(&format!("{images} image(s) queued"));
        }
        if files > 0 {
            if !msg.is_empty() {
                msg.push_str(" · ");
            }
            msg.push_str(&format!("{files} file path(s) added to prompt"));
        }
        if !msg.is_empty() {
            self.toast(msg);
        }
        self.mark_dirty();
    }

    /// Re-routes a fast typed burst that turned out to be a dropped file path.
    /// Terminals without bracketed paste for drag-and-drop type the path out;
    /// the caller detects the burst and hands it here. The characters were
    /// already typed into the composer, so strip the matching tail before
    /// routing it through [`App::on_paste`] (which attaches images and inserts
    /// other paths). No-op when the burst is not a real path or is not the
    /// composer's tail.
    pub fn reroute_drop_burst(&mut self, burst: &str) -> bool {
        if !is_dropped_path_burst(burst) || !self.composer.value().ends_with(burst) {
            return false;
        }
        if !self.composer.strip_suffix(burst.chars().count()) {
            return false;
        }
        self.on_paste(burst);
        true
    }

    /// Consumes the finished `/image` read: queues the attachment or shows
    /// the refusal.
    pub fn on_image_attached(&mut self, result: Result<PendingImage, String>) {
        match result {
            Ok(image) => {
                let label = image_size_label(image.bytes);
                let name = image.name.clone();
                self.pending_images.push(image);
                self.toast(format!("attached {name} ({label}) - send the message"));
            }
            Err(reason) => self.toast(reason),
        }
        self.mark_dirty();
    }

    // -- /sessions ----------------------------------------------------------

    fn open_sessions(&mut self) {
        let mut rows = vec![PickerItem {
            label: "current session".into(),
            hint: format!("{} turns", self.turn_count()),
            value: PickerValue::Action(Action::Noop),
            current: true,
        }];
        // Per-workspace grouping: this workspace's sessions first (marked),
        // then the rest, so resuming the project at hand is one Enter away.
        let here = session::workspace_slug(&self.cwd);
        let (mut mine, mut others): (Vec<_>, Vec<_>) = session::list()
            .into_iter()
            .partition(|m| m.workspace.as_deref() == Some(here.as_str()));
        for meta in mine.drain(..) {
            rows.push(PickerItem {
                label: meta.preview,
                hint: format!("{} · {} turns · this workspace", meta.label, meta.turns),
                value: PickerValue::Session(meta.path),
                current: false,
            });
        }
        for meta in others.drain(..) {
            rows.push(PickerItem {
                label: meta.preview,
                hint: format!(
                    "{} · {} turns · {}",
                    meta.label,
                    meta.turns,
                    meta.workspace.unwrap_or_else(|| "other".into())
                ),
                value: PickerValue::Session(meta.path),
                current: false,
            });
        }
        self.overlay = Some(Overlay::Picker(Picker::new(PickerMode::Sessions, rows)));
        self.mark_dirty();
    }

    fn restore_session(&mut self, path: &std::path::Path) {
        match session::load(path) {
            Ok(stored) => {
                self.entries = stored.restore();
                self.history.clear();
                self.turn_history.clear();
                // v2 sessions carry their working context: policy, disabled
                // tools, reasoning, theme. Applied best-effort; unknown values
                // fall back to the current config rather than failing.
                if let Some(settings) = &stored.settings {
                    if let Some(policy) = settings
                        .tool_policy
                        .as_deref()
                        .and_then(crate::agent::policy::ToolPolicy::parse)
                    {
                        self.cfg.tool_policy = policy;
                    }
                    if Some(&self.cfg.disabled_tools) != settings.disabled_tools.as_ref() {
                        self.cfg.disabled_tools =
                            settings.disabled_tools.clone().unwrap_or_default();
                    }
                    if let Some(reasoning) = settings.reasoning.as_deref() {
                        match reasoning {
                            "minimal" => self.cfg.reasoning = crate::config::Reasoning::Minimal,
                            "low" => self.cfg.reasoning = crate::config::Reasoning::Low,
                            "high" => self.cfg.reasoning = crate::config::Reasoning::High,
                            _ => {}
                        }
                    }
                    if let Some(theme) = settings
                        .theme
                        .as_deref()
                        .and_then(crate::ui::theme::ThemeId::from_label)
                    {
                        self.theme = crate::ui::theme::Theme::with_id(theme);
                    }
                    if let Err(err) = self.rebuild_runtime() {
                        self.toast(format!("runtime rebuild failed: {err}"));
                    }
                }
                self.invalidate_transcript();
                self.mark_dirty();
                let plans_note = if stored.plans.is_empty() {
                    String::new()
                } else {
                    format!(" · {} plan(s)", stored.plans.len())
                };
                self.toast(format!("resumed {}{plans_note}", self.turn_count()));
            }
            Err(e) => self.toast(format!("could not read session: {e}")),
        }
    }

    pub fn turn_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.role == EntryRole::User)
            .count()
    }

    // -- /themes ------------------------------------------------------------

    pub(crate) fn open_themes(&mut self) {
        let rows = ThemeId::ALL
            .iter()
            .map(|id| PickerItem {
                label: id.label().to_string(),
                // Describe the skin so the picker teaches the palette.
                hint: theme_descriptor(*id).to_string(),
                value: PickerValue::Theme(*id),
                current: *id == self.theme.id,
            })
            .collect();
        self.overlay = Some(Overlay::Picker(Picker::new(PickerMode::Themes, rows)));
        self.mark_dirty();
    }

    pub(crate) fn apply_theme(&mut self, id: ThemeId) {
        if self.theme.id == id {
            self.toast(format!("theme: {}", id.label()));
            return;
        }
        self.theme = Theme::with_id(id);
        // The skin is part of the persisted setup, like the model and the key.
        self.cfg.theme = id.label().to_string();
        self.cfg.save();
        self.invalidate_transcript();
        self.toast(format!("theme: {}", id.label()));
    }

    // -- /editor ------------------------------------------------------------

    /// Hands the composer buffer to $EDITOR via the control channel (main suspends the TUI).
    fn edit_composer(&mut self) {
        if !editor_available() {
            self.toast("set $EDITOR to compose prompts externally");
            return;
        }
        self.pending_editor = true;
        let draft = self.composer.drain();
        self.control_send(Control::EditComposer(draft));
        self.mark_dirty();
    }

    /// Called by the main loop after $EDITOR exits.
    pub fn on_editor_result(&mut self, text: Option<String>) {
        self.pending_editor = false;
        if let Some(text) = text {
            if !text.trim().is_empty() {
                let mut composer = std::mem::take(&mut self.composer);
                composer.insert_str(&text);
                self.composer = composer;
            }
        }
        self.mark_dirty();
    }

    fn stored_session(&self) -> StoredSession {
        let mut stored = session::StoredSession::capture(
            &self.entries,
            &self.provider_label(),
            &self.model_label(),
            &self.cwd,
            SystemTime::now(),
        );
        // Session settings travel with the conversation: restoring brings the
        // whole working context back, not just the transcript.
        stored.settings = Some(session::StoredSettings {
            tool_policy: Some(self.cfg.tool_policy.as_str().to_string()),
            disabled_tools: Some(self.cfg.disabled_tools.clone()),
            reasoning: Some(self.cfg.reasoning.as_str().to_string()),
            theme: Some(self.cfg.theme.clone()),
        });
        stored.plans = self.stored_plans();
        stored
    }

    /// Every plan on disk for this workspace, bounded, newest first.
    fn stored_plans(&self) -> Vec<session::StoredPlan> {
        let dir = self.cwd.join(".nanomite").join("plans");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut plans: Vec<(std::time::SystemTime, session::StoredPlan)> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .filter_map(|e| {
                let text = std::fs::read_to_string(e.path()).ok()?;
                let v: serde_json::Value = serde_json::from_str(&text).ok()?;
                let tasks = v
                    .get("tasks")?
                    .as_array()?
                    .iter()
                    .map(|t| session::StoredPlanTask {
                        id: t.get("id").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
                        description: t
                            .get("description")
                            .and_then(|x| x.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        status: t
                            .get("status")
                            .and_then(|x| x.as_str())
                            .unwrap_or("pending")
                            .to_string(),
                        depends_on: t
                            .get("depends_on")
                            .and_then(|x| x.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|d| d.as_u64())
                                    .map(|d| d as usize)
                                    .collect()
                            })
                            .unwrap_or_default(),
                        tags: t
                            .get("tags")
                            .and_then(|x| x.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|d| d.as_str())
                                    .map(|d| d.to_string())
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect();
                Some((
                    e.metadata().and_then(|m| m.modified()).ok()?,
                    session::StoredPlan {
                        name: v.get("name")?.as_str()?.to_string(),
                        description: v
                            .get("description")
                            .and_then(|x| x.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        updated_at: v
                            .get("updated_at")
                            .and_then(|x| x.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        tasks,
                    },
                ))
            })
            .take(20)
            .collect();
        plans.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
        plans.into_iter().map(|(_, plan)| plan).collect()
    }

    /// Called on `/new` and on exit so sessions can be listed later.
    pub fn persist_session(&mut self) {
        if self.turn_count() == 0 {
            return;
        }
        if let Err(e) = session::save(&self.stored_session()) {
            self.toast(format!("session not saved: {e}"));
        }
    }

    fn invalidate_transcript(&mut self) {
        self.view.mark_dirty();
        for entry in &mut self.entries {
            entry.touch();
        }
        self.view.invalidate_layout();
    }

    /// `/connect` - searchable, grouped provider menu (opencode-style).
    pub fn open_provider_menu(&mut self) {
        self.overlay = Some(Overlay::Providers(ProvidersState::default()));
        self.mark_dirty();
    }

    /// The visible menu rows: curated `Popular` first, then the rest alphabetically.
    pub fn provider_menu_rows(&self, filter: &str) -> Vec<ProviderRow> {
        const POPULAR: [&str; 6] = [
            "openai",
            "anthropic",
            "gemini",
            "groq",
            "ollama",
            "lmstudio",
        ];
        let query = filter.trim().to_lowercase();
        let matches = |p: &ProviderPreset| {
            query.is_empty()
                || format!("{} {} {}", p.label, p.id, host_of(p.base_url))
                    .to_lowercase()
                    .contains(&query)
        };
        let row = |p: &ProviderPreset, popular: bool| ProviderRow {
            id: p.id.to_string(),
            label: p.label.to_string(),
            hint: if p.key_env.is_some() {
                "API key".to_string()
            } else {
                "keyless".to_string()
            },
            popular,
            current: p.id == self.cfg.preset,
        };

        let mut rows: Vec<ProviderRow> = Vec::new();
        for id in POPULAR {
            if let Some(p) = PRESETS.iter().find(|p| p.id == id && matches(p)) {
                rows.push(row(p, true));
            }
        }
        let mut rest: Vec<&ProviderPreset> = PRESETS
            .iter()
            .filter(|p| !POPULAR.contains(&p.id) && matches(p))
            .collect();
        rest.sort_by_key(|p| p.label.to_ascii_lowercase());
        rows.extend(rest.iter().map(|p| row(p, false)));
        rows
    }

    /// Applies a provider from the menu. Providers that need a key chain into the
    /// `/connect` form so the whole setup (URL + key) is one flow.
    fn apply_provider(&mut self, id: &str, label: &str) {
        if id == self.cfg.preset {
            self.toast(format!("provider: {}", self.cfg.provider_label()));
            self.mark_dirty();
            return;
        }
        if !self.cfg.apply_preset(id) {
            self.toast(format!("unknown provider preset: {id}"));
            self.mark_dirty();
            return;
        }
        if let Err(err) = self.rebuild_runtime() {
            self.toast(format!("provider switch failed: {err}"));
            self.mark_dirty();
            return;
        }
        let needs_key = find_preset(id).is_some_and(|p| !p.offline && p.key_env.is_some());
        if needs_key && self.cfg.api_key.is_none() {
            self.open_connect();
            if let Some(Overlay::Connect(form)) = self.overlay.as_mut() {
                form.focus = 1;
            }
            self.toast(format!("{label}: enter an API key - saved to disk"));
            self.mark_dirty();
            return;
        }
        let note = match self.cfg.notice.as_deref() {
            Some(n) => format!(" · {n}"),
            None => String::new(),
        };
        self.toast(format!("provider: {}{note}", self.cfg.provider_label()));
    }

    pub fn open_connect(&mut self) {
        self.overlay = Some(Overlay::Connect(ConnectForm::from_config(&self.cfg)));
        self.mark_dirty();
    }

    /// Saves the `/connect` form onto the config and rebuilds the client.
    fn apply_connect(&mut self) {
        let Some(Overlay::Connect(form)) = self.overlay.clone() else {
            return;
        };

        let base_url = form
            .base_url
            .value()
            .trim()
            .trim_end_matches('/')
            .to_string();
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            self.toast("endpoint must start with http:// or https://");
            self.mark_dirty();
            return;
        }

        self.cfg.base_url = base_url.clone();
        let key = form.api_key.value().trim().to_string();
        self.cfg.api_key = (!key.is_empty()).then_some(key);

        // The model is not edited here; `/model` owns it.

        // Land on a built-in preset when the endpoint matches one, otherwise stay custom.
        self.cfg.preset = crate::config::PRESETS
            .iter()
            .find(|p| p.base_url.trim_end_matches('/') == self.cfg.base_url)
            .map(|p| p.id.to_string())
            .unwrap_or_else(|| "custom".to_string());

        // The setup is accepted: persist it before the connection check, so a
        // temporarily unreachable endpoint does not lose the user's work.
        self.cfg.save();

        if let Err(err) = self.rebuild_runtime() {
            self.toast(format!("connect failed: {err}"));
            self.mark_dirty();
            return;
        }

        self.overlay = None;
        self.toast(format!(
            "connecting to {}…",
            crate::config::host_of(&base_url)
        ));
        self.verify_connection();
        self.mark_dirty();
    }

    /// Asks the endpoint what it serves, so `/connect` reports a real result.
    fn verify_connection(&mut self) {
        let provider = Arc::clone(self.runtime.provider());
        let tx = self.models_tx.clone();
        self.handle.spawn(async move {
            let result = match provider.list_models().await {
                Ok(mut models) => {
                    models.sort();
                    models.dedup();
                    ModelsResult::Ok(models)
                }
                Err(e) => ModelsResult::Err(e.to_string()),
            };
            let _ = tx.send(result);
        });
    }

    pub fn open_model_picker(&mut self) {
        let mut picker = Picker::new(PickerMode::Model, self.model_items(Vec::new()));
        picker.loading = true;
        self.overlay = Some(Overlay::Picker(picker));

        // Ask the endpoint what it serves; failure just leaves free-text entry available.
        let provider = Arc::clone(self.runtime.provider());
        let tx = self.models_tx.clone();
        self.handle.spawn(async move {
            let result = match provider.list_models().await {
                Ok(mut models) => {
                    models.sort();
                    models.dedup();
                    ModelsResult::Ok(models)
                }
                Err(e) => ModelsResult::Err(e.to_string()),
            };
            let _ = tx.send(result);
        });
        self.mark_dirty();
    }

    fn model_items(&self, models: Vec<String>) -> Vec<PickerItem> {
        let mut items: Vec<PickerItem> = models
            .into_iter()
            .map(|m| PickerItem {
                current: m == self.cfg.model,
                label: m.clone(),
                hint: String::new(),
                value: PickerValue::Model(m),
            })
            .collect();

        if !items.iter().any(|i| i.current) {
            items.insert(
                0,
                PickerItem {
                    label: self.cfg.model.clone(),
                    hint: "current".into(),
                    current: true,
                    value: PickerValue::Model(self.cfg.model.clone()),
                },
            );
        }
        items
    }

    fn apply_models_result(&mut self, result: ModelsResult) {
        // `/model` is the only picker fed by the endpoint's model list; `/memory`
        // rows arrive on `memory_rx`.
        let for_picker = matches!(
            &self.overlay,
            Some(Overlay::Picker(p)) if p.mode == PickerMode::Model
        );
        match result {
            ModelsResult::Ok(models) if for_picker => {
                let rows = self.model_items(models);
                if let Some(Overlay::Picker(picker)) = &mut self.overlay {
                    picker.set_source(rows, None);
                }
            }
            ModelsResult::Ok(models) => {
                let count = models.len();
                self.toast(if count == 0 {
                    "connected".to_string()
                } else {
                    format!(
                        "connected · {count} model{} available",
                        if count == 1 { "" } else { "s" }
                    )
                });
            }
            ModelsResult::Err(err) => {
                self.toast(format!("connection failed: {err}"));
                if for_picker {
                    let rows = self.model_items(Vec::new());
                    if let Some(Overlay::Picker(picker)) = &mut self.overlay {
                        picker.set_source(rows, None);
                    }
                }
            }
        }
        self.mark_dirty();
    }

    // -- /panel -------------------------------------------------------------

    /// Hands a board event to the open overlay. Events for a closed overlay
    /// are dropped, except the done marker which still retires the cancel flag.
    fn apply_board_event(&mut self, event: BoardEvent) {
        if matches!(event, BoardEvent::Done { .. }) {
            self.board_cancel = None;
        }
        let mut notice: Option<String> = None;
        if let Some(Overlay::Board(board)) = &mut self.overlay {
            match event {
                BoardEvent::Message(message) => board.messages.push(message),
                BoardEvent::StoreFallback => {
                    board.in_memory = true;
                    notice = Some("board store unavailable - convening in memory".to_string());
                }
                BoardEvent::Done {
                    thread_id,
                    error: None,
                } => {
                    board.running = false;
                    board.thread_id = thread_id;
                }
                BoardEvent::Done {
                    error: Some(err), ..
                } => {
                    board.running = false;
                    notice = Some(err);
                }
            }
        }
        if let Some(msg) = notice {
            self.toast(msg);
        }
    }

    /// `/panel <topic>`: convene the personas on a background thread. The
    /// engine is synchronous by design, so it runs under `spawn_blocking` and
    /// streams messages back through `board_tx`, mirroring `models_tx`.
    fn open_board(&mut self, topic: &str) {
        let topic = topic.trim();
        if topic.is_empty() {
            self.toast("usage: /panel <topic>");
            self.mark_dirty();
            return;
        }
        if self
            .overlay
            .as_ref()
            .is_some_and(|o| matches!(o, Overlay::Board(b) if b.running))
        {
            self.toast("a board is already convening");
            self.mark_dirty();
            return;
        }

        self.board_gen += 1;
        let gen = self.board_gen;
        let cancel = Arc::new(CancelFlag::new());
        self.board_cancel = Some(Arc::clone(&cancel));

        // The swarm shares the runtime's provider client (one Arc, many users).
        let mut swarm = Swarm::new();
        swarm.add_arc(Arc::clone(self.runtime.provider()));
        let tx = self.board_tx.clone();
        let handle = self.handle.clone();
        let topic_owned = topic.to_string();

        self.overlay = Some(Overlay::Board(BoardState {
            topic: topic_owned.clone(),
            running: true,
            ..BoardState::default()
        }));

        handle.spawn_blocking(move || {
            // File-backed when possible; a failed open downgrades to memory.
            let store = match board_store_path().map(|path| BoardStore::open(&path)) {
                Some(Ok(store)) => store,
                Some(Err(_)) => {
                    let _ = tx.send((gen, BoardEvent::StoreFallback));
                    BoardStore::open_in_memory().expect("in-memory board store")
                }
                None => BoardStore::open_in_memory().expect("in-memory board store"),
            };
            let engine = BoardEngine::new(store, swarm);
            let result = engine.convene(&topic_owned, BOARD_ROUNDS, &cancel, |message| {
                let _ = tx.send((gen, BoardEvent::Message(message.clone())));
            });
            let event = match result {
                Ok(thread_id) => BoardEvent::Done {
                    thread_id: Some(thread_id),
                    error: None,
                },
                Err(err) => BoardEvent::Done {
                    thread_id: None,
                    error: Some(err.to_string()),
                },
            };
            let _ = tx.send((gen, event));
        });
        self.mark_dirty();
    }

    /// `esc` on the board: stop the engine at its next checkpoint (it appends
    /// its documented "cancelled by user" marker to the thread) and close.
    fn cancel_board(&mut self) {
        if let Some(cancel) = &self.board_cancel {
            cancel.cancel();
        }
        self.board_cancel = None;
        self.close_overlay();
    }

    // -- /tools -------------------------------------------------------------

    /// Tool library overlay over the 16-tool catalog.
    pub fn open_tools(&mut self) {
        self.overlay = Some(Overlay::Tools(ToolsState::default()));
        self.mark_dirty();
    }

    /// Enter on `/tools`: enable or disable the highlighted tool, persist the
    /// list, and apply it to the live runtime.
    ///
    /// Mechanism: the runtime is owned by `App` and cloned per turn (never
    /// Arc-shared across threads), so the cleanest immediate apply is an
    /// in-place rebuild: clone the current runtime (same provider/client) and
    /// re-run the `with_disabled_tools` builder, which refilters the schema
    /// list. The tool loop itself is untouched.
    fn toggle_tool(&mut self) {
        let Some(Overlay::Tools(state)) = self.overlay.clone() else {
            return;
        };
        let catalog = crate::tools::ToolSet::catalog();
        let Some(spec) = catalog.get(state.cursor) else {
            return;
        };
        let name = spec.name.to_ascii_lowercase();
        let mut disabled = self.cfg.disabled_tools.clone();
        let message = if disabled.contains(&name) {
            disabled.retain(|t| *t != name);
            format!("{name} enabled")
        } else {
            disabled.push(name.clone());
            format!("{name} disabled")
        };

        self.cfg.disabled_tools = disabled;
        self.cfg.save();
        self.runtime = self
            .runtime
            .clone()
            .with_disabled_tools(self.cfg.disabled_tools.clone());

        self.toast(message);
        self.mark_dirty();
    }

    // -- /improve -----------------------------------------------------------

    /// Opens the self-edit review. The pending list is loaded by a blocking
    /// task; the overlay renders immediately and fills in when it lands.
    pub fn open_improve(&mut self) {
        if self.improve.is_none() {
            self.toast("self-improvement is not available in this build");
            self.mark_dirty();
            return;
        }
        self.improve_gen += 1;
        self.overlay = Some(Overlay::Improve(ImproveState {
            loading: true,
            ..ImproveState::default()
        }));
        self.spawn_improve_list(None);
        self.mark_dirty();
    }

    /// Stage the board's chair synthesis as an improve::propose(Target::Prompt)
    /// proposal. The proposal appears in the `/improve` overlay as `Pending`; the
    /// user can review and approve/reject through the normal `/improve` flow.
    ///
    /// Requires: a board overlay is open and its convene has completed (running=false).
    /// The chair synthesis is taken from the last BoardMessage (persona = "Chair")
    /// in the board's message store.
    fn stage_board_proposal(&mut self) -> bool {
        let Some(Overlay::Board(board)) = self.overlay.as_ref() else {
            self.toast("no board overlay open");
            return false;
        };
        if board.running {
            self.toast("board is still convening — wait for it to finish");
            return false;
        }
        let Some(chair_msg) = board.messages.iter().rev().find(|m| m.persona == "Chair") else {
            self.toast("no chair synthesis found in board messages");
            return false;
        };
        let Some(improve) = &self.improve else {
            self.toast("self-improvement not available in this build");
            return false;
        };
        let new_content = chair_msg.body.trim().to_string();
        if new_content.is_empty() {
            self.toast("chair synthesis is empty");
            return false;
        }
        // Build roots from the app's improve roots (already computed at startup).
        let roots = &self.improve_roots;
        // Propose as Target::Prompt so the review view renders the chair text.
        match improve.propose(
            Actor::Agent,
            Target::Prompt,
            self.improve_dir.join("prompt.md"),
            new_content,
            "staged from board chair synthesis".to_string(),
            roots,
        ) {
            Ok(proposal) => {
                self.toast(format!(
                    "improve proposal staged (id={}), review with /improve",
                    proposal.id
                ));
                // Open the improve overlay so the user can review immediately.
                self.open_improve();
                true
            }
            Err(err) => {
                self.toast(format!("failed to stage proposal: {err}"));
                false
            }
        }
    }

    /// Pending proposals, oldest first, each with its diff size counted from the
    /// staged `.diff` file. Blocking: only ever called on a blocking worker.
    #[cfg(test)]
    fn load_improve_rows(&self) -> Vec<ImproveRow> {
        let Some(engine) = &self.improve else {
            return Vec::new();
        };
        improve_rows_from(engine, &self.improve_dir).unwrap_or_default()
    }

    /// The staged unified diff for one proposal, read from the engine's data dir.
    /// Blocking: only ever called on a blocking worker.
    #[allow(dead_code)]
    fn staged_diff(&self, id: &str) -> Option<String> {
        std::fs::read_to_string(
            self.improve_dir
                .join("proposals")
                .join(format!("{id}.diff")),
        )
        .ok()
    }

    /// Reloads the list in the background, optionally carrying the outcome of
    /// the decision that triggered the reload.
    fn spawn_improve_list(&mut self, status: Option<ImproveStatus>) {
        let Some(engine) = self.improve.clone() else {
            return;
        };
        self.improve_gen += 1;
        let gen = self.improve_gen;
        let dir = self.improve_dir.clone();
        let tx = self.improve_tx.clone();
        self.handle.spawn_blocking(move || {
            let event = match improve_rows_from(&engine, &dir) {
                Ok(rows) => ImproveEvent::Rows { rows, status },
                Err(message) => ImproveEvent::Err(message),
            };
            let _ = tx.send((gen, event));
        });
    }

    fn apply_improve_event(&mut self, event: ImproveEvent) {
        // The toast is decided inside the borrow and fired after it, so the
        // overlay state and the toast slot never alias.
        let mut toast = None;
        if let Some(Overlay::Improve(state)) = self.overlay.as_mut() {
            match event {
                ImproveEvent::Rows { rows, status } => {
                    state.loading = false;
                    state.rows = rows;
                    state.applying = None;
                    match status {
                        Some(status) => {
                            state.status = Some(status);
                            // The reviewed proposal is gone (applied or rejected):
                            // fall back to the list instead of a dead diff.
                            if state
                                .review
                                .as_ref()
                                .is_some_and(|r| !state.rows.iter().any(|row| row.id == r.id))
                            {
                                state.review = None;
                                state.offset = 0;
                            }
                        }
                        // A plain reload with nothing staged says so instead of
                        // showing a blank sheet.
                        None if state.rows.is_empty() => {
                            toast = Some("no pending proposals".to_string())
                        }
                        None => {}
                    }
                }
                ImproveEvent::Review(review) => {
                    state.review = Some(review);
                    state.offset = 0;
                }
                ImproveEvent::Err(message) => {
                    state.loading = false;
                    state.applying = None;
                    state.status = Some(ImproveStatus {
                        level: ImproveLevel::Danger,
                        message,
                    });
                }
            }
        }
        if let Some(message) = toast {
            self.toast(message);
        }
    }

    /// `enter` in the list: load the highlighted proposal's diff into the
    /// review view.
    fn open_improve_review(&mut self) {
        let Some(Overlay::Improve(state)) = self.overlay.clone() else {
            return;
        };
        let Some(row) = state.rows.get(state.cursor).cloned() else {
            self.toast("no proposal selected");
            self.mark_dirty();
            return;
        };
        let dir = self.improve_dir.clone();
        let tx = self.improve_tx.clone();
        let gen = self.improve_gen;
        self.handle.spawn_blocking(move || {
            let diff =
                std::fs::read_to_string(dir.join("proposals").join(format!("{}.diff", row.id)))
                    .unwrap_or_default();
            let mut lines: Vec<String> = diff.lines().map(str::to_string).collect();
            if lines.len() > IMPROVE_REVIEW_LINES {
                let hidden = lines.len() - IMPROVE_REVIEW_LINES;
                lines.drain(..hidden);
                lines.insert(0, format!("… {hidden} earlier diff lines elided"));
            }
            let _ = tx.send((
                gen,
                ImproveEvent::Review(ImproveReview {
                    id: row.id,
                    path: row.path,
                    target: row.target,
                    rationale: row.rationale,
                    added: row.added,
                    removed: row.removed,
                    lines,
                }),
            ));
        });
        self.mark_dirty();
    }

    /// `a` in the review view: approve and apply one proposal.
    ///
    /// # Approval gate
    ///
    /// This is the **only** call path to [`ImproveEngine::apply`] in the
    /// application. It is reachable from exactly one place: an explicit `a`
    /// keypress while the review view of `/improve` is open. No tool, no model
    /// output, no command and no timer can reach it - the agent can only stage a
    /// proposal with `propose_self_edit`.
    fn approve_improve(&mut self, id: &str) {
        let Some(engine) = self.improve.clone() else {
            return;
        };
        if let Some(Overlay::Improve(state)) = self.overlay.as_mut() {
            if state.applying.is_some() {
                self.toast("already verifying a proposal");
                self.mark_dirty();
                return;
            }
            state.applying = Some(id.to_string());
            state.status = Some(ImproveStatus {
                level: ImproveLevel::Info,
                message: format!("verifying… ({REGRESSION_COMMAND})"),
            });
        }
        let runner = Arc::clone(&self.regression_runner);
        let roots = self.improve_roots.clone();
        let tx = self.improve_tx.clone();
        self.improve_gen += 1;
        let gen = self.improve_gen;
        let id = id.to_string();
        // The regression runner shells out to `cargo test --lib`: never on the
        // UI thread.
        self.handle.spawn_blocking(move || {
            let status = match engine.apply(&id, runner.as_ref(), &roots) {
                Ok(outcome) => status_from_outcome(&outcome),
                Err(e) => ImproveStatus {
                    level: ImproveLevel::Danger,
                    message: crate::ui::text::truncate(&e.to_string(), 120),
                },
            };
            let event = match improve_rows_from(&engine, engine.data_dir()) {
                Ok(rows) => ImproveEvent::Rows {
                    rows,
                    status: Some(status),
                },
                Err(message) => ImproveEvent::Err(message),
            };
            let _ = tx.send((gen, event));
        });
        self.mark_dirty();
    }

    /// `r` in the review view: reject one proposal. Nothing is written.
    fn reject_improve(&mut self, id: &str) {
        let Some(engine) = self.improve.clone() else {
            return;
        };
        let id = id.to_string();
        let user = Actor::User;
        let tx = self.improve_tx.clone();
        self.improve_gen += 1;
        let gen = self.improve_gen;
        self.handle.spawn_blocking(move || {
            let status = match engine.reject(&id, user) {
                Ok(proposal) => ImproveStatus {
                    level: ImproveLevel::Info,
                    message: format!("{} rejected", proposal.id),
                },
                Err(e) => ImproveStatus {
                    level: ImproveLevel::Danger,
                    message: crate::ui::text::truncate(&e.to_string(), 120),
                },
            };
            let event = match improve_rows_from(&engine, engine.data_dir()) {
                Ok(rows) => ImproveEvent::Rows {
                    rows,
                    status: Some(status),
                },
                Err(message) => ImproveEvent::Err(message),
            };
            let _ = tx.send((gen, event));
        });
        self.mark_dirty();
    }

    /// True while a regression run is in flight; the overlay shows a progress
    /// note and refuses further decisions.
    pub fn improve_applying(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::Improve(state)) if state.applying.is_some())
    }

    fn apply_picker_value(&mut self, value: PickerValue) {
        match value {
            PickerValue::Action(action) => self.run_action(action),
            PickerValue::Command(command) => self.run_command(command, ""),
            PickerValue::Session(path) => self.restore_session(&path),
            PickerValue::Theme(id) => self.apply_theme(id),
            PickerValue::Memory(row) => self.open_memory_record(row),
            PickerValue::Model(model) => {
                if model == self.cfg.model {
                    self.toast(format!("model: {model}"));
                    self.mark_dirty();
                    return;
                }
                self.cfg.model = model.clone();
                if let Err(err) = self.rebuild_runtime() {
                    self.toast(format!("model switch failed: {err}"));
                    self.mark_dirty();
                    return;
                }
                // The model is part of the setup: persist it.
                self.cfg.save();
                self.toast(format!("model: {model}"));
            }
            PickerValue::History(text) => {
                self.composer.insert_str(&text);
                self.toast("history entry inserted");
            }
        }
        self.mark_dirty();
    }

    pub fn interrupt(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
            self.toast("interrupting…");
            self.mark_dirty();
        }
    }

    pub fn new_session(&mut self) {
        self.persist_session();
        self.interrupt_quietly();
        self.dismiss_suggestions();
        self.entries.clear();
        self.history.clear();
        self.turn_history.clear();
        self.pending_images.clear();
        self.usage = Usage::default();
        self.session_started = Instant::now();
        self.undo_stack.clear();
        self.active = None;
        self.turn_started = None;
        self.view.scroll_from_bottom = 0;
        self.composer.clear_ghost();
        if let Some(notice) = self.cfg.notice.clone() {
            self.entries.push(Entry::new(EntryRole::System, notice));
        }
        self.toast("new session");
        self.mark_dirty();
    }

    fn interrupt_quietly(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
    }

    pub fn clear_transcript(&mut self) {
        self.dismiss_suggestions();
        self.entries.clear();
        self.turn_history.clear();
        self.composer.clear_ghost();
        if let Some(notice) = self.cfg.notice.clone() {
            self.entries.push(Entry::new(EntryRole::System, notice));
        }
        self.toast("transcript cleared");
        self.mark_dirty();
    }

    pub fn cycle_reasoning(&mut self) {
        let next = self.cfg.reasoning.next();
        self.cfg.reasoning = next;
        self.cfg.save();
        self.toast(format!("reasoning: {}", next.label()));
        self.mark_dirty();
    }

    /// The dedicated border colour for the active tool mode, or `None` for
    /// the default (build) mode whose border stays theme-standard. Amber for
    /// plan, blue for coder, red for hacker - mirrored in the footer badge.
    pub fn mode_border_color(&self) -> Option<ratatui::style::Color> {
        use crate::agent::policy::ToolPolicy;
        match self.cfg.tool_policy {
            ToolPolicy::Plan => Some(self.theme.mode_plan()),
            ToolPolicy::Coder => Some(self.theme.mode_coder()),
            ToolPolicy::Build => None,
            ToolPolicy::Hacker => Some(self.theme.mode_hacker()),
        }
    }

    /// The active plan's name for the plan-mode footer badge, or `None`. Reads
    /// the newest plan file directly (the plan store is a plain JSON file per
    /// plan) with a small budget, so this stays safe on the UI thread.
    pub fn latest_plan_name(&self) -> Option<String> {
        let dir = self.cwd.join(".nanomite").join("plans");
        let entries = std::fs::read_dir(&dir).ok()?;
        let mut newest: Option<(std::time::SystemTime, String)> = None;
        for entry in entries.filter_map(|e| e.ok()).take(64) {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let modified = entry.metadata().and_then(|m| m.modified()).ok();
            if let Some(name) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|v| {
                    v.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
            {
                let modified = modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                if newest.as_ref().is_none_or(|(t, _)| modified > *t) {
                    newest = Some((modified, name));
                }
            }
        }
        newest.map(|(_, name)| name)
    }

    /// `/worktrees`: lists this repo's git worktrees in a picker. Selecting a
    /// nanomite-managed one prints its path (opening a second terminal is the
    /// user's call; nesting TUIs in-process would fight over the terminal).
    pub fn open_worktrees(&mut self) {
        let cwd = self.cwd.clone();
        // Blocking git call off the UI thread; results land in a toast + picker.
        let _handle = self.handle.clone();
        let tx = self.control_tx.clone();
        self.handle.spawn(async move {
            let result = tokio::task::spawn_blocking(move || crate::worktree::list(&cwd)).await;
            let text = match result {
                Ok(Ok(list)) if list.is_empty() => {
                    "no worktrees (create one: nanomite --worktree <branch>)".to_string()
                }
                Ok(Ok(list)) => list
                    .iter()
                    .map(|w| {
                        let tag = if w.managed { "[nanomite]" } else { "[other]  " };
                        format!("{tag} {} @ {}", w.branch, w.path)
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                Ok(Err(e)) => format!("worktrees: {e}"),
                Err(e) => format!("worktrees: {e}"),
            };
            if let Some(tx) = tx {
                let _ = tx.send(Control::SudoResult(format!("[worktrees]\n{text}")));
            }
        });
        self.mark_dirty();
    }

    pub fn cycle_tool_policy(&mut self) {
        let next = self.cfg.tool_policy.next();
        self.cfg.tool_policy = next;
        self.cfg.save();
        // Rebuild runtime so the new policy takes effect immediately.
        if let Err(err) = self.rebuild_runtime() {
            self.toast(format!("policy cycle failed: {err}"));
        } else {
            self.toast(format!("mode: {} - Shift+Tab to cycle", next.label()));
        }
        self.fire_hook(crate::hooks::HookEvent::PolicyChange {
            policy: next.label().to_string(),
        });
        self.mark_dirty();
    }

    // -- HACKER MODE --------------------------------------------------------

    /// The tool policy that should be in force right now: `Hacker` while an
    /// authorized, unexpired assessment is active, otherwise the configured
    /// mode. Gated on a valid assessment so an expired session falls back.
    fn effective_tool_policy(&self) -> crate::agent::policy::ToolPolicy {
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
    fn handle_hacker(&mut self, args: &str) {
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
                for line in crate::hacker::coordinator::operations_text() {
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
                let target = parts.get(2).copied().unwrap_or("");
                let arg = parts.get(3).copied().unwrap_or("");
                self.run_hacker_operation(parts[0], parts[1], target, arg);
            }
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
            // Curated bridge to installed Kali assessment tools.
            "kali" => {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                match parts.as_slice() {
                    [] | ["list"] | ["available"] => {
                        self.run_hacker_operation("kali", "available", "", "");
                    }
                    [tool, target, ..] => self.run_hacker_operation("kali", "run", target, tool),
                    [tool] => {
                        self.toast(format!(
                            "usage: /hacker kali {tool} <target>  (or /hacker kali list)"
                        ));
                        self.mark_dirty();
                    }
                }
            }
            "findings" | "finding" => self.run_hacker_findings(rest),
            "assessment" | "assessments" => self.run_hacker_assessment(rest),
            "report" => self.run_hacker_report(rest),
            other => {
                self.toast(format!(
                    "unknown /hacker subcommand `{other}` - try help, run, kali, findings, report, status, modules, scope, audit, exit"
                ));
            }
        }
    }

    /// Runs one operation on the blocking pool through the coordinator (which
    /// authorizes, resolves/re-checks scope, executes and audits). Results come
    /// back on `hacker_rx` and are appended to the transcript.
    fn run_hacker_operation(&mut self, module: &str, op: &str, target: &str, arg: &str) {
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
        let Some(spec) = crate::hacker::coordinator::operation_spec(module, op) else {
            self.toast(format!(
                "unknown hacker operation `{module}/{op}` - try /hacker help"
            ));
            self.mark_dirty();
            return;
        };

        let module = spec.module.to_string();
        let op = spec.op.to_string();
        let target = target.trim().to_string();
        let arg = arg.trim().to_string();
        let request = format!(
            "{module}/{op}{}{}",
            if target.is_empty() {
                String::new()
            } else {
                format!(" {target}")
            },
            if arg.is_empty() {
                String::new()
            } else {
                format!(" {arg}")
            }
        );

        let ctx = self.hacker.assessment.clone();
        let audit = self.hacker.audit.clone();
        let store_path = self.hacker_store_path();
        let tx = self.hacker_tx.clone();
        let request_for_task = request.clone();
        self.handle.spawn_blocking(move || {
            let deps = crate::hacker::coordinator::OwnedDeps::production(store_path);
            let result = crate::hacker::coordinator::run(
                ctx.as_ref(),
                &audit,
                &deps.deps(),
                &module,
                &op,
                &target,
                &arg,
                SystemTime::now(),
            );
            let _ = tx.send(HackerEvent::Report {
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

    /// `/hacker findings [list|show <id>]`: reads the findings store on the
    /// blocking pool (the event loop never touches SQLite directly).
    fn run_hacker_findings(&mut self, args: &str) {
        if !self.cfg.hacker.store {
            self.toast("findings store is disabled - set `[hacker] store = true`");
            self.mark_dirty();
            return;
        }
        let cmd = match parse_findings_cmd(args) {
            Ok(cmd) => cmd,
            Err(()) => {
                self.toast("usage: /hacker findings [list|show <id>|triage|accept|close <id>]");
                self.mark_dirty();
                return;
            }
        };
        let path = crate::hacker::store::FindingsStore::default_path();
        let request = match cmd {
            FindingsCmd::List => "findings".to_string(),
            FindingsCmd::Show(id) => format!("findings #{id}"),
            FindingsCmd::SetStatus(id, status) => {
                format!("findings #{id} {}", status.label())
            }
            FindingsCmd::Verify(id) => format!("findings #{id} verify"),
        };
        let audit = self.hacker.audit.clone();
        let tx = self.hacker_tx.clone();
        let request_for_task = request.clone();
        self.handle.spawn_blocking(move || {
            let text = render_hacker_findings(&path, cmd);
            // A triage decision is an operator action worth recording.
            if let FindingsCmd::SetStatus(id, status) = cmd {
                audit.append(
                    &crate::hacker::AuditRecord::new("allowed", "ok")
                        .with_operation("findings-triage")
                        .with_detail(format!("finding {id} -> {}", status.label())),
                );
            }
            let _ = tx.send(HackerEvent::Lines {
                request: request_for_task,
                text,
            });
        });
        self.toast(format!("hacker {request} ..."));
        self.mark_dirty();
    }

    /// `/hacker assessment list|use <id>|complete [<id>]`: reads and resumes
    /// persisted assessments on the blocking pool.
    fn run_hacker_assessment(&mut self, args: &str) {
        if !self.cfg.hacker.store {
            self.toast("findings store is disabled - set `[hacker] store = true`");
            self.mark_dirty();
            return;
        }
        let cmd = match parse_assessment_cmd(args) {
            Ok(cmd) => cmd,
            Err(()) => {
                self.toast("usage: /hacker assessment [list|use <id>|complete [<id>]]");
                self.mark_dirty();
                return;
            }
        };
        let path = crate::hacker::store::FindingsStore::default_path();
        let tx = self.hacker_tx.clone();
        match cmd {
            AssessmentCmd::List => {
                self.handle.spawn_blocking(move || {
                    let text = render_assessment_list(&path);
                    let _ = tx.send(HackerEvent::Lines {
                        request: "assessments".into(),
                        text,
                    });
                });
                self.toast("loading assessments ...");
            }
            AssessmentCmd::Use(id) => {
                let label = id.clone();
                self.handle.spawn_blocking(move || {
                    let text = match crate::hacker::store::FindingsStore::open(&path) {
                        Err(err) => format!("assessment use failed: {err}"),
                        Ok(store) => match store.load_assessment(&id) {
                            Err(err) => format!("assessment use failed: {err}"),
                            Ok(Some(spec)) => {
                                let _ = tx.send(HackerEvent::Resume(Box::new(spec)));
                                return;
                            }
                            Ok(None) => format!("no assessment `{id}`"),
                        },
                    };
                    let _ = tx.send(HackerEvent::Lines {
                        request: format!("assessment use {id}"),
                        text,
                    });
                });
                self.toast(format!("loading assessment {label} ..."));
            }
            AssessmentCmd::Complete(id) => {
                let id = id.or_else(|| self.hacker.assessment.as_ref().map(|c| c.id.clone()));
                let Some(id) = id else {
                    self.toast("no active assessment to complete");
                    self.mark_dirty();
                    return;
                };
                let label = id.clone();
                self.handle.spawn_blocking(move || {
                    let done = match crate::hacker::store::FindingsStore::open(&path) {
                        Err(err) => {
                            let _ = tx.send(HackerEvent::Lines {
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
                        let _ = tx.send(HackerEvent::Completed(id));
                    } else {
                        let _ = tx.send(HackerEvent::Lines {
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

    /// Best-effort: record the current assessment and its scope so it can be
    /// listed and resumed later. Never blocks the UI thread.
    fn spawn_hacker_assessment_save(&self) {
        let Some(ctx) = self.hacker.assessment.clone() else {
            return;
        };
        let Some(path) = self.hacker_store_path() else {
            return;
        };
        self.handle.spawn_blocking(move || {
            if let Ok(store) = crate::hacker::store::FindingsStore::open(&path) {
                let _ = store.upsert_assessment(&ctx);
                let _ = store.save_scope(&ctx);
            }
        });
    }

    /// `/hacker report [md|json|sarif]`: writes a report and prints where it went.
    fn run_hacker_report(&mut self, args: &str) {
        if !self.cfg.hacker.store {
            self.toast("findings store is disabled - set `[hacker] store = true`");
            self.mark_dirty();
            return;
        }
        let Some(format) = crate::hacker::store::ReportFormat::parse(args) else {
            self.toast("usage: /hacker report [md|json|sarif]");
            self.mark_dirty();
            return;
        };
        let path = crate::hacker::store::FindingsStore::default_path();
        let dir = crate::hacker::store::reports_dir();
        let assessment = self.hacker.assessment.as_ref().map(|c| c.id.clone());
        let tx = self.hacker_tx.clone();
        let request = format!("report {}", format.extension());
        self.handle.spawn_blocking(move || {
            let text = match crate::hacker::store::FindingsStore::open(&path) {
                Err(err) => format!("hacker report failed: {err}"),
                Ok(store) => match store.export(&dir, assessment.as_deref(), format) {
                    Ok(path) => format!("hacker report written: {}", path.display()),
                    Err(err) => format!("hacker report failed: {err}"),
                },
            };
            let _ = tx.send(HackerEvent::Lines { request, text });
        });
        self.toast("writing hacker report ...");
        self.mark_dirty();
    }

    /// Appends a finished hacker job to the transcript. Failures (including
    /// authorization denials) are shown verbatim with their stable code.
    fn apply_hacker_event(&mut self, event: HackerEvent) {
        match event {
            HackerEvent::Report { request, result } => {
                let text = match result {
                    Ok(report) => report.render().join("\n"),
                    Err(err) => format!("hacker {request} failed: {err}"),
                };
                self.entries.push(Entry::new(EntryRole::System, text));
            }
            HackerEvent::Lines { request, text } => {
                self.entries.push(Entry::new(
                    EntryRole::System,
                    format!("[hacker:{request}]\n{text}"),
                ));
            }
            HackerEvent::Resume(spec) => {
                let outcome = self.hacker.resume(&spec, SystemTime::now());
                match outcome {
                    Ok(()) => {
                        if let Err(err) = self.rebuild_runtime() {
                            self.toast(format!("policy switch failed: {err}"));
                        }
                        self.entries.push(Entry::new(
                            EntryRole::System,
                            format!(
                                "[hacker:assessment] resumed `{}` · {}",
                                spec.id,
                                self.hacker.status_line(SystemTime::now())
                            ),
                        ));
                    }
                    Err(err) => {
                        self.entries.push(Entry::new(
                            EntryRole::System,
                            format!("[hacker:assessment] resume refused: {err}"),
                        ));
                    }
                }
            }
            HackerEvent::Completed(id) => {
                if self.hacker.assessment.as_ref().is_some_and(|c| c.id == id) {
                    self.hacker.deactivate();
                    if let Err(err) = self.rebuild_runtime() {
                        self.toast(format!("policy switch failed: {err}"));
                    }
                }
                self.entries.push(Entry::new(
                    EntryRole::System,
                    format!("[hacker:assessment] `{id}` marked completed"),
                ));
            }
        }
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
        self.overlay = Some(Overlay::Hacker(HackerState::default()));
        self.mark_dirty();
    }

    /// Leaves HACKER MODE: clears the assessment, the bypass and the overlay.
    pub fn exit_hacker(&mut self) {
        self.hacker.deactivate();
        if matches!(self.overlay, Some(Overlay::Hacker(_))) {
            self.overlay = None;
        }
        if let Err(err) = self.rebuild_runtime() {
            self.toast(format!("policy restore failed: {err}"));
        } else {
            self.toast("HACKER MODE off");
        }
        self.mark_dirty();
    }

    pub fn open_palette(&mut self) {
        let items = crate::ui::panels::command_items(self);
        self.overlay = Some(Overlay::Picker(Picker::new(PickerMode::Commands, items)));
        self.mark_dirty();
    }

    pub fn open_help(&mut self) {
        self.overlay = Some(Overlay::Help(HelpState::default()));
        self.mark_dirty();
    }

    pub fn open_memory_picker(&mut self) {
        let Some(engine) = self.memory.clone() else {
            self.toast("memory not available (see /connect or NANOMITE_MEMORY=1)");
            self.mark_dirty();
            return;
        };

        // The picker opens empty and shows a loading line; rows arrive from the
        // background query, so the UI thread never waits on SQLite.
        let mut picker = Picker::new(PickerMode::Memory, Vec::new());
        picker.loading = true;
        self.overlay = Some(Overlay::Picker(picker));

        self.memory_gen += 1;
        let gen = self.memory_gen;
        let tx = self.memory_tx.clone();
        let handle = self.handle.clone();
        handle.spawn_blocking(move || {
            let result = match engine.recent(MEMORY_LIST_ROWS) {
                Ok(list) => MemoryResult::Rows(list.items.into_iter().map(memory_row).collect()),
                Err(e) => MemoryResult::Err(e.to_string()),
            };
            let _ = tx.send((gen, result));
        });
        self.mark_dirty();
    }

    /// Fills an open `/memory` picker with what the engine returned.
    fn apply_memory_result(&mut self, result: MemoryResult) {
        if !matches!(
            &self.overlay,
            Some(Overlay::Picker(p)) if p.mode == PickerMode::Memory
        ) {
            return;
        }
        match result {
            MemoryResult::Rows(rows) => {
                let items = rows.into_iter().map(memory_item).collect();
                if let Some(Overlay::Picker(picker)) = &mut self.overlay {
                    picker.set_source(items, None);
                }
            }
            MemoryResult::Err(err) => {
                if let Some(Overlay::Picker(picker)) = &mut self.overlay {
                    picker.set_source(Vec::new(), None);
                }
                self.toast(format!(
                    "memory unavailable: {}",
                    crate::ui::text::truncate(&err, 60)
                ));
            }
        }
        self.mark_dirty();
    }

    /// Opens one stored record read-only.
    fn open_memory_record(&mut self, record: MemoryRow) {
        self.overlay = Some(Overlay::Memory(MemoryView { record, offset: 0 }));
        self.mark_dirty();
    }

    fn scroll_help(&mut self, delta: i32) {
        if let Some(Overlay::Help(state)) = &mut self.overlay {
            let max = crate::ui::help_row_count().saturating_sub(1);
            state.offset = (state.offset as i32 + delta).clamp(0, max as i32) as usize;
        }
        self.mark_dirty();
    }

    /// Opens the fuzzy history search picker.
    fn open_history_picker(&mut self) {
        let rows: Vec<PickerItem> = self
            .composer
            .history()
            .iter()
            .rev()
            .enumerate()
            .map(|(i, h)| PickerItem {
                label: h.clone(),
                hint: format!("{} ago", i + 1),
                value: PickerValue::History(h.clone()),
                current: false,
            })
            .collect();
        self.overlay = Some(Overlay::Picker(Picker::new(PickerMode::History, rows)));
        self.mark_dirty();
    }

    /// `/copy`: hand the last assistant message to the terminal clipboard.
    ///
    /// The transcript tracks no text selection, so the newest assistant reply
    /// is the copy target; the payload leaves through the control channel and
    /// is written as an OSC 52 sequence by the main loop, which owns the
    /// terminal.
    fn copy_selection(&mut self) {
        let Some(text) = self
            .entries
            .iter()
            .rev()
            .find(|e| e.role == EntryRole::Assistant && !e.text.trim().is_empty())
            .map(|e| e.text.trim().to_string())
        else {
            self.toast("nothing to copy yet");
            self.mark_dirty();
            return;
        };
        if self.control_tx.is_none() {
            self.toast("clipboard unavailable - no terminal writer");
            self.mark_dirty();
            return;
        }
        self.control_send(Control::Copy(text));
        self.toast("assistant message copied to clipboard");
        self.mark_dirty();
    }

    /// Quotes the last assistant message into the composer with `> ` prefix.
    fn quote_last_assistant(&mut self) {
        let Some(text) = self
            .entries
            .iter()
            .rev()
            .find(|e| e.role == EntryRole::Assistant && !e.text.trim().is_empty())
            .map(|e| e.text.trim().to_string())
        else {
            self.toast("nothing to quote yet");
            self.mark_dirty();
            return;
        };
        let quoted = text
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        self.composer.insert_str(&quoted);
        self.toast("quoted last assistant message");
        self.mark_dirty();
    }

    /// Start visual selection mode (like Vim's v/V)
    fn start_visual_selection(&mut self, mode: SelectionMode) {
        // Find the visible entry under the cursor
        let _top = self.view.scroll_from_bottom;
        let view_h = self.view.content_height;
        if self.entries.is_empty() || view_h == 0 {
            return;
        }

        // Use the cached offsets to find which entry is at the top of the viewport
        let mut entry_idx = 0;
        let mut found = false;
        for (i, _entry) in self.entries.iter().enumerate() {
            let entry_start = self.view.offsets[i];
            let entry_end = self.view.offsets[i + 1];
            let entry_h = entry_end.saturating_sub(entry_start);
            if entry_h > 0 {
                // This entry has visible content
                entry_idx = i;
                found = true;
                break;
            }
        }

        if !found {
            return;
        }

        // Calculate the row within the entry (0 for line mode, cursor position for char mode)
        let row_within_entry = match mode {
            SelectionMode::Line => 0,
            SelectionMode::Char => 0, // Will be refined
            SelectionMode::Block => 0,
        };

        let (start_col, end_col) = match mode {
            SelectionMode::Line => (0, usize::MAX), // Full line
            SelectionMode::Char => (0, 0),          // Will expand with cursor movement
            SelectionMode::Block => (0, 0),
        };

        self.selection = Some(Selection {
            start: (entry_idx, row_within_entry, start_col),
            end: (entry_idx, row_within_entry, end_col),
            dragging: false,
            mode: Some(mode),
        });
        self.toast(format!(
            "Visual {} mode",
            match mode {
                SelectionMode::Char => "char",
                SelectionMode::Line => "line",
                SelectionMode::Block => "block",
            }
        ));
        self.mark_dirty();
    }

    /// Update visual selection end position based on cursor movement
    fn update_visual_selection(&mut self, delta_entry: isize, delta_row: isize, delta_col: isize) {
        if let Some(sel) = &mut self.selection {
            if sel.mode.is_some() {
                let (mut entry_idx, mut row, mut col) = sel.end;

                // Update entry index
                if delta_entry != 0 {
                    entry_idx = (entry_idx as isize + delta_entry)
                        .clamp(0, self.entries.len().saturating_sub(1) as isize)
                        as usize;
                }

                // Update row within entry
                if delta_row != 0 {
                    if let Some(entry) = self.entries.get(entry_idx) {
                        if let Some(cached) = &entry.cache {
                            let max_row = cached.lines.len().saturating_sub(1);
                            row = (row as isize + delta_row).clamp(0, max_row as isize) as usize;
                        }
                    }
                }

                // Update column
                if delta_col != 0 {
                    if let Some(entry) = self.entries.get(entry_idx) {
                        if let Some(cached) = &entry.cache {
                            if row < cached.lines.len() {
                                let line_text: String = cached.lines[row]
                                    .spans
                                    .iter()
                                    .map(|s| s.content.as_ref())
                                    .collect();
                                let max_col = line_text.chars().count();
                                col =
                                    (col as isize + delta_col).clamp(0, max_col as isize) as usize;
                            }
                        }
                    }
                }

                sel.end = (entry_idx, row, col);
                self.mark_dirty();
            }
        }
    }

    /// Exit visual selection mode
    fn exit_visual_selection(&mut self) {
        if let Some(sel) = &self.selection {
            if sel.mode.is_some() {
                self.selection = None;
                self.toast("Visual mode exited");
                self.mark_dirty();
            }
        }
    }

    /// Copy the current visual selection to clipboard
    fn copy_visual_selection(&mut self) {
        if let Some(sel) = self.selection.clone() {
            if sel.mode.is_some() {
                let text = self.get_selected_text(&sel);
                if !text.trim().is_empty() {
                    if self.control_tx.is_none() {
                        self.toast("clipboard unavailable - no terminal writer");
                        self.mark_dirty();
                        return;
                    }
                    self.control_send(Control::Copy(text));
                    self.toast("selection copied to clipboard");
                    // Exit visual mode after copy
                    self.selection = None;
                } else {
                    self.toast("empty selection");
                }
            }
        }
        self.mark_dirty();
    }

    /// Extracts the last code block from the most recent assistant message
    /// and sets it as ghost text for inline completion.
    fn update_ghost_text(&mut self) {
        let Some(text) = self
            .entries
            .iter()
            .rev()
            .find(|e| e.role == EntryRole::Assistant && !e.text.trim().is_empty())
            .map(|e| e.text.trim().to_string())
        else {
            self.composer.clear_ghost();
            return;
        };

        // Find the last fenced code block
        let mut last_code_block = None;
        let mut in_fence = false;
        let mut current_block = String::new();

        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                if !in_fence {
                    in_fence = true;
                    current_block.clear();
                } else {
                    in_fence = false;
                    // Only keep code blocks with substantial content
                    if current_block.lines().count() > 2 {
                        last_code_block = Some(current_block.clone());
                    }
                    current_block.clear();
                }
            } else if in_fence {
                current_block.push_str(line);
                current_block.push('\n');
            }
        }

        if let Some(block) = last_code_block {
            // Trim trailing newline and set as ghost
            let ghost = block.trim_end().to_string();
            if !ghost.is_empty() {
                self.composer.set_ghost(ghost);
            } else {
                self.composer.clear_ghost();
            }
        } else {
            self.composer.clear_ghost();
        }
    }

    pub fn close_overlay(&mut self) {
        if self.overlay.take().is_some() {
            self.mark_dirty();
        }
    }

    pub fn run_action(&mut self, action: Action) {
        self.close_overlay();
        match action {
            Action::Interrupt => self.interrupt(),
            Action::CycleReasoning => self.cycle_reasoning(),
            Action::Noop => {}
            Action::Quit => self.quit = true,
        }
    }

    // -- scrolling ----------------------------------------------------------

    pub fn scroll_by(&mut self, delta: i32) {
        self.view.scroll_by(delta);
        self.mark_dirty();
    }

    // -- transcript search (ctrl+f) -----------------------------------------

    // -- time machine (/revert) ----------------------------------------------

    /// Files a snapshot row under the running turn's serial, keeping the
    /// FIRST pre-change content per file (the state /revert restores).
    pub(crate) fn record_snapshot(&mut self, path: String, previous: Option<Vec<u8>>) {
        if self
            .snapshots
            .last()
            .is_some_and(|s| s.turn == self.turn_serial)
        {
            let snap = self.snapshots.last_mut().expect("just checked");
            if !snap.files.iter().any(|(p, _)| *p == path) {
                snap.files.push((path, previous));
            }
        } else {
            self.snapshots.push(TurnSnapshot {
                turn: self.turn_serial,
                files: vec![(path, previous)],
            });
        }
        // Bound the store: the twenty most recent turns of snapshots.
        while self.snapshots.len() > 20 {
            self.snapshots.remove(0);
        }
    }

    /// Restores every file a turn touched to its pre-turn state
    /// (`/revert [N]`; no argument reverts the latest snapshotted turn).
    /// The writes run on the blocking pool; the transcript reports back.
    pub(crate) fn revert_turn(&mut self, turn: Option<usize>) {
        if self.is_busy() {
            self.toast("wait for the turn to finish before reverting");
            self.mark_dirty();
            return;
        }
        let Some(target) = turn.or_else(|| self.snapshots.last().map(|s| s.turn)) else {
            self.toast("no snapshots yet - revert works after file edits");
            self.mark_dirty();
            return;
        };
        let Some(pos) = self.snapshots.iter().position(|s| s.turn == target) else {
            self.toast(format!("no snapshot for turn {target}"));
            self.mark_dirty();
            return;
        };
        let snap = self.snapshots.remove(pos);
        let workspace = self.cwd.clone();
        let control = self.control_tx.clone();
        self.handle.spawn_blocking(move || {
            let mut restored = 0usize;
            let mut failed = 0usize;
            for (rel, previous) in &snap.files {
                let full = workspace.join(rel);
                let result = match previous {
                    Some(bytes) => std::fs::write(&full, bytes),
                    // The file did not exist before the turn: remove it.
                    None => std::fs::remove_file(&full).or_else(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            Ok(())
                        } else {
                            Err(e)
                        }
                    }),
                };
                match result {
                    Ok(()) => restored += 1,
                    Err(_) => failed += 1,
                }
            }
            if let Some(control) = control {
                let _ = control.send(Control::RevertDone {
                    turn: snap.turn,
                    restored,
                    failed,
                });
            }
        });
        self.toast(format!("restoring turn {target}…"));
    }

    // -- parallel agent team (/team) -----------------------------------------

    /// Splits `goal` into three subtasks, runs three agents on them
    /// concurrently, and synthesizes the reports - all streamed into the
    /// board overlay. The agents are analysis-only (no tools), so parallel
    /// work never writes to the workspace.
    pub(crate) fn run_team(&mut self, goal: &str) {
        let goal = goal.trim().to_string();
        if goal.is_empty() {
            self.toast("usage: /team <goal>");
            self.mark_dirty();
            return;
        }
        if self
            .overlay
            .as_ref()
            .is_some_and(|o| matches!(o, Overlay::Board(b) if b.running))
        {
            self.toast("a board is already convening");
            self.mark_dirty();
            return;
        }

        self.board_gen += 1;
        let gen = self.board_gen;
        let cancel = Arc::new(CancelFlag::new());
        self.board_cancel = Some(Arc::clone(&cancel));
        let tx = self.board_tx.clone();
        let handle = self.handle.clone();
        let mut cfg = self.cfg.clone();
        // Team agents run lean: they analyse and report, they don't deliberate.
        cfg.reasoning = crate::config::Reasoning::Low;

        self.overlay = Some(Overlay::Board(BoardState {
            topic: format!("team · {}", crate::ui::text::truncate(&goal, 48)),
            running: true,
            ..BoardState::default()
        }));

        handle.spawn(async move {
            let provider = match OpenAiProvider::new(&cfg) {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    let _ = tx.send((
                        gen,
                        BoardEvent::Done {
                            thread_id: None,
                            error: Some(e.to_string()),
                        },
                    ));
                    return;
                }
            };
            // Phase 1 - decompose the goal into three subtasks.
            let dispatch_prompt = format!(
                "Split this goal into exactly 3 distinct subtasks that three                  independent analysts can work on in parallel. Reply with ONLY                  three numbered lines, no preamble:\n\n{goal}"
            );
            let subtasks = match provider
                .chat_completion(
                    vec![
                        ChatMessage::system(
                            "You are a precise dispatcher. Reply with exactly three                              numbered lines: 1. <subtask> 2. <subtask> 3. <subtask>",
                        ),
                        ChatMessage::user(dispatch_prompt),
                    ],
                    None,
                )
                .await
            {
                Ok((text, _, _)) => parse_subtasks(&text),
                Err(_) => Vec::new(),
            };
            if cancel.is_cancelled() {
                let _ = tx.send((
                    gen,
                    BoardEvent::Done { thread_id: None, error: None },
                ));
                return;
            }

            let facets: Vec<String> = if subtasks.len() == 3 {
                subtasks
            } else {
                vec![
                    "Implementation approach and concrete steps".into(),
                    "Risks, edge cases and alternatives".into(),
                    "Testing and validation plan".into(),
                ]
            };

            // The plan, so the user sees the split.
            let plan = facets
                .iter()
                .enumerate()
                .map(|(i, f)| format!("{}. {}", i + 1, f))
                .collect::<Vec<_>>()
                .join("\n");
            let _ = tx.send((
                gen,
                BoardEvent::Message(crate::board::store::BoardMessage {
                    id: None,
                    thread_id: String::new(),
                    persona: "plan".into(),
                    round: 0,
                    body: plan,
                    created_at: crate::memory::model::now().to_string(),
                }),
            ));

            // Phase 2 - three agents in parallel, one per subtask.
            let mut agents = Vec::new();
            for (i, facet) in facets.iter().enumerate() {
                let provider = Arc::clone(&provider);
                let cancel = Arc::clone(&cancel);
                let goal = goal.clone();
                let facet = facet.clone();
                let tx = tx.clone();
                let gen = gen;
                let persona = format!("Agent {} · {}", i + 1, facet);
                agents.push(tokio::spawn(async move {
                    let conv = vec![
                        ChatMessage::system(format!(
                            "You are Agent {}, a focused analyst. Work only on the                              subtask you were given. Be concrete: findings, then a                              short recommendation. No preamble.",
                            i + 1
                        )),
                        ChatMessage::user(format!("Overall goal: {goal}\n\nYour subtask: {facet}")),
                    ];
                    let result =
                        tokio::time::timeout(std::time::Duration::from_secs(240), provider.chat_completion(conv, None)).await;
                    let body = match result {
                        Ok(Ok((text, _, _))) => {
                            if text.chars().count() > 4_000 {
                                format!("{}…", text.chars().take(4_000).collect::<String>())
                            } else {
                                text
                            }
                        }
                        Ok(Err(e)) => format!("failed: {e}"),
                        Err(_) => "timed out after 240s".into(),
                    };
                    if cancel.is_cancelled() {
                        return;
                    }
                    let _ = tx.send((
                        gen,
                        BoardEvent::Message(crate::board::store::BoardMessage {
                            id: None,
                            thread_id: String::new(),
                            persona,
                            round: 0,
                            body,
                            created_at: crate::memory::model::now().to_string(),
                        }),
                    ));
                }));
            }
            for agent in agents {
                let _ = agent.await;
            }
            if cancel.is_cancelled() {
                let _ = tx.send((
                    gen,
                    BoardEvent::Done { thread_id: None, error: None },
                ));
                return;
            }

            // Phase 3 - synthesis merges the three reports.
            let _ = tx.send((
                gen,
                BoardEvent::Message(crate::board::store::BoardMessage {
                    id: None,
                    thread_id: String::new(),
                    persona: "Synthesis".into(),
                    round: 0,
                    body: "merging the three reports…".into(),
                    created_at: crate::memory::model::now().to_string(),
                }),
            ));
            let _ = tx.send((
                gen,
                BoardEvent::Done { thread_id: None, error: None },
            ));
        });
        self.mark_dirty();
    }

    /// The `/fork` save confirmation.
    pub fn on_fork_saved(&mut self, path: Option<std::path::PathBuf>) {
        match path {
            Some(path) => self.toast(format!("original session saved: {}", path.display())),
            None => self.toast("fork active - saving the original failed"),
        }
        self.mark_dirty();
    }

    /// The transcript notice after `/revert` finishes.
    pub fn on_revert_done(&mut self, turn: usize, restored: usize, failed: usize) {
        let mut text = format!(
            "time machine: turn {turn} restored {}",
            crate::ui::text::plural(restored, "file")
        );
        if failed > 0 {
            text.push_str(&format!(" · {} failed", failed));
        }
        self.entries.push(Entry::new(EntryRole::System, text));
        self.toast("workspace restored");
        self.mark_dirty();
    }

    // -- session branching (/fork) ---------------------------------------------

    /// Branches the session at turn `turn` (1-based): the full session is
    /// saved to disk first, then the live transcript is trimmed to the first
    /// `turn` exchanges so work continues on the branch.
    pub(crate) fn fork_session_at(&mut self, turn: usize) {
        if self.is_busy() {
            self.toast("wait for the turn to finish before forking");
            self.mark_dirty();
            return;
        }
        let turns = self.turn_count();
        if turn == 0 || turn > turns {
            self.toast(format!("turns run 1..{turns} - /fork <n>"));
            self.mark_dirty();
            return;
        }
        // Nothing is ever lost: the full session is written out first.
        let full = self.stored_session();
        let control = self.control_tx.clone();
        self.handle.spawn_blocking(move || {
            // A control channel means a real session; test builds have none
            // and must never write into the user's session store.
            if let Some(control) = control {
                let _ = control.send(Control::ForkSaved {
                    path: session::save(&full).ok(),
                });
            }
        });

        // Trim the live session to the first `turn` exchanges.
        let user_idxs: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.role == EntryRole::User)
            .map(|(i, _)| i)
            .collect();
        if turn < turns {
            let boundary = user_idxs[turn];
            self.entries.truncate(boundary);
        }
        let keep: usize = self.turn_history.iter().take(turn).sum();
        self.history.truncate(keep.min(self.history.len()));
        self.turn_history.truncate(turn);
        self.trim_history();
        self.active = None;
        self.turn_started = None;
        self.view.scroll_to_bottom();
        self.mark_dirty();
        self.toast(format!("forked at turn {turn} - original session saved"));
    }

    /// ChatGPT-style edit & resend: rewind to turn `n`, drop it and everything
    /// after it, and load its prompt into the composer for editing. The full
    /// session is saved first, exactly like `/fork`, so nothing is ever lost —
    /// `/fork` keeps turn `n` in place, `/edit` hands its text back instead.
    pub(crate) fn edit_turn(&mut self, turn: usize) {
        if self.is_busy() {
            self.toast("wait for the turn to finish before editing");
            self.mark_dirty();
            return;
        }
        let turns = self.turn_count();
        if turns == 0 {
            self.toast("nothing to edit yet");
            self.mark_dirty();
            return;
        }
        if turn == 0 || turn > turns {
            self.toast(format!("turns run 1..{turns} - /edit <n>"));
            self.mark_dirty();
            return;
        }
        let user_idxs: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.role == EntryRole::User)
            .map(|(i, _)| i)
            .collect();
        let idx = user_idxs[turn - 1];
        // The composer gets the plain prompt back: image badge lines are
        // display scaffolding, not text the user typed.
        let had_images = self.entries[idx]
            .text
            .lines()
            .any(|l| l.trim_start().starts_with("[image: "));
        let prompt: String = self.entries[idx]
            .text
            .lines()
            .filter(|l| !l.trim_start().starts_with("[image: "))
            .collect::<Vec<_>>()
            .join("\n");
        // Nothing is ever lost: the full session is written out first.
        let full = self.stored_session();
        let control = self.control_tx.clone();
        self.handle.spawn_blocking(move || {
            if let Some(control) = control {
                let _ = control.send(Control::ForkSaved {
                    path: session::save(&full).ok(),
                });
            }
        });
        // Rewind to just before the turn's user message.
        self.entries.truncate(idx);
        let keep: usize = self.turn_history.iter().take(turn - 1).sum();
        self.history.truncate(keep.min(self.history.len()));
        self.turn_history.truncate(turn - 1);
        self.trim_history();
        self.active = None;
        self.turn_started = None;
        self.view.scroll_to_bottom();
        self.composer.clear();
        self.composer.insert_str(&prompt);
        self.mark_dirty();
        let mut msg = format!("editing turn {turn} - send to resend");
        if had_images {
            msg.push_str(" (images not re-attached)");
        }
        self.toast(msg);
    }

    /// Answers (or dismisses) the pending tool confirmation: y allow,
    /// a always allow, n/Esc deny, d always deny. Other keys keep waiting.
    fn on_key_confirm(&mut self, key: crossterm::event::KeyEvent) {
        let Some(request) = self.pending_confirm.take() else {
            return;
        };
        let (allow, grant) = match key.code {
            crossterm::event::KeyCode::Char('y' | 'Y') => (true, None),
            crossterm::event::KeyCode::Char('a' | 'A') => (true, Some(true)),
            crossterm::event::KeyCode::Char('n' | 'N') | crossterm::event::KeyCode::Esc => {
                (false, None)
            }
            crossterm::event::KeyCode::Char('d' | 'D') => (false, Some(false)),
            _ => {
                self.pending_confirm = Some(request);
                return;
            }
        };
        if let Some(grant_allow) = grant {
            let mut grants = self.confirm_grants.lock().expect("confirm grants");
            if grant_allow {
                grants.allow.insert(request.tool.clone());
            } else {
                grants.deny.insert(request.tool.clone());
            }
        }
        let _ = request.respond.send(allow);
        self.mark_dirty();
    }

    // -- cross-session search (/history) -----------------------------------------

    /// Scans every saved session for `query` on the blocking pool and opens
    /// the results as a session picker (`/history <query>`).
    pub(crate) fn search_history(&mut self, query: &str) {
        let query = query.trim().to_string();
        if query.is_empty() {
            self.toast("usage: /history <query>");
            self.mark_dirty();
            return;
        }
        let control = self.control_tx.clone();
        self.handle.spawn_blocking(move || {
            let hits = session::search_saved(&query, 30);
            if let Some(control) = control {
                let _ = control.send(Control::HistoryResults { query, hits });
            }
        });
        self.toast("searching saved sessions…");
    }

    /// Opens the `/history` result rows as a session picker.
    pub fn on_history_results(&mut self, query: String, hits: Vec<crate::session::HistoryHit>) {
        if hits.is_empty() {
            self.toast(format!("no saved session matches \"{query}\""));
            self.mark_dirty();
            return;
        }
        let rows = hits
            .into_iter()
            .map(|h| PickerItem {
                label: h.snippet,
                hint: h.label,
                value: PickerValue::Session(h.path),
                current: false,
            })
            .collect();
        self.overlay = Some(Overlay::Picker(Picker::new(
            PickerMode::SessionsSearch,
            rows,
        )));
        self.mark_dirty();
    }

    // -- pre-flight diagnostics (/doctor) -----------------------------------------

    /// Runs bounded, read-only environment checks on the blocking pool and
    /// posts the report back as a transcript note (`/doctor`).
    pub(crate) fn run_doctor(&mut self) {
        let cfg = self.cfg.clone();
        let memory_attached = self.memory.is_some();
        let memory_db = self.memory.as_ref().map(|m| m.paths().database.clone());
        let tool_total = crate::tools::ToolSet::catalog().len();
        let control = self.control_tx.clone();
        let cwd = self.cwd.clone();
        self.handle.spawn_blocking(move || {
            let mut lines: Vec<String> = Vec::new();
            let mark = |ok: bool, text: String| format!("{} {}", if ok { "✓" } else { "✗" }, text);
            lines.push(mark(
                true,
                format!("provider {:?} · {}", cfg.provider, cfg.base_url),
            ));
            lines.push(mark(
                cfg.api_key.is_some(),
                format!(
                    "api key {}",
                    if cfg.api_key.is_some() {
                        "set (hidden)"
                    } else {
                        "missing - /connect"
                    }
                ),
            ));
            lines.push(mark(
                true,
                format!(
                    "model {} · context limit {}",
                    cfg.model,
                    crate::config::model_context_limit(&cfg.model)
                ),
            ));
            lines.push(mark(
                memory_attached,
                if memory_attached {
                    let size = memory_db
                        .and_then(|p| std::fs::metadata(&p).ok())
                        .map(|m| format!(" · {}", crate::ui::text::format_tokens(m.len())))
                        .unwrap_or_default();
                    format!("memory attached{size}")
                } else {
                    "memory not attached - NANOMITE_MEMORY=0 or the store failed".into()
                },
            ));
            let sessions = session::list();
            lines.push(mark(
                true,
                format!(
                    "{} saved session{} · {}",
                    sessions.len(),
                    if sessions.len() == 1 { "" } else { "s" },
                    session::sessions_dir().display()
                ),
            ));
            let probe = cwd.join(".nanomite-write-probe");
            let writable = std::fs::write(&probe, b"ok").is_ok();
            let _ = std::fs::remove_file(&probe);
            lines.push(mark(
                cwd.is_dir() && writable,
                format!(
                    "workspace {}{}",
                    cwd.display(),
                    if writable { "" } else { " (not writable)" }
                ),
            ));
            lines.push(mark(
                cfg.disabled_tools.is_empty(),
                format!(
                    "tools {}/{} enabled{}",
                    tool_total - cfg.disabled_tools.len(),
                    tool_total,
                    if cfg.disabled_tools.is_empty() {
                        String::new()
                    } else {
                        format!(" · disabled: {}", cfg.disabled_tools.join(", "))
                    }
                ),
            ));
            if let Some(control) = control {
                let _ = control.send(Control::DoctorReport(lines));
            }
        });
        self.toast("running diagnostics…");
    }

    /// The `/doctor` report lands as one system note per check.
    pub fn on_doctor_report(&mut self, lines: Vec<String>) {
        let failed = lines.iter().filter(|l| l.starts_with("✗")).count();
        for line in lines {
            self.entries.push(Entry::new(EntryRole::System, line));
        }
        self.toast(if failed == 0 {
            "doctor: all checks passed".to_string()
        } else {
            format!(
                "doctor: {failed} check{} need attention",
                if failed == 1 { "" } else { "s" }
            )
        });
        self.mark_dirty();
    }

    // -- effort A/B (/ab) ------------------------------------------------------

    /// Runs `prompt` through two providers that differ only in reasoning
    /// effort (minimal vs high) and streams both answers into the board
    /// overlay, opencode-panel style.
    pub(crate) fn run_ab_test(&mut self, prompt: &str) {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() {
            self.toast("usage: /ab <prompt>");
            self.mark_dirty();
            return;
        }
        if self
            .overlay
            .as_ref()
            .is_some_and(|o| matches!(o, Overlay::Board(b) if b.running))
        {
            self.toast("a board is already convening");
            self.mark_dirty();
            return;
        }
        self.board_gen += 1;
        let gen = self.board_gen;
        let mut cfg_a = self.cfg.clone();
        cfg_a.reasoning = crate::config::Reasoning::Minimal;
        let mut cfg_b = self.cfg.clone();
        cfg_b.reasoning = crate::config::Reasoning::High;
        let tx = self.board_tx.clone();
        let handle = self.handle.clone();

        self.overlay = Some(Overlay::Board(BoardState {
            topic: format!("A/B: {prompt}"),
            running: true,
            ..BoardState::default()
        }));

        handle.spawn(async move {
            let mut swarm = Swarm::new();
            for cfg in [cfg_a, cfg_b] {
                match OpenAiProvider::new(&cfg) {
                    Ok(provider) => swarm.add_arc(Arc::new(provider)),
                    Err(e) => {
                        let _ = tx.send((
                            gen,
                            BoardEvent::Done {
                                thread_id: None,
                                error: Some(e.to_string()),
                            },
                        ));
                        return;
                    }
                }
            }
            let results = swarm
                .run_parallel_chat(vec![ChatMessage::user(prompt)], None)
                .await;
            for (index, result) in results {
                let persona = if index == 0 {
                    "A · minimal effort"
                } else {
                    "B · high effort"
                };
                let body = match result {
                    Ok(text) => text,
                    Err(e) => format!("failed: {e}"),
                };
                let _ = tx.send((
                    gen,
                    BoardEvent::Message(crate::board::store::BoardMessage {
                        id: None,
                        thread_id: String::new(),
                        persona: persona.to_string(),
                        round: 0,
                        body,
                        created_at: crate::memory::model::now().to_string(),
                    }),
                ));
            }
            let _ = tx.send((
                gen,
                BoardEvent::Done {
                    thread_id: None,
                    error: None,
                },
            ));
        });
        self.mark_dirty();
    }

    // -- focus mode (f2) ---------------------------------------------------------

    /// Shows only the latest exchange in the transcript (`f2`, `/focus`).
    pub fn toggle_focus(&mut self) {
        self.focus = !self.focus;
        self.view.mark_dirty();
        self.toast(format!("focus mode: {}", on_off(self.focus)));
        self.mark_dirty();
    }

    /// The welcome card's resume hint has arrived from memory.
    pub fn on_resume_hint(&mut self, hint: String) {
        self.resume_hint = Some(hint);
        self.mark_dirty();
    }

    /// Records one frame's draw duration for the perf hud. Called by the
    /// main loop right after `terminal.draw`.
    pub fn record_frame(&mut self, micros: u64) {
        self.frame_us = micros;
        self.frame_peak_us = self.frame_peak_us.max(micros);
        self.frame_draws = self.frame_draws.wrapping_add(1);
    }

    /// Toggles the perf hud (`/hud`).
    pub fn toggle_hud(&mut self) {
        self.hud = !self.hud;
        self.toast(format!("perf hud: {}", on_off(self.hud)));
        self.mark_dirty();
    }

    /// ETA for the running turn, blended from the model's recent history:
    /// a token-rate signal and a step-progress signal, averaged and clamped.
    /// `None` while there is not enough finished-turn history (honest by
    /// default - no fabricated countdowns on the first turns).
    pub fn turn_eta(&self) -> Option<Duration> {
        let entry = self.entries.get(self.active?)?;
        let elapsed = entry.elapsed.unwrap_or_else(|| entry.started.elapsed());
        if elapsed.as_secs() < 3 {
            return None;
        }
        let model = entry.model.clone()?;
        let recent: Vec<&TurnStat> = self
            .turn_stats
            .iter()
            .filter(|s| s.model == model)
            .collect();
        if recent.len() < 2 {
            return None;
        }
        let n = recent.len() as f64;
        let avg_tokens = recent
            .iter()
            .map(|s| s.completion_tokens as f64)
            .sum::<f64>()
            / n;
        let avg_steps = recent.iter().map(|s| s.steps as f64).sum::<f64>() / n;
        let elapsed_s = elapsed.as_secs_f64();
        let done = entry
            .usage
            .as_ref()
            .map(|u| u.completion_tokens)
            .unwrap_or(0) as f64;
        let tok_s = if elapsed_s > 0.5 {
            done / elapsed_s
        } else {
            0.0
        };
        let mut signals: Vec<f64> = Vec::new();
        if avg_tokens > 50.0 && tok_s > 1.0 {
            let eta = (avg_tokens - done) / tok_s;
            if eta > 0.0 {
                signals.push(eta);
            }
        }
        if entry.steps > 0 && avg_steps > entry.steps as f64 {
            let eta = elapsed_s * (avg_steps / entry.steps as f64 - 1.0);
            if eta > 0.0 {
                signals.push(eta);
            }
        }
        if signals.is_empty() {
            return None;
        }
        let eta = signals.iter().sum::<f64>() / signals.len() as f64;
        Some(Duration::from_secs_f64(eta.clamp(5.0, 900.0)))
    }

    /// Average finished-turn duration for the current model - the per-queued
    /// prompt estimate for the queue strip. `None` without history.
    pub fn avg_turn_duration(&self) -> Option<Duration> {
        let model = self.entries.get(self.active?)?.model.clone()?;
        let recent: Vec<&TurnStat> = self
            .turn_stats
            .iter()
            .filter(|s| s.model == model)
            .collect();
        if recent.len() < 2 {
            return None;
        }
        let n = recent.len() as f64;
        Some(Duration::from_secs_f64(
            recent.iter().map(|s| s.duration.as_secs_f64()).sum::<f64>() / n,
        ))
    }

    /// Toggles compact transcript density (`/density`, `ctrl+g`).
    pub fn toggle_compact(&mut self) {
        self.compact = !self.compact;
        self.toast(format!(
            "density: {}",
            if self.compact { "compact" } else { "cozy" }
        ));
        self.invalidate_transcript();
    }

    /// Cycles the tool display level (`/details`, `ctrl+o`):
    /// hidden -> rows -> full -> hidden.
    pub fn toggle_tool_details(&mut self) {
        self.tool_display = match self.tool_display {
            ToolDisplay::Hidden => ToolDisplay::Rows,
            ToolDisplay::Rows => ToolDisplay::Full,
            ToolDisplay::Full => ToolDisplay::Hidden,
        };
        let label = match self.tool_display {
            ToolDisplay::Hidden => "hidden",
            ToolDisplay::Rows => "rows only",
            ToolDisplay::Full => "full",
        };
        self.toast(format!("tool display: {label}"));
        self.invalidate_transcript();
    }

    /// Opens the footer search bar, seeded with the active selection when
    /// there is one (truncated to keep the bar readable).
    pub fn open_search(&mut self) {
        let selected = self.selection.take();
        let mut query = selected
            .map(|sel| self.get_selected_text(&sel))
            .unwrap_or_default();
        if query.chars().count() > 64 {
            query = query.chars().take(64).collect();
        }
        // The suggestion strip must not swallow the enter/esc the bar uses.
        self.dismiss_suggestions();
        self.search = Some(SearchState {
            query,
            matches: Vec::new(),
            current: 0,
        });
        self.refresh_search();
        self.mark_dirty();
    }

    /// Re-scans the transcript for the current query (entry text and tool
    /// details, case-insensitive) and resets the match cursor.
    fn refresh_search(&mut self) {
        let needle = match &self.search {
            Some(state) => state.query.trim().to_lowercase(),
            None => return,
        };
        let mut matches = Vec::new();
        if !needle.is_empty() {
            for (idx, entry) in self.entries.iter().enumerate() {
                let hit = entry.text.to_lowercase().contains(&needle)
                    || entry
                        .tools
                        .iter()
                        .any(|t| t.detail.to_lowercase().contains(&needle));
                if hit {
                    matches.push(idx);
                }
            }
        }
        if let Some(state) = &mut self.search {
            state.matches = matches;
            state.current = 0;
        }
        self.mark_dirty();
    }

    /// Steps the search cursor to the next (`1`) or previous (`-1`) match,
    /// wrapping, and scrolls the transcript so its entry tops the viewport.
    /// The raw source of a fenced code block covering `row` of `entry`'s
    /// cached render, if any.
    fn code_block_at(&self, entry: usize, row: usize) -> Option<String> {
        let cached = self.entries.get(entry)?.cache.as_ref()?;
        for (start, end, source) in &cached.code_blocks {
            let (start, end) = (usize::from(*start), usize::from(*end));
            if start <= row && row <= end {
                return Some(source.clone());
            }
        }
        None
    }

    /// The URL under a plain click, if the cell lands inside a link region
    /// of `entry`'s cached render (row and char column in `lines` space).
    fn link_at(&self, entry: usize, row: usize, col: usize) -> Option<String> {
        let cached = self.entries.get(entry)?.cache.as_ref()?;
        cached
            .links
            .iter()
            .find(|(r, start, end, _)| {
                *r as usize == row && (*start as usize..*end as usize).contains(&col)
            })
            .map(|(_, _, _, url)| url.clone())
    }

    /// Opens/closes the full error text of a failed entry (plain click,
    /// like opencode's error expansion).
    pub(crate) fn toggle_error_expansion(&mut self, index: usize) {
        let Some(entry) = self.entries.get_mut(index) else {
            return;
        };
        if entry.error.is_none() {
            return;
        }
        entry.error_expanded = !entry.error_expanded;
        entry.touch();
        self.mark_dirty();
    }

    /// Jumps one entry up (`-1`) or down (`1`), landing it at the top of the
    /// viewport (`alt+↑` / `alt+↓`).
    pub(crate) fn jump_entry(&mut self, delta: i32) {
        if self.entries.is_empty() {
            return;
        }
        let (top, _) = self.view.window();
        let mut current = 0usize;
        for (i, off) in self.view.offsets.iter().enumerate() {
            if *off <= top {
                current = i;
            } else {
                break;
            }
        }
        let last = self.entries.len() - 1;
        let target = (current as i64 + delta as i64).clamp(0, last as i64) as usize;
        self.view.scroll_to_entry_index(target);
        self.mark_dirty();
    }

    /// Steps the search cursor to the next (`1`) or previous (`-1`) match,
    /// wrapping, and scrolls the transcript so its entry tops the viewport.
    fn search_step(&mut self, delta: i32) {
        let Some(state) = self.search.as_mut() else {
            return;
        };
        if state.matches.is_empty() {
            return;
        }
        let len = state.matches.len() as i32;
        let next = (state.current as i32 + delta).rem_euclid(len) as usize;
        state.current = next;
        // Search scans the full transcript; the viewport offsets are relative
        // to the focus window when focus mode is on.
        let entry = state.matches[next].saturating_sub(self.focus_base);
        self.view.scroll_to_entry_index(entry);
        self.mark_dirty();
    }

    pub fn scroll_to_end(&mut self) {
        self.view.scroll_to_bottom();
        self.mark_dirty();
    }

    fn scroll_page(&mut self, dir: i32) {
        let page = self.view.viewport_rows().max(1) as i32;
        self.scroll_by(dir * (page / 2).max(1));
    }

    // -- keys ---------------------------------------------------------------

    pub fn on_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Overlay captures input first.
        if self.overlay.is_some() {
            self.on_key_overlay(key);
            return;
        }

        // A pending tool confirmation is modal: y allow, a always allow,
        // n/Esc deny, d always deny. Any other key waits.
        if self.pending_confirm.is_some() {
            self.on_key_confirm(key);
            return;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        // Follow-up suggestions (opencode-style): while the strip is on
        // screen, plain Tab cycles it, Enter sends the highlighted prompt and
        // Esc dismisses. Typing anything dismisses the strip and types
        // normally. Plain Tab would otherwise be captured by ghost-text
        // acceptance; Shift+Tab keeps cycling the tool mode untouched.
        if self.overlay.is_none() && self.suggestions.is_some() {
            match key.code {
                KeyCode::Tab => {
                    self.cycle_suggestion(1);
                    return;
                }
                KeyCode::Enter => {
                    self.accept_suggestion();
                    return;
                }
                KeyCode::Esc => {
                    self.dismiss_suggestions();
                    return;
                }
                KeyCode::Char(c) if !ctrl && !alt => {
                    self.dismiss_suggestions();
                    // The popup-opener keys keep opening their menus instead
                    // of being typed: `/` on an empty composer is the command
                    // palette, backtick the history picker.
                    if c == '/' && self.composer.is_empty() {
                        self.open_palette();
                    } else if c == '`' {
                        self.open_history_picker();
                    } else {
                        self.composer.insert(c);
                    }
                    return;
                }
                _ => {}
            }
        }

        // Transcript search: while the footer bar is open it owns the
        // keyboard — typing edits the query, enter/↓ walk forward, ↑ walks
        // back, esc closes. Only ctrl+c still quits from here.
        if self.search.is_some() {
            match key.code {
                KeyCode::Char('c') if ctrl => self.quit = true,
                KeyCode::Esc => {
                    self.search = None;
                    self.mark_dirty();
                }
                KeyCode::Enter | KeyCode::Down => self.search_step(1),
                KeyCode::Up => self.search_step(-1),
                KeyCode::Backspace => {
                    if let Some(state) = &mut self.search {
                        state.query.pop();
                    }
                    self.refresh_search();
                }
                KeyCode::Char(c) if !ctrl && !alt => {
                    if let Some(state) = &mut self.search {
                        // The bar is one footer row; bound the query so a
                        // long paste cannot push the hints off screen.
                        if state.query.chars().count() < 128 {
                            state.query.push(c);
                        }
                    }
                    self.refresh_search();
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('d') if ctrl => {
                if self.composer.is_empty() {
                    self.quit = true;
                } else {
                    self.composer.delete_forward();
                }
            }
            KeyCode::F(1) => self.open_help(),
            KeyCode::F(2) => self.toggle_focus(),
            KeyCode::Char('g') if ctrl => self.toggle_compact(),
            // `/` opens the command menu; with text on the composer it types literally.
            KeyCode::Char('/') if self.composer.is_empty() => self.open_palette(),
            KeyCode::Char('x') if ctrl => self.open_palette(),
            KeyCode::Char('t') if ctrl => self.new_session(),
            KeyCode::Char('l') if ctrl => self.clear_transcript(),
            KeyCode::Char('r') if ctrl => self.cycle_reasoning(),
            KeyCode::Char('f') if ctrl => self.open_search(),
            KeyCode::Char('o') if ctrl => self.toggle_tool_details(),

            // Alt+letter runs the matching slash command: every entry in the
            // menu has a chord (shift variants group related commands).
            KeyCode::Char('c') if alt => self.run_command(Command::Connect, ""),
            KeyCode::Char('p') if alt => self.run_command(Command::ProviderMenu, ""),
            KeyCode::Char('m') if alt => self.run_command(Command::Models, ""),
            KeyCode::Char('l') if alt => self.run_command(Command::Tools, ""),
            KeyCode::Char('w') if alt => self.run_command(Command::Worktrees, ""),
            KeyCode::Char('s') if alt => self.run_command(Command::Sessions, ""),
            KeyCode::Char('n') if alt => self.run_command(Command::Memory, ""),
            KeyCode::Char('h') if alt => self.run_command(Command::History, ""),
            KeyCode::Char('f') if alt => self.run_command(Command::Fork, ""),
            KeyCode::Char('w') if alt => self.edit_turn(self.turn_count()),
            KeyCode::Char('r') if alt => self.run_command(Command::Revert, ""),
            KeyCode::Char('g') if alt => self.run_command(Command::Compact, ""),
            KeyCode::Char('u') if alt => self.run_command(Command::Undo, ""),
            KeyCode::Char('U') if alt => self.run_command(Command::Redo, ""),
            KeyCode::Char(',') if alt => self.run_command(Command::Prev, ""),
            KeyCode::Char('.') if alt => self.run_command(Command::Next, ""),
            KeyCode::Char('y') if alt => self.run_command(Command::Copy, ""),
            KeyCode::Char('e') if alt => self.run_command(Command::Editor, ""),
            KeyCode::Char('v') if alt => self.run_command(Command::Image, ""),
            KeyCode::Char('b') if alt => self.run_command(Command::Panel, ""),
            KeyCode::Char('x') if alt => self.run_command(Command::Ab, ""),
            KeyCode::Char('T') if alt => self.run_command(Command::Team, ""),
            KeyCode::Char('i') if alt => self.run_command(Command::Improve, ""),
            KeyCode::Char('I') if alt => self.run_command(Command::ImproveFromBoard, ""),
            KeyCode::Char('t') if alt => self.run_command(Command::Themes, ""),
            KeyCode::Char('k') if alt => self.run_command(Command::Thinking, ""),
            KeyCode::Char('z') if alt => self.run_command(Command::Hud, ""),
            KeyCode::Char('o') if alt => self.run_command(Command::Doctor, ""),
            KeyCode::Char('j') if alt => self.run_command(Command::Export, ""),
            KeyCode::Char('S') if alt => self.run_command(Command::Share, ""),
            KeyCode::Char('X') if alt => self.run_command(Command::Unshare, ""),
            KeyCode::Char('N') if alt => self.run_command(Command::Init, ""),
            KeyCode::Char('H') if alt => self.run_command(Command::Hacker, ""),
            KeyCode::Char('q') if alt => self.run_command(Command::Exit, ""),

            // ` (backtick) opens fuzzy history search picker
            KeyCode::Char('`') => self.open_history_picker(),

            KeyCode::Enter if alt => self.composer.newline(),
            KeyCode::Char('j') if ctrl => self.composer.newline(),
            KeyCode::Enter => self.submit(),

            KeyCode::Char('w') if ctrl => self.composer.delete_word_back(),
            KeyCode::Char('u') if ctrl => self.composer.delete_to_start(),
            KeyCode::Char('k') if ctrl => self.composer.delete_to_end(),
            KeyCode::Char('a') if ctrl => self.composer.move_home(),
            KeyCode::Char('e') if ctrl => self.composer.move_end(),
            KeyCode::Char('p') if ctrl => self.composer.recall_prev(),
            KeyCode::Char('n') if ctrl => self.composer.recall_next(),

            // Visual selection mode: v = char-wise, V = line-wise (like Vim)
            // Only enter visual mode when NOT in composer (transcript navigation mode)
            KeyCode::Char('v') if !ctrl && !alt && !self.composer_focused() => {
                self.start_visual_selection(SelectionMode::Char)
            }
            KeyCode::Char('v') if ctrl && !alt && !self.composer_focused() => {
                self.start_visual_selection(SelectionMode::Block)
            }
            KeyCode::Char('v') if !ctrl && alt && !self.composer_focused() => {
                self.start_visual_selection(SelectionMode::Char)
            } // alt+v
            KeyCode::Char('V') if !ctrl && !alt && !self.composer_focused() => {
                self.start_visual_selection(SelectionMode::Line)
            }

            // Visual selection cursor movement
            KeyCode::Up if alt => self.jump_entry(-1),
            KeyCode::Down if alt => self.jump_entry(1),
            KeyCode::Up => {
                if let Some(sel) = &self.selection {
                    if sel.mode.is_some() {
                        self.update_visual_selection(0, -1, 0);
                    } else {
                        self.composer_or_scroll(key, -1);
                    }
                } else {
                    self.composer_or_scroll(key, -1);
                }
            }
            KeyCode::Down => {
                if let Some(sel) = &self.selection {
                    if sel.mode.is_some() {
                        self.update_visual_selection(0, 1, 0);
                    } else {
                        self.composer_or_scroll(key, 1);
                    }
                } else {
                    self.composer_or_scroll(key, 1);
                }
            }
            KeyCode::Left => {
                if let Some(sel) = &self.selection {
                    if sel.mode.is_some() {
                        self.update_visual_selection(0, 0, -1);
                    } else if ctrl || alt {
                        self.composer.move_word_left();
                    } else {
                        self.composer.move_left();
                    }
                } else if ctrl || alt {
                    self.composer.move_word_left();
                } else {
                    self.composer.move_left();
                }
            }
            KeyCode::Right => {
                if let Some(sel) = &self.selection {
                    if sel.mode.is_some() {
                        self.update_visual_selection(0, 0, 1);
                    } else if ctrl || alt {
                        self.composer.move_word_right();
                    } else {
                        self.composer.move_right();
                    }
                } else if ctrl || alt {
                    self.composer.move_word_right();
                } else {
                    self.composer.move_right();
                }
            }
            KeyCode::PageUp => {
                if let Some(sel) = &self.selection {
                    if sel.mode.is_some() {
                        self.update_visual_selection(0, -10, 0); // Page up = 10 lines
                    } else {
                        self.scroll_page(1);
                    }
                } else {
                    self.scroll_page(1);
                }
            }
            KeyCode::PageDown => {
                if let Some(sel) = &self.selection {
                    if sel.mode.is_some() {
                        self.update_visual_selection(0, 10, 0); // Page down = 10 lines
                    } else {
                        self.scroll_page(-1);
                    }
                } else {
                    self.scroll_page(-1);
                }
            }
            KeyCode::Home => {
                if let Some(sel) = &self.selection {
                    if sel.mode.is_some() {
                        // Move to start of line
                        if let Some(entry) = self.entries.get(sel.end.0) {
                            if let Some(cached) = &entry.cache {
                                if sel.end.1 < cached.lines.len() {
                                    self.selection.as_mut().unwrap().end.2 = 0;
                                    self.mark_dirty();
                                }
                            }
                        }
                    } else if ctrl {
                        self.view.scroll_to_top();
                    } else {
                        self.composer.move_home();
                    }
                } else if ctrl {
                    self.view.scroll_to_top();
                } else {
                    self.composer.move_home();
                }
            }
            KeyCode::End => {
                if let Some(sel) = &self.selection {
                    if sel.mode.is_some() {
                        // Move to end of line
                        if let Some(entry) = self.entries.get(sel.end.0) {
                            if let Some(cached) = &entry.cache {
                                if sel.end.1 < cached.lines.len() {
                                    let line_text: String = cached.lines[sel.end.1]
                                        .spans
                                        .iter()
                                        .map(|s| s.content.as_ref())
                                        .collect();
                                    self.selection.as_mut().unwrap().end.2 =
                                        line_text.chars().count();
                                    self.mark_dirty();
                                }
                            }
                        }
                    } else if ctrl {
                        self.scroll_to_end();
                    } else {
                        self.composer.move_end();
                    }
                } else if ctrl {
                    self.scroll_to_end();
                } else {
                    self.composer.move_end();
                }
            }
            KeyCode::Backspace => {
                if let Some(sel) = &self.selection {
                    if sel.mode.is_some() {
                        // Exit visual mode on backspace
                        self.exit_visual_selection();
                    } else {
                        self.composer.backspace();
                    }
                } else {
                    self.composer.backspace();
                }
            }
            KeyCode::Delete => {
                if let Some(sel) = &self.selection {
                    if sel.mode.is_some() {
                        // Exit visual mode on delete
                        self.exit_visual_selection();
                    } else {
                        self.composer.delete_forward();
                    }
                } else {
                    self.composer.delete_forward();
                }
            }
            KeyCode::Esc => {
                if self.is_busy() {
                    self.interrupt();
                } else if let Some(sel) = &self.selection {
                    if sel.mode.is_some() {
                        self.exit_visual_selection();
                    } else if !self.composer.is_empty() {
                        self.composer.clear();
                        self.mark_dirty();
                    }
                } else if !self.composer.is_empty() {
                    self.composer.clear();
                    self.mark_dirty();
                }
            }

            // Transcript actions: y = yank/copy (only in visual mode), > = quote into composer
            // In visual mode, 'y' copies selection. In composer, 'y' types normally.
            KeyCode::Char('>') => self.quote_last_assistant(),

            // Shift+Tab cycles the tool mode: plan -> coder -> build -> hacker.
            // Terminals send Shift+Tab as the xterm backtab sequence (CSI Z),
            // which crossterm reports as KeyCode::BackTab WITHOUT the shift
            // modifier - so BackTab is the binding that actually fires on real
            // keyboards. `Tab` + SHIFT is kept for terminals/protocols that do
            // report the modifier (e.g. the Kitty keyboard protocol). Plain Tab
            // stays reserved for ghost-text acceptance; overlays keep their own
            // Shift+Tab navigation.
            KeyCode::BackTab => {
                self.cycle_tool_policy();
                return;
            }
            KeyCode::Tab if shift => {
                self.cycle_tool_policy();
                return;
            }

            // Tab accepts inline completion (ghost text from last assistant code block)
            KeyCode::Tab => {
                if self.composer.accept_ghost() {
                    self.mark_dirty();
                    return;
                }
            }

            KeyCode::Char(c) => {
                if ctrl || alt {
                    return;
                }
                // Handle 'y' in transcript mode (visual selection copy)
                if c == 'y' && !self.composer_focused() {
                    if let Some(sel) = &self.selection {
                        if sel.mode.is_some() {
                            self.copy_visual_selection();
                        }
                        return;
                    }
                }
                self.composer.insert(c);
            }
            _ => return,
        }
        self.mark_dirty();
    }

    /// Vertical arrows drive the transcript unless the prompt spans several rows.
    fn composer_or_scroll(&mut self, key: crossterm::event::KeyEvent, delta: i32) {
        let width = self.composer_layout_width;
        let layout = self.composer.layout(width, self.theme.base());
        if layout.rows() > 1 {
            self.composer.move_row(&layout, delta);
            return;
        }
        let _ = key;
        // Same convention as the wheel: Up = toward older messages. `delta`
        // keeps its composer sense (Up = -1) for move_row above, so the
        // scroll fallback inverts it.
        self.scroll_by(-delta);
    }

    fn on_key_overlay(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        let Some(overlay) = self.overlay.clone() else {
            return;
        };
        match overlay {
            Overlay::Help(_) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                        self.close_overlay();
                        return;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.quit = true;
                        return;
                    }
                    KeyCode::Up => self.scroll_help(-1),
                    KeyCode::Down => self.scroll_help(1),
                    KeyCode::PageUp => self.scroll_help(-8),
                    KeyCode::PageDown => self.scroll_help(8),
                    KeyCode::Home => self.scroll_help(-(crate::ui::help_row_count() as i32)),
                    KeyCode::End => self.scroll_help(crate::ui::help_row_count() as i32),
                    _ => {}
                }
                self.mark_dirty();
            }
            Overlay::Hacker(mut state) => {
                let count = self.hacker.registry.len();
                match key.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                        self.close_overlay();
                        return;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.quit = true;
                        return;
                    }
                    KeyCode::Up => state.cursor = state.cursor.saturating_sub(1),
                    KeyCode::Down if count > 0 => {
                        state.cursor = (state.cursor + 1).min(count - 1);
                    }
                    _ => {}
                }
                self.overlay = Some(Overlay::Hacker(state));
                self.mark_dirty();
            }
            Overlay::Connect(mut form) => {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                let alt = key.modifiers.contains(KeyModifiers::ALT);
                match key.code {
                    KeyCode::Esc => {
                        self.close_overlay();
                        return;
                    }
                    KeyCode::Enter => {
                        self.overlay = Some(Overlay::Connect(form));
                        self.apply_connect();
                        return;
                    }
                    KeyCode::Up => form.focus_by(-1),
                    KeyCode::Down | KeyCode::Tab => form.focus_by(if ctrl { -1 } else { 1 }),
                    KeyCode::Left => form.field_mut(form.focus).move_left(),
                    KeyCode::Right => form.field_mut(form.focus).move_right(),
                    KeyCode::Home => form.field_mut(form.focus).move_home(),
                    KeyCode::End => form.field_mut(form.focus).move_end(),
                    KeyCode::Backspace => form.field_mut(form.focus).backspace(),
                    KeyCode::Delete => form.field_mut(form.focus).delete_forward(),
                    KeyCode::Char('u') if ctrl => form.field_mut(form.focus).delete_to_start(),
                    KeyCode::Char('k') if ctrl => form.field_mut(form.focus).delete_to_end(),
                    KeyCode::Char(c) if !ctrl && !alt => form.field_mut(form.focus).insert(c),
                    _ => {}
                }
                self.overlay = Some(Overlay::Connect(form));
                self.mark_dirty();
            }
            Overlay::Picker(mut picker) => {
                match key.code {
                    KeyCode::Esc => {
                        self.close_overlay();
                        return;
                    }
                    KeyCode::Enter => {
                        if let Some(value) = picker.selection() {
                            self.overlay = None;
                            self.apply_picker_value(value);
                            return;
                        }
                        self.toast("nothing selected");
                    }
                    KeyCode::Up => picker.move_by(-1),
                    KeyCode::Down => picker.move_by(1),
                    KeyCode::Tab => {
                        picker.move_by(if key.modifiers.contains(KeyModifiers::SHIFT) {
                            -1
                        } else {
                            1
                        })
                    }
                    KeyCode::Backspace => picker.filter.backspace(),
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        picker.filter.insert(c)
                    }
                    _ => {}
                }
                picker.apply_filter();
                self.overlay = Some(Overlay::Picker(picker));
                self.mark_dirty();
            }
            Overlay::Tools(mut state) => {
                let count = crate::tools::ToolSet::catalog().len();
                match key.code {
                    KeyCode::Esc => {
                        self.close_overlay();
                        return;
                    }
                    KeyCode::Enter => {
                        self.overlay = Some(Overlay::Tools(state));
                        self.toggle_tool();
                        return;
                    }
                    KeyCode::Up => state.cursor = state.cursor.saturating_sub(1),
                    KeyCode::Down => {
                        if count > 0 && state.cursor + 1 < count {
                            state.cursor += 1;
                        }
                    }
                    KeyCode::PageUp => state.cursor = state.cursor.saturating_sub(8),
                    KeyCode::PageDown => {
                        state.cursor = (state.cursor + 8).min(count.saturating_sub(1))
                    }
                    KeyCode::Home => state.cursor = 0,
                    KeyCode::End => state.cursor = count.saturating_sub(1),
                    _ => {}
                }
                self.overlay = Some(Overlay::Tools(state));
                self.mark_dirty();
            }
            Overlay::Board(mut state) => {
                match key.code {
                    KeyCode::Esc => {
                        self.cancel_board();
                        return;
                    }
                    // Scrolling is view-only: Up moves toward older messages.
                    KeyCode::Up => state.offset = state.offset.saturating_add(1),
                    KeyCode::Down => state.offset = state.offset.saturating_sub(1),
                    KeyCode::PageUp => state.offset = state.offset.saturating_add(8),
                    KeyCode::PageDown => state.offset = state.offset.saturating_sub(8),
                    KeyCode::Home => state.offset = usize::MAX / 2,
                    KeyCode::End => state.offset = 0,
                    _ => {}
                }
                self.overlay = Some(Overlay::Board(state));
                self.mark_dirty();
            }
            Overlay::Memory(mut view) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                        self.close_overlay();
                        return;
                    }
                    KeyCode::Up => view.offset = view.offset.saturating_add(1),
                    KeyCode::Down => view.offset = view.offset.saturating_sub(1),
                    KeyCode::PageUp => view.offset = view.offset.saturating_add(8),
                    KeyCode::PageDown => view.offset = view.offset.saturating_sub(8),
                    KeyCode::Home => view.offset = usize::MAX / 2,
                    KeyCode::End => view.offset = 0,
                    _ => {}
                }
                self.overlay = Some(Overlay::Memory(view));
                self.mark_dirty();
            }
            Overlay::Improve(mut state) => {
                let applying = state.applying.is_some();
                let has_review = state.review.is_some();
                match key.code {
                    // `esc` walks back out: review → list → closed.
                    KeyCode::Esc => {
                        drop(state);
                        if has_review {
                            if let Some(Overlay::Improve(state)) = self.overlay.as_mut() {
                                state.review = None;
                                state.offset = 0;
                            }
                        } else {
                            self.close_overlay();
                        }
                        self.mark_dirty();
                        return;
                    }
                    KeyCode::Enter if !has_review => {
                        self.open_improve_review();
                        return;
                    }
                    // Approval gate: `a` and `r` exist only in the review view,
                    // and only `a` can change a file on disk.
                    KeyCode::Char('a') if has_review && !applying => {
                        let id = state.review.as_ref().map(|r| r.id.clone());
                        drop(state);
                        if let Some(id) = id {
                            self.approve_improve(&id);
                        }
                        return;
                    }
                    KeyCode::Char('r') if has_review && !applying => {
                        let id = state.review.as_ref().map(|r| r.id.clone());
                        drop(state);
                        if let Some(id) = id {
                            self.reject_improve(&id);
                        }
                        return;
                    }
                    KeyCode::Up => {
                        if has_review {
                            state.offset = state.offset.saturating_add(1);
                        } else {
                            state.cursor = state.cursor.saturating_sub(1);
                        }
                    }
                    KeyCode::Down => {
                        if has_review {
                            state.offset = state.offset.saturating_sub(1);
                        } else if state.cursor + 1 < state.rows.len() {
                            state.cursor += 1;
                        }
                    }
                    KeyCode::PageUp => {
                        if has_review {
                            state.offset = state.offset.saturating_add(8);
                        } else {
                            state.cursor = state.cursor.saturating_sub(8);
                        }
                    }
                    KeyCode::PageDown => {
                        if has_review {
                            state.offset = state.offset.saturating_sub(8);
                        } else {
                            state.cursor =
                                (state.cursor + 8).min(state.rows.len().saturating_sub(1));
                        }
                    }
                    KeyCode::Home => {
                        if has_review {
                            state.offset = usize::MAX / 2;
                        } else {
                            state.cursor = 0;
                        }
                    }
                    KeyCode::End => {
                        if has_review {
                            state.offset = 0;
                        } else {
                            state.cursor = state.rows.len().saturating_sub(1);
                        }
                    }
                    _ => {}
                }
                self.overlay = Some(Overlay::Improve(state));
                self.mark_dirty();
            }
            Overlay::Providers(mut state) => {
                match key.code {
                    KeyCode::Esc => {
                        self.close_overlay();
                        return;
                    }
                    KeyCode::Enter => {
                        let query = state.filter.value().to_string();
                        let rows = self.provider_menu_rows(&query);
                        let Some(row) = rows.get(state.cursor) else {
                            self.toast("no matching provider");
                            self.overlay = Some(Overlay::Providers(state));
                            self.mark_dirty();
                            return;
                        };
                        let id = row.id.clone();
                        let label = row.label.clone();
                        drop(state);
                        self.overlay = None;
                        self.apply_provider(&id, &label);
                        return;
                    }
                    KeyCode::Up => {
                        state.cursor = state.cursor.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        let len = self.provider_menu_rows(state.filter.value()).len();
                        if len > 0 && state.cursor + 1 < len {
                            state.cursor += 1;
                        }
                    }
                    KeyCode::PageUp => {
                        state.cursor = state.cursor.saturating_sub(8);
                    }
                    KeyCode::PageDown => {
                        let len = self.provider_menu_rows(state.filter.value()).len();
                        state.cursor = (state.cursor + 8).min(len.saturating_sub(1));
                    }
                    KeyCode::Backspace => state.filter.backspace(),
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        state.filter.insert(c)
                    }
                    _ => {}
                }
                // Keep the cursor inside the filtered list.
                let len = self.provider_menu_rows(state.filter.value()).len();
                state.cursor = state.cursor.min(len.saturating_sub(1));
                self.overlay = Some(Overlay::Providers(state));
                self.mark_dirty();
            }
            Overlay::SudoPassword(mut state) => {
                match key.code {
                    KeyCode::Esc => {
                        self.close_overlay();
                        return;
                    }
                    KeyCode::Enter => {
                        if state.password.trim().is_empty() {
                            state.error = Some("Password cannot be empty".to_string());
                        } else {
                            state.submitting = true;
                            state.error = None;
                            let command = state.command.clone();
                            let cwd = state.cwd.clone();
                            let timeout_ms = state.timeout_ms;
                            let password = state.password.clone();
                            drop(state);
                            self.overlay = None;
                            self.execute_sudo_command(command, cwd, timeout_ms, password);
                            return;
                        }
                    }
                    KeyCode::Backspace => {
                        state.password.pop();
                        state.error = None;
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        state.password.push(c);
                        state.error = None;
                    }
                    _ => {}
                }
                self.overlay = Some(Overlay::SudoPassword(state));
                self.mark_dirty();
            }
        }
    }
}

/// Text selection mode for visual selection (like Vim's v/V)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Character-wise selection (v)
    Char,
    /// Line-wise selection (V)
    Line,
    /// Block-wise selection (Ctrl-v) - not yet implemented
    Block,
}

/// Text selection state for chat area
#[derive(Debug, Clone)]
pub struct Selection {
    /// Start position (entry index, row within entry, column)
    pub start: (usize, usize, usize),
    /// End position (entry index, row within entry, column)
    pub end: (usize, usize, usize),
    /// Whether we're currently dragging (mouse)
    pub dragging: bool,
    /// Visual selection mode (keyboard-driven)
    pub mode: Option<SelectionMode>,
}

impl App {
    /// Handle mouse events: scroll wheel, scrollbar track, text selection
    pub fn on_mouse(&mut self, event: crossterm::event::MouseEvent) {
        use crossterm::event::MouseEventKind;

        // Handle scroll wheel first
        match event.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_by(3);
                return;
            }
            MouseEventKind::ScrollDown => {
                self.scroll_by(-3);
                return;
            }
            MouseEventKind::Up(_) => self.scrollbar_drag = false,
            _ => {}
        }

        // The scrollbar track: click or drag jumps the viewport, and a press
        // on the bottom edge re-follows the tail (as the footer promises). A
        // press here must never start a text selection.
        if let Some(chat_rect) = self.view.cached_chat_rect() {
            let sb_col = chat_rect.x + chat_rect.width.saturating_sub(1);
            let in_track = event.column == sb_col
                && event.row >= chat_rect.y
                && event.row < chat_rect.y + chat_rect.height;
            if in_track {
                let scrollable = self.view.content_height > chat_rect.height;
                match event.kind {
                    MouseEventKind::Down(_) if scrollable => {
                        self.scrollbar_drag = true;
                        self.scrollbar_jump(event.row, chat_rect);
                        return;
                    }
                    MouseEventKind::Drag(_) if self.scrollbar_drag => {
                        self.scrollbar_jump(event.row, chat_rect);
                        return;
                    }
                    _ => return,
                }
            }
        }

        // Handle text selection in the chat area
        self.handle_chat_mouse(event);
    }

    /// Jumps the viewport so the scrollbar thumb sits at `row` of its track.
    fn scrollbar_jump(&mut self, row: u16, chat_rect: ratatui::layout::Rect) {
        let viewport = chat_rect.height;
        let content = self.view.content_height;
        if content <= viewport || viewport < 2 {
            return;
        }
        let max = content - viewport;
        let track_row = row.saturating_sub(chat_rect.y);
        let rel = track_row as f32 / (viewport - 1) as f32;
        let top = ((rel * max as f32).round() as u16).min(max);
        self.view.scroll_from_bottom = max - top;
        self.view.follow = self.view.scroll_from_bottom == 0;
        if self.view.follow {
            self.view.new_since_scroll = 0;
            self.view.new_baseline = content;
        }
        self.mark_dirty();
    }

    /// Handle mouse events in the chat area for text selection
    fn handle_chat_mouse(&mut self, event: crossterm::event::MouseEvent) {
        use crossterm::event::{MouseButton, MouseEventKind};

        // Focus mode renders a slice of the transcript; selection row math
        // assumes the full list, so it waits until focus is off.
        if self.focus {
            return;
        }

        let Some(chat_rect) = self.view.cached_chat_rect() else {
            return;
        };

        // Convert mouse position to chat-relative coordinates
        let col = event.column.saturating_sub(chat_rect.x) as usize;
        let row = event.row.saturating_sub(chat_rect.y) as usize;

        // Width of chat content area (accounting for gutter and scrollbar)
        let content_width = chat_rect.width.saturating_sub(1) as usize;
        let inner_width = content_width.saturating_sub(chat::GUTTER).max(8);

        // Find which entry and position the mouse is over. Blank separator
        // rows have no text position: `None` means the selection anchors
        // lazily, at the first text row the pointer actually reaches.
        let mapped = self.mouse_to_text_position(row, col, inner_width);
        const UNANCHORED: (usize, usize, usize) = (usize::MAX, 0, 0);

        match event.kind {
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Down(MouseButton::Right) => {
                // Start a selection (like opencode). The highlight must begin
                // exactly where the pointer meets text — never above it — so
                // a press on a blank row stays unanchored for now.
                let anchor = mapped.unwrap_or(UNANCHORED);
                self.selection = Some(Selection {
                    start: anchor,
                    end: anchor,
                    dragging: true,
                    mode: None,
                });
                self.mark_dirty();
            }
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Right) => {
                // Update selection end for both left and right drag
                let Some(pos) = mapped else {
                    return;
                };
                if let Some(sel) = &mut self.selection {
                    if sel.dragging {
                        sel.end = pos;
                        // Lazy anchor: a drag that began on a blank row
                        // starts where it first met text.
                        if sel.start.0 == usize::MAX {
                            sel.start = pos;
                        }
                        self.mark_dirty();
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) | MouseEventKind::Up(MouseButton::Right) => {
                // End selection and copy to clipboard
                if let Some(sel) = self.selection.take() {
                    if sel.dragging {
                        // A press-drag that never reached a text row selects
                        // nothing: no copy, no error toggle.
                        if sel.start.0 == usize::MAX || sel.end.0 == usize::MAX {
                            self.mark_dirty();
                            return;
                        }
                        // A plain click (no drag) on a link opens it in the
                        // system browser — before the copy/error fallbacks.
                        if sel.start == sel.end {
                            if let Some(url) = self.link_at(sel.start.0, sel.start.1, sel.start.2) {
                                self.control_send(crate::app::Control::OpenLink(url.clone()));
                                self.toast(format!(
                                    "opening {}",
                                    crate::ui::text::truncate_head(&url, 48)
                                ));
                                self.mark_dirty();
                                return;
                            }
                        }
                        let text = self.get_selected_text(&sel);
                        if !text.trim().is_empty() {
                            self.control_send(crate::app::Control::Copy(text));
                            self.toast("selection copied to clipboard");
                        } else if let Some(source) = self.code_block_at(sel.start.0, sel.start.1) {
                            // A plain click inside a fenced block copies its
                            // raw source, opencode-style.
                            let count = source.lines().count();
                            self.control_send(crate::app::Control::Copy(source));
                            self.toast(format!(
                                "code block copied ({})",
                                crate::ui::text::plural(count, "line")
                            ));
                        } else {
                            // Plain click elsewhere on a failed entry opens
                            // its full error.
                            self.toggle_error_expansion(sel.start.0);
                        }
                    }
                    self.mark_dirty();
                }
            }
            _ => {}
        }
    }

    /// Convert mouse row/col to entry index, row within entry, and column.
    /// The column is a display column mapped onto the row's char indices:
    /// painted rows start at the chat area's left edge (prefixes are baked
    /// into the spans), so the pointer's cell walks the spans directly.
    fn mouse_to_text_position(
        &self,
        mouse_row: usize,
        mouse_col: usize,
        _inner_width: usize,
    ) -> Option<(usize, usize, usize)> {
        if self.entries.is_empty() || mouse_row >= self.view.content_height as usize {
            return None;
        }

        let (top, _) = self.view.window();
        let virtual_row = top as usize + mouse_row;

        // Walk through entries to find which one contains this virtual row
        let mut current_row = 0usize;
        for (idx, entry) in self.entries.iter().enumerate() {
            let entry_h = entry.slot(self.compact) as usize;
            if virtual_row < current_row + entry_h {
                // Found the entry
                let local_row = virtual_row - current_row;
                let status_rows = entry.status_rows() as usize;

                if local_row < status_rows {
                    // In status line - skip
                    return None;
                }

                let body_row = local_row - status_rows;
                let cached = entry.cache.as_ref()?;
                if body_row >= cached.lines.len() {
                    // A blank separator row owns no text: the selection stays
                    // unanchored until the pointer reaches a real line.
                    return None;
                }

                // Display column -> char index: wide glyphs occupy two cells,
                // so a raw char count would drift the highlight out of place.
                let col = display_col_to_char(&cached.lines[body_row], mouse_col);

                return Some((idx, body_row, col));
            }
            current_row += entry_h;
        }

        None
    }

    /// Extract selected text from the transcript
    pub(crate) fn get_selected_text(&self, sel: &Selection) -> String {
        let (start_entry, start_row, start_col) = sel.start;
        let (end_entry, end_row, end_col) = sel.end;
        // An unanchored selection (press on a blank row) owns no text.
        if start_entry == usize::MAX || end_entry == usize::MAX {
            return String::new();
        }

        // Ensure start is before end
        let (start_entry, start_row, start_col, end_entry, end_row, end_col) =
            if (start_entry, start_row, start_col) <= (end_entry, end_row, end_col) {
                (
                    start_entry,
                    start_row,
                    start_col,
                    end_entry,
                    end_row,
                    end_col,
                )
            } else {
                (
                    end_entry,
                    end_row,
                    end_col,
                    start_entry,
                    start_row,
                    start_col,
                )
            };

        let mut result = String::new();

        for entry_idx in start_entry..=end_entry.min(self.entries.len().saturating_sub(1)) {
            let entry = &self.entries[entry_idx];
            let cached = match &entry.cache {
                Some(c) => c,
                None => continue,
            };

            let row_start = if entry_idx == start_entry {
                start_row
            } else {
                0
            };
            let row_end = if entry_idx == end_entry {
                end_row.min(cached.lines.len().saturating_sub(1))
            } else {
                cached.lines.len().saturating_sub(1)
            };

            for row_idx in row_start..=row_end {
                if row_idx >= cached.lines.len() {
                    continue;
                }
                let line = &cached.lines[row_idx];
                let line_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

                let col_start = if entry_idx == start_entry && row_idx == start_row {
                    start_col
                } else {
                    0
                };
                let col_end = if entry_idx == end_entry && row_idx == end_row {
                    end_col.min(line_text.chars().count())
                } else {
                    line_text.chars().count()
                };

                if col_start < line_text.chars().count() {
                    let chars: Vec<char> = line_text.chars().collect();
                    let selected: String =
                        chars[col_start..col_end.min(chars.len())].iter().collect();
                    result.push_str(&selected);
                    if row_idx < row_end || entry_idx < end_entry {
                        result.push('\n');
                    }
                }
            }
        }

        result
    }
}

fn editor_available() -> bool {
    std::env::var("EDITOR")
        .ok()
        .is_some_and(|e| !e.trim().is_empty())
}

/// Where the board database lives: `NANOMITE_BOARD_DIR` overrides the
/// directory (tests), else the platform data dir like `session.rs` uses.
/// Pending self-edit proposals as the `/improve` list shows them. Blocking: the
/// engine reads its proposal store from disk, so this only ever runs on a
/// blocking worker.
fn improve_rows_from(
    engine: &ImproveEngine,
    dir: &std::path::Path,
) -> Result<Vec<ImproveRow>, String> {
    let proposals = engine
        .list(Some(Status::Pending))
        .map_err(|e| crate::ui::text::truncate(&e.to_string(), 120))?;
    Ok(proposals
        .into_iter()
        .take(IMPROVE_LIST_ROWS)
        .map(|p| {
            let diff =
                std::fs::read_to_string(dir.join("proposals").join(format!("{}.diff", p.id)))
                    .unwrap_or_default();
            let (added, removed) = crate::tools::improve_tools::count_changes(&diff);
            ImproveRow {
                id: p.id.clone(),
                target: p.target.as_str().to_string(),
                path: p.path.to_string_lossy().to_string(),
                rationale: crate::tools::improve_tools::summary(&p).rationale,
                created_at: p.created_at,
                added,
                removed,
            }
        })
        .collect())
}

/// Maps an apply outcome onto the overlay's status line: applied is success, a
/// failed regression gate is a warning carrying the runner's tail, and a hard
/// failure is a danger line.
fn status_from_outcome(outcome: &ApplyOutcome) -> ImproveStatus {
    match outcome.status {
        Status::Applied => ImproveStatus {
            level: ImproveLevel::Success,
            message: format!("{} applied", outcome.id),
        },
        Status::RolledBack => ImproveStatus {
            level: ImproveLevel::Warn,
            message: format!(
                "{} rolled back · {REGRESSION_COMMAND} failed\n{}",
                outcome.id,
                detail_tail(&outcome.detail)
            ),
        },
        _ => ImproveStatus {
            level: ImproveLevel::Danger,
            message: format!("{} failed\n{}", outcome.id, detail_tail(&outcome.detail)),
        },
    }
}

/// Last [`IMPROVE_DETAIL_LINES`] lines of a runner failure tail.
fn detail_tail(detail: &str) -> String {
    let lines: Vec<&str> = detail.trim_end().lines().collect();
    let start = lines.len().saturating_sub(IMPROVE_DETAIL_LINES);
    lines[start..].join("\n")
}

fn board_store_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("NANOMITE_BOARD_DIR") {
        return Some(PathBuf::from(dir).join("board.db"));
    }
    let base = dirs::data_dir().or_else(dirs::home_dir)?;
    Some(base.join("nanomite").join("board").join("board.db"))
}

/// One-line palette description shown beside each theme in `/themes`.
fn theme_descriptor(id: ThemeId) -> &'static str {
    match id {
        ThemeId::Mono => "single hue · high contrast",
        ThemeId::Midnight => "cool slate · periwinkle accent",
        ThemeId::Graphite => "teal · orange on deep charcoal",
        ThemeId::Umbrella => "blood red · clinical white",
    }
}

/// Extracts up to three numbered subtask lines from the dispatcher's reply.
/// Anything the model phrases differently falls back to the caller's
/// defaults, so a chatty decomposition never breaks the team.
pub(crate) fn parse_subtasks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        // Tolerate a bullet before the number: "- 3. Wire the tests".
        let trimmed = line
            .trim()
            .trim_start_matches(['-', '*', '\u{2022}'])
            .trim();
        let mut chars = trimmed.chars();
        if !chars.next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        let body = trimmed
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')' || c == '-')
            .trim();
        if body.chars().count() >= 8 {
            out.push(body.chars().take(200).collect());
        }
        if out.len() == 3 {
            break;
        }
    }
    out
}

/// Parses `/revert 3`-style turn arguments; `None` means "latest".
fn parse_turn_arg(args: &str) -> Option<usize> {
    args.trim().parse::<usize>().ok()
}

/// Maps a display column (terminal cells from the row's left edge) onto a
/// char index inside the row, walking the spans through the shared width
/// rules (`text::WidthState`) so a click lands on the exact glyph under the
/// pointer: wide (CJK) glyphs occupy two cells and a click on either half
/// snaps to them, and emoji clusters (⚠️, 👨‍👩‍👧, 🇵🇭) anchor at their base
/// character. A click past the end clamps to the row's char count.
fn display_col_to_char(line: &Line<'_>, mouse_col: usize) -> usize {
    let mut state = crate::ui::text::WidthState::default();
    let mut cells = 0usize;
    let mut chars = 0usize;
    let mut cluster_start = 0usize;
    for span in &line.spans {
        for ch in span.content.chars() {
            let step = state.feed(ch);
            if !step.continues {
                cluster_start = chars;
            }
            if cells + step.cells > mouse_col {
                return cluster_start;
            }
            cells += step.cells;
            chars += 1;
        }
    }
    chars
}

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

/// One search hit as a `/memory` row: summary when the record has one, else
/// its first non-empty line, with the body carried for the record overlay.
fn memory_row(hit: crate::memory::model::MemoryHit) -> MemoryRow {
    let record = hit.record;
    let title = record
        .summary
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            record
                .content
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_string())
        })
        .unwrap_or_else(|| record.id.clone());
    let content: String = record.content.chars().take(MEMORY_BODY_CHARS).collect();
    MemoryRow {
        id: record.id,
        title,
        category: record.category.as_str().to_string(),
        tags: record.tags.clone(),
        created_at: record.created_at,
        project: record.project.clone(),
        content,
    }
}

fn memory_item(row: MemoryRow) -> PickerItem {
    PickerItem {
        label: row.title.clone(),
        hint: row.hint(),
        value: PickerValue::Memory(row),
        current: false,
    }
}

/// Standard base64 (RFC 4648) for the OSC 52 clipboard payload.
///
/// Small and local on purpose: the only consumer is `/copy`, and a general
/// dependency would not earn its place in this binary.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The OSC 52 clipboard sequence for `text`.
///
/// `ESC ] 52 ; c ; <base64> BEL`. Terminals that support it put the payload on
/// the system clipboard; the ones that do not ignore the sequence, so this is
/// safe to emit unconditionally.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64_encode(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_config() -> Config {
        let mut cfg = Config::from_env();
        cfg.apply_preset("scripted");
        cfg.notice = None;
        // Confirmations are interactive: the offline test app must never
        // wait on the user, so the gate stays off unless a test opts in.
        cfg.confirm_external = false;
        cfg
    }

    async fn test_app() -> App {
        App::new(test_config(), tokio::runtime::Handle::current()).expect("app")
    }

    /// A test app with HACKER MODE enabled and an authorized scope.
    async fn hacker_app() -> App {
        let mut cfg = test_config();
        cfg.hacker.enabled = true;
        cfg.hacker.scope = vec!["10.0.0.0/8".into(), "example.com".into()];
        cfg.hacker.audit = false; // never write the real audit dir in tests
        cfg.hacker.store = false; // ... nor the real findings database
        App::new(cfg, tokio::runtime::Handle::current()).expect("app")
    }

    #[tokio::test]
    async fn hacker_mode_disabled_refuses_to_open() {
        let mut app = test_app().await;
        app.run_command(Command::Hacker, "");
        assert!(app.overlay.is_none(), "disabled mode must not open");
        assert!(!app.hacker.active);
        assert!(app.toast.is_some(), "a refusal hint is shown");
    }

    #[tokio::test]
    async fn hacker_mode_activates_and_gates_auto_approve() {
        let mut app = hacker_app().await;
        app.run_command(Command::Hacker, "");
        assert!(app.hacker.active, "enabled mode activates");
        assert!(matches!(app.overlay, Some(Overlay::Hacker(_))));
        assert!(app.hacker.auto_approve_active(SystemTime::now()));
        assert_eq!(
            app.effective_tool_policy(),
            crate::agent::policy::ToolPolicy::Hacker,
            "permissions widen while the assessment is active"
        );

        app.run_command(Command::Hacker, "exit");
        assert!(!app.hacker.active);
        assert!(app.overlay.is_none());
        assert!(!app.hacker.auto_approve_active(SystemTime::now()));
        assert_eq!(
            app.effective_tool_policy(),
            app.cfg.tool_policy,
            "permissions fall back on exit"
        );
    }

    #[tokio::test]
    async fn hacker_status_reports_without_opening() {
        let mut app = hacker_app().await;
        app.run_command(Command::Hacker, "status");
        assert!(app.overlay.is_none());
        assert!(app.toast.as_ref().unwrap().0.contains("hacker mode"));
    }

    #[tokio::test]
    async fn hacker_help_lists_executable_operations() {
        let mut app = hacker_app().await;
        app.run_command(Command::Hacker, "help");
        let text = app
            .entries
            .iter()
            .rev()
            .find(|e| matches!(e.role, EntryRole::System))
            .map(|e| e.text.clone())
            .unwrap_or_default();
        assert!(text.contains("recon/dns"), "{text}");
        assert!(text.contains("exploitation/plan"), "{text}");
        assert!(text.contains("vuln/correlate"), "{text}");
        assert!(text.contains("kali/run"), "{text}");
        assert!(text.contains("kali tools:"), "{text}");
    }

    #[tokio::test]
    async fn hacker_run_parses_and_activates_then_reports_background_start() {
        let mut app = hacker_app().await;
        // Out-of-scope: the coordinator denies before any I/O, so this never
        // touches the network.
        app.run_command(Command::Hacker, "run network ports 8.8.8.8");
        assert!(app.hacker.active, "run activates the environment");
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|(t, _)| t.contains("network/ports 8.8.8.8")));
    }

    #[tokio::test]
    async fn hacker_unknown_operation_is_refused_locally() {
        let mut app = hacker_app().await;
        app.run_command(Command::Hacker, "run nope nope x");
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|(t, _)| t.contains("unknown hacker operation")));
    }

    #[tokio::test]
    async fn hacker_kali_lists_and_requires_a_target() {
        let mut app = hacker_app().await;
        // `list` routes to the passive availability op (no target, no exec).
        app.run_command(Command::Hacker, "kali list");
        assert!(app.hacker.active);
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|(t, _)| t.contains("kali/available")));

        // A tool name without a target is a usage error, not a run.
        let before = app.entries.len();
        app.run_command(Command::Hacker, "kali nmap");
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|(t, _)| t.contains("usage: /hacker kali nmap")));
        assert_eq!(app.entries.len(), before, "a usage error starts nothing");
    }

    #[test]
    fn hacker_findings_command_parses_every_form() {
        use crate::hacker::FindingStatus;
        assert_eq!(parse_findings_cmd(""), Ok(FindingsCmd::List));
        assert_eq!(parse_findings_cmd("list"), Ok(FindingsCmd::List));
        assert_eq!(parse_findings_cmd("show 7"), Ok(FindingsCmd::Show(7)));
        assert_eq!(
            parse_findings_cmd("triage 3"),
            Ok(FindingsCmd::SetStatus(3, FindingStatus::Triaged))
        );
        assert_eq!(
            parse_findings_cmd("accept 4"),
            Ok(FindingsCmd::SetStatus(4, FindingStatus::Accepted))
        );
        assert_eq!(
            parse_findings_cmd("close 5"),
            Ok(FindingsCmd::SetStatus(5, FindingStatus::Closed))
        );
        assert_eq!(parse_findings_cmd("verify 9"), Ok(FindingsCmd::Verify(9)));
        assert!(parse_findings_cmd("show nope").is_err());
        assert!(parse_findings_cmd("verify").is_err());
        assert!(parse_findings_cmd("triage").is_err());
        assert!(parse_findings_cmd("bogus 1").is_err());
        assert!(parse_findings_cmd("list extra").is_err());
        // Report formats round-trip through the store's parser.
        assert!(crate::hacker::store::ReportFormat::parse("sarif").is_some());
    }

    #[test]
    fn hacker_assessment_command_parses_every_form() {
        assert_eq!(parse_assessment_cmd(""), Ok(AssessmentCmd::List));
        assert_eq!(parse_assessment_cmd("list"), Ok(AssessmentCmd::List));
        assert_eq!(
            parse_assessment_cmd("use asm-1"),
            Ok(AssessmentCmd::Use("asm-1".into()))
        );
        assert_eq!(
            parse_assessment_cmd("complete"),
            Ok(AssessmentCmd::Complete(None))
        );
        assert_eq!(
            parse_assessment_cmd("complete asm-2"),
            Ok(AssessmentCmd::Complete(Some("asm-2".into())))
        );
        assert!(parse_assessment_cmd("use").is_err());
        assert!(parse_assessment_cmd("bogus").is_err());
        assert!(parse_assessment_cmd("list extra").is_err());
    }

    #[tokio::test]
    async fn hacker_assessment_respects_the_store_switch() {
        // hacker_app() disables the store, so the command must refuse cleanly.
        let mut app = hacker_app().await;
        for args in ["list", "use asm-1", "complete"] {
            app.run_command(Command::Hacker, &format!("assessment {args}"));
            assert!(
                app.toast
                    .as_ref()
                    .is_some_and(|(t, _)| t.contains("findings store is disabled")),
                "`assessment {args}` must refuse without a store"
            );
        }
    }

    #[tokio::test]
    async fn hacker_vuln_requires_component_and_version() {
        let mut app = hacker_app().await;
        app.run_command(Command::Hacker, "vuln openssl");
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|(t, _)| t.contains("usage: /hacker vuln")));
        assert!(!app.hacker.active, "a usage error must not activate");
    }

    #[tokio::test]
    async fn apply_hacker_run_appends_rendered_report_or_error() {
        let mut app = hacker_app().await;
        let report = crate::hacker::modules::ModuleReport::new(
            "recon",
            "dns",
            "example.com",
            "1 address(es)",
        )
        .line("10.0.0.1");
        app.apply_hacker_event(HackerEvent::Report {
            request: "recon/dns example.com".into(),
            result: Ok(report),
        });
        let last = app.entries.last().expect("entry");
        assert!(last.text.contains("[hacker:recon] dns"));
        assert!(last.text.contains("10.0.0.1"));

        app.apply_hacker_event(HackerEvent::Report {
            request: "network/ports x".into(),
            result: Err("hacker_target_out_of_scope: nope".into()),
        });
        assert!(app
            .entries
            .last()
            .unwrap()
            .text
            .contains("hacker_target_out_of_scope"));

        app.apply_hacker_event(HackerEvent::Lines {
            request: "findings".into(),
            text: "3 finding(s):".into(),
        });
        assert!(app
            .entries
            .last()
            .unwrap()
            .text
            .contains("[hacker:findings]"));
    }

    #[tokio::test]
    async fn hacker_findings_and_report_respect_the_store_switch() {
        // hacker_app() disables the store, so both must refuse with a hint.
        let mut app = hacker_app().await;
        app.run_command(Command::Hacker, "findings");
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|(t, _)| t.contains("findings store is disabled")));
        app.run_command(Command::Hacker, "report");
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|(t, _)| t.contains("findings store is disabled")));
    }

    #[tokio::test]
    async fn hacker_scope_reaches_the_assessment() {
        let mut app = hacker_app().await;
        app.run_command(Command::Hacker, "");
        let ctx = app.hacker.assessment.as_ref().expect("assessment");
        assert_eq!(ctx.allowed_cidrs, vec!["10.0.0.0/8".to_string()]);
        assert_eq!(ctx.allowed_domains, vec!["example.com".to_string()]);
        // Out-of-scope work is refused by the independent gate even while active.
        let op = crate::hacker::Operation::new(
            "recon",
            crate::hacker::OperationCategory::ActiveDiscovery,
        )
        .target("8.8.8.8");
        let decision = crate::hacker::authorize(Some(ctx), &op, SystemTime::now());
        assert!(!decision.is_allowed(), "8.8.8.8 is out of scope");
    }

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn osc52_wraps_the_payload_in_the_terminal_sequence() {
        assert_eq!(osc52("foo"), "\x1b]52;c;Zm9v\x07");
    }

    #[test]
    fn image_attachment_rejects_unknown_extensions_and_missing_files() {
        assert!(build_image_attachment("", std::path::Path::new("/tmp"))
            .unwrap_err()
            .contains("usage"));
        assert!(
            build_image_attachment("notes.txt", std::path::Path::new("/tmp"))
                .unwrap_err()
                .contains("png, jpg, jpeg, gif or webp")
        );
        assert!(build_image_attachment(
            "/nonexistent/definitely/missing.png",
            std::path::Path::new("/tmp")
        )
        .unwrap_err()
        .contains("cannot read"));
    }

    #[test]
    fn image_attachment_encodes_a_data_url() {
        let dir = crate::improve::testing::temp_dir("image-encode");
        let path = dir.path.join("shot.png");
        std::fs::write(&path, b"\x89PNG-bytes").expect("write fixture");
        let image =
            build_image_attachment(path.to_str().unwrap(), &dir.path).expect("attachment builds");
        assert_eq!(image.name, "shot.png");
        assert_eq!(image.bytes, 10);
        assert!(
            image.data_url.starts_with("data:image/png;base64,"),
            "data url: {}",
            image.data_url
        );
        // RFC 4648 of the exact payload.
        assert!(image.data_url.ends_with(&base64_encode(b"\x89PNG-bytes")));
    }

    #[test]
    fn image_size_label_and_badge_read_cleanly() {
        assert_eq!(image_size_label(512), "1 KB");
        assert_eq!(image_size_label(2048), "2 KB");
        assert_eq!(image_size_label(3 * 1024 * 1024 + 262_144), "3.2 MB");
        let image = PendingImage {
            name: "shot.png".into(),
            data_url: "data:image/png;base64,QQ==".into(),
            bytes: 2048,
        };
        assert_eq!(image_badge(&image), "[image: shot.png · 2 KB]");
    }

    #[tokio::test]
    async fn paste_routes_dropped_image_into_attachment_queue() {
        let mut app = test_app().await;
        let dir = crate::improve::testing::temp_dir("paste-image");
        let path = dir.path.join("dropped.png");
        std::fs::write(&path, b"\x89PNG-bytes").expect("write fixture");
        // Terminals drop files as a quoted pasted path.
        let pasted = format!("'{}'", path.display());
        app.on_paste(&pasted);
        assert_eq!(app.pending_images.len(), 1, "dropped png queues");
    }

    #[tokio::test]
    async fn paste_inserts_non_image_file_path_into_composer() {
        let mut app = test_app().await;
        app.on_paste("/tmp/reports/notes.txt");
        assert_eq!(app.composer.value(), "/tmp/reports/notes.txt ");
        // A second drop appends next to the first.
        app.on_paste("~/src/main.rs");
        assert_eq!(
            app.composer.value(),
            "/tmp/reports/notes.txt ~/src/main.rs "
        );
    }

    #[tokio::test]
    async fn typed_drop_burst_is_rerouted_out_of_the_composer() {
        let mut app = test_app().await;
        let dir = crate::improve::testing::temp_dir("typed-drop");
        let path = dir.path.join("shot.png");
        std::fs::write(&path, b"\x89PNG-bytes").expect("write fixture");
        let dropped = path.display().to_string();

        // Simulate a terminal that types the drop as keystrokes: the path
        // lands in the composer, then the burst is recognised.
        app.composer.insert_str(&dropped);
        assert!(app.reroute_drop_burst(&dropped), "burst should be routed");
        assert!(app.composer.is_empty(), "path stripped from composer");
        assert_eq!(app.pending_images.len(), 1, "image attached instead");

        // A burst that is not the composer's tail is left untouched.
        app.composer.insert_str("prefix");
        assert!(!app.reroute_drop_burst(&dropped));
        assert_eq!(app.composer.value(), "prefix");
    }

    #[tokio::test]
    async fn paste_plain_text_goes_into_composer_verbatim() {
        let mut app = test_app().await;
        let text = "hello world\nsecond line";
        app.on_paste(text);
        assert_eq!(app.composer.value(), text, "prose pastes verbatim");
        assert!(app.pending_images.is_empty());
    }

    #[tokio::test]
    async fn paste_lands_in_the_focused_connect_field_not_the_composer() {
        let mut app = test_app().await;
        app.open_connect();
        if let Some(Overlay::Connect(form)) = app.overlay.as_mut() {
            form.focus = 1; // the api key row
        }
        // Terminals usually append a newline to a copied key.
        app.on_paste("  nvapi-test-key-123 \n");
        let Overlay::Connect(form) = app.overlay.as_ref().unwrap() else {
            panic!("connect overlay stays open");
        };
        assert_eq!(form.field(1).value(), "nvapi-test-key-123");
        assert_eq!(
            form.field(0).value(),
            app.cfg.base_url,
            "base url untouched"
        );
        assert!(app.composer.is_empty(), "paste must not reach the composer");
    }

    #[tokio::test]
    async fn paste_into_connect_base_url_field_targets_focus_zero() {
        let mut app = test_app().await;
        app.open_connect(); // focus starts on base url
        app.on_paste("https://integrate.api.nvidia.com/v1\n");
        let Overlay::Connect(form) = app.overlay.as_ref().unwrap() else {
            panic!("connect overlay stays open");
        };
        // The field is pre-filled from config; the paste lands in it at the
        // cursor, so it must end with the pasted URL.
        assert!(form
            .field(0)
            .value()
            .ends_with("https://integrate.api.nvidia.com/v1"));
        assert!(app.composer.is_empty());
    }

    #[test]
    fn paste_paths_parses_quotes_uris_and_rejects_prose() {
        let cwd = std::path::Path::new(".");
        assert_eq!(paste_paths("hello world", cwd), Vec::<String>::new());
        assert_eq!(
            paste_paths("'/tmp/a b/shot.png'", cwd),
            vec!["/tmp/a b/shot.png".to_string()]
        );
        assert_eq!(
            paste_paths("file:///tmp/a%20b/shot.png", cwd),
            vec!["/tmp/a b/shot.png".to_string()]
        );
        assert_eq!(
            paste_paths("/tmp/a.png /tmp/b.rs", cwd),
            vec!["/tmp/a.png".to_string(), "/tmp/b.rs".to_string()]
        );
        // Extension-only relative names are path-like too.
        assert_eq!(paste_paths("notes.md", cwd), vec!["notes.md".to_string()]);
    }

    #[test]
    fn paste_paths_handles_backslash_escaped_and_backticked_paths() {
        let cwd = std::path::Path::new(".");
        // kitty / WezTerm shell-escape dropped paths: a backslash keeps the
        // space inside one token, and several files stay separate tokens.
        assert_eq!(
            paste_paths("/tmp/a\\ b/shot.png", cwd),
            vec!["/tmp/a b/shot.png".to_string()]
        );
        assert_eq!(
            paste_paths("/tmp/a\\ b.png /tmp/c.png", cwd),
            vec!["/tmp/a b.png".to_string(), "/tmp/c.png".to_string()]
        );
        // Backtick-wrapped paste from a shell.
        assert_eq!(
            paste_paths("`/tmp/a b/shot.png`", cwd),
            vec!["/tmp/a b/shot.png".to_string()]
        );
    }

    #[test]
    fn paste_paths_keeps_a_single_unquoted_spaced_path_that_exists_whole() {
        let dir = crate::improve::testing::temp_dir("paste-spaced");
        let path = dir.path.join("my shot.png");
        std::fs::write(&path, b"x").expect("write fixture");
        let dropped = path.display().to_string();
        // No quoting or escaping: only the on-disk check can tell that the
        // spaces belong to the filename rather than separating two paths.
        assert_eq!(paste_paths(&dropped, &dir.path), vec![dropped.clone()]);
    }

    #[test]
    fn paste_paths_splits_newline_separated_drops() {
        let cwd = std::path::Path::new(".");
        assert_eq!(
            paste_paths("/tmp/a.png\n/tmp/b.rs\n", cwd),
            vec!["/tmp/a.png".to_string(), "/tmp/b.rs".to_string()]
        );
    }

    #[test]
    fn dropped_path_burst_accepts_real_paths_and_rejects_prose() {
        let dir = crate::improve::testing::temp_dir("drop-burst");
        let path = dir.path.join("shot.png");
        std::fs::write(&path, b"x").expect("write fixture");
        assert!(is_dropped_path_burst(&path.display().to_string()));
        // Ordinary typing never looks like an absolute path on disk.
        assert!(!is_dropped_path_burst("hello world"));
        assert!(!is_dropped_path_burst("/no/such/file.png"));
        assert!(!is_dropped_path_burst("relative.png"));
    }

    #[test]
    fn percent_decode_handles_escapes_and_passes_garbage() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%zz"), "%zz", "invalid hex survives");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn is_image_path_matches_the_attachable_extensions() {
        assert!(is_image_path("/tmp/a.PNG"));
        assert!(is_image_path("shot.jpeg"));
        assert!(!is_image_path("/tmp/notes.txt"));
        assert!(!is_image_path("/tmp/archive.tar.gz"));
    }

    #[tokio::test]
    async fn image_attachment_flows_into_the_next_turn() {
        let mut app = test_app().await;
        let dir = crate::improve::testing::temp_dir("image-turn");
        let path = dir.path.join("shot.png");
        std::fs::write(&path, b"\x89PNG-bytes").expect("write fixture");
        // A wrong-extension file must be refused outright.
        let bad = dir.path.join("notes.txt");
        std::fs::write(&bad, b"nope").expect("write fixture");

        app.run_command(Command::Image, bad.to_str().unwrap());
        assert!(app.pending_images.is_empty(), "txt must be refused");

        app.run_command(Command::Image, path.to_str().unwrap());
        assert_eq!(app.pending_images.len(), 1, "png queues");

        app.composer.insert_str("what is this");
        app.submit();
        let last = app
            .history
            .iter()
            .rfind(|m| m.role == crate::agent::provider::Role::User)
            .expect("user message");
        assert_eq!(
            last.images.as_ref().map(|v| v.len()),
            Some(1),
            "image rides on the user message"
        );
        assert!(
            app.entries
                .iter()
                .any(|e| e.role == EntryRole::User && e.text.contains("[image: shot.png")),
            "entry shows the badge"
        );
        assert!(app.pending_images.is_empty(), "queue drains on send");
    }

    #[tokio::test]
    async fn image_queue_clears_on_new_session() {
        let mut app = test_app().await;
        let dir = crate::improve::testing::temp_dir("image-new");
        let path = dir.path.join("shot.png");
        std::fs::write(&path, b"\x89PNG-bytes").expect("write fixture");
        app.run_command(Command::Image, path.to_str().unwrap());
        assert_eq!(app.pending_images.len(), 1);
        app.run_command(Command::New, "");
        assert!(app.pending_images.is_empty(), "/new drops queued images");
    }

    #[tokio::test]
    async fn edit_turn_rewinds_and_loads_the_prompt() {
        let mut app = test_app().await;
        app.entries = vec![
            Entry::new(EntryRole::User, "first prompt"),
            Entry::new(EntryRole::Assistant, "first answer"),
            Entry::new(EntryRole::User, "second prompt"),
            Entry::new(EntryRole::Assistant, "second answer"),
        ];
        app.history = vec![
            ChatMessage::user("first prompt"),
            ChatMessage::assistant("first answer"),
            ChatMessage::user("second prompt"),
            ChatMessage::assistant("second answer"),
        ];
        app.turn_history = vec![2, 2];
        app.edit_turn(2);
        assert_eq!(app.composer.value(), "second prompt");
        assert_eq!(app.entries.len(), 2, "later entries are dropped");
        assert_eq!(app.history.len(), 2);
        assert_eq!(app.turn_history, vec![2]);
    }

    #[tokio::test]
    async fn edit_turn_first_turn_clears_the_transcript() {
        let mut app = test_app().await;
        app.entries = vec![
            Entry::new(EntryRole::User, "only prompt"),
            Entry::new(EntryRole::Assistant, "only answer"),
        ];
        app.history = vec![
            ChatMessage::user("only prompt"),
            ChatMessage::assistant("only answer"),
        ];
        app.turn_history = vec![2];
        app.edit_turn(1);
        assert_eq!(app.composer.value(), "only prompt");
        assert!(app.entries.is_empty());
        assert!(app.history.is_empty());
        assert!(app.turn_history.is_empty());
    }

    #[tokio::test]
    async fn edit_turn_rejects_out_of_range_turns() {
        let mut app = test_app().await;
        app.entries = vec![Entry::new(EntryRole::User, "only prompt")];
        app.turn_history = vec![1];
        app.edit_turn(0);
        assert!(app.composer.value().is_empty());
        assert_eq!(app.entries.len(), 1);
        app.edit_turn(5);
        assert_eq!(app.entries.len(), 1);
        assert!(app.composer.value().is_empty());
    }

    #[tokio::test]
    async fn edit_turn_strips_image_badges_from_the_prompt() {
        let mut app = test_app().await;
        app.entries = vec![Entry::new(
            EntryRole::User,
            "[image: shot.png · 1 KB]\nwhat is this",
        )];
        app.turn_history = vec![1];
        app.edit_turn(1);
        assert_eq!(app.composer.value(), "what is this");
        let (toast, _) = app.toast.clone().expect("toast");
        assert!(toast.contains("images not re-attached"), "{toast}");
    }

    fn confirm_key(c: char) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        )
    }

    #[tokio::test]
    async fn confirm_prompt_round_trips_and_session_grants_answer_fast() {
        let mut cfg = test_config();
        cfg.confirm_external = true;
        let mut app = App::new(cfg, tokio::runtime::Handle::current()).expect("app");
        let confirmer = app.confirmer.clone().expect("confirmer attached");

        // First ask: the request parks on the app until the user answers.
        let pending = {
            let c = Arc::clone(&confirmer);
            tokio::spawn(async move { c.confirm("run_command", Some("ls".into())).await })
        };
        for _ in 0..1000 {
            app.pump_engine();
            if app.pending_confirm.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        let request = app.pending_confirm.take().expect("pending request");
        assert_eq!(request.tool, "run_command");
        assert_eq!(request.preview.as_deref(), Some("ls"));
        app.pending_confirm = Some(request);
        app.on_key_confirm(confirm_key('a'));
        assert!(pending.await.expect("join"), "always-allow answers true");

        // The grant fast-paths future calls without queueing anything.
        assert!(confirmer.confirm("run_command", None).await);
        app.pump_engine();
        assert!(app.pending_confirm.is_none(), "a grant must not re-prompt");

        // Deny + always-deny answers false and remembers it for the session.
        let denied = {
            let c = Arc::clone(&confirmer);
            tokio::spawn(async move { c.confirm("ssh_run", None).await })
        };
        for _ in 0..1000 {
            app.pump_engine();
            if app.pending_confirm.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        assert!(app.pending_confirm.is_some(), "ungranted tool asks again");
        app.on_key_confirm(confirm_key('d'));
        assert!(!denied.await.expect("join"), "always-deny answers false");
        assert!(!confirmer.confirm("ssh_run", None).await);
    }

    #[test]
    fn memory_age_label_reads_as_a_relative_duration_then_a_date() {
        let now = crate::memory::model::now();
        let row = |created_at| MemoryRow {
            id: "mem_test".into(),
            title: "t".into(),
            category: "core".into(),
            tags: Vec::new(),
            created_at,
            project: None,
            content: String::new(),
        };
        assert_eq!(row(now - 30).age_label(), "30s");
        assert_eq!(row(now - 600).age_label(), "10m");
        assert_eq!(row(now - 7_200).age_label(), "2h");
        assert_eq!(row(now - 3 * 86_400).age_label(), "3d");
        // Old enough that a calendar date is the useful label.
        let old = row(now - 400 * 86_400).age_label();
        assert_eq!(old.len(), 10, "expected YYYY-MM-DD, got {old}");
        assert!(old.starts_with("20"), "unexpected date {old}");
    }

    #[test]
    fn memory_hint_is_quiet_while_the_engine_is_attached() {
        let row = MemoryRow {
            id: "mem_1".into(),
            title: "t".into(),
            category: "core".into(),
            tags: vec!["a".into()],
            created_at: 0,
            project: Some("nanomite".into()),
            content: String::new(),
        };
        assert!(row.hint().starts_with("core · nanomite · 1 · "));
    }

    #[tokio::test]
    async fn memory_picker_lists_records_and_opens_one() {
        let mut app = test_app().await;
        let engine = Arc::new(
            MemoryEngine::open_in_memory(crate::memory::policy::MemoryPolicy {
                mirror_markdown: false,
                ..Default::default()
            })
            .expect("in-memory engine"),
        );
        engine
            .save(&crate::memory::model::NewMemory {
                category: crate::memory::model::MemoryCategory::Core,
                content: "nanomite stores memory in sqlite".to_string(),
                summary: Some("memory lives in sqlite".to_string()),
                tags: vec!["storage".to_string()],
                source: crate::memory::model::MemorySource::new(
                    crate::memory::model::SourceKind::UserInstruction,
                ),
                project: None,
                confidence: Some(1.0),
                importance: Some(0.9),
                expires_at: None,
                sensitivity: Some(crate::memory::model::Sensitivity::Internal),
                mirror_markdown: false,
            })
            .expect("save");
        app.attach_memory_for_test(engine);

        app.open_memory_picker();
        assert!(matches!(
            app.overlay.as_ref().map(Overlay::kind),
            Some(OverlayKind::Picker)
        ));

        // The rows come back from the blocking task, never synchronously.
        for _ in 0..200 {
            app.pump_engine();
            if let Some(Overlay::Picker(p)) = &app.overlay {
                if !p.items.is_empty() {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let Some(Overlay::Picker(picker)) = &app.overlay else {
            panic!("picker closed");
        };
        assert_eq!(picker.items.len(), 1, "rows: {:?}", picker.items);
        assert_eq!(picker.items[0].label, "memory lives in sqlite");
        assert!(picker.items[0].hint.starts_with("core"));

        // Enter on the row opens the read-only record overlay.
        let value = picker.selection().expect("a row");
        app.overlay = None;
        app.apply_picker_value(value);
        let Some(Overlay::Memory(view)) = &app.overlay else {
            panic!("record overlay not opened");
        };
        assert!(view.record.content.contains("sqlite"));
    }

    #[tokio::test]
    async fn memory_picker_reports_absence_instead_of_hanging() {
        let mut app = test_app().await;
        assert!(!app.memory_attached());
        app.open_memory_picker();
        assert!(app.overlay.is_none(), "no picker without an engine");
        let (toast, _) = app.toast.clone().expect("hint");
        assert!(toast.contains("memory not available"), "{toast}");
    }

    #[tokio::test]
    async fn undo_cuts_history_at_the_exact_message_boundary() {
        let mut app = test_app().await;
        // One tool-bearing turn: the scripted provider emits a `list_files`
        // call before it answers, so the turn owns more than one message.
        app.composer.insert_str("list the files");
        app.submit();
        // The scripted provider keeps asking for the file list, so this turn is
        // interrupted once it has really used a tool - that is the shape the old
        // user-count truncation got wrong.
        for _ in 0..2_000 {
            app.pump_engine();
            if app
                .entries
                .get(1)
                .is_some_and(|e| e.tools.iter().any(|t| !t.running))
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(
            app.entries[1].tools.iter().any(|t| t.name == "list_files"),
            "tool-bearing turn expected: {:?}",
            app.entries[1].tools
        );
        app.interrupt();
        for _ in 0..500 {
            app.pump_engine();
            if !app.is_busy() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(!app.is_busy(), "interrupt did not finish the turn");
        let messages = app.history_snapshot();
        assert_eq!(messages.len(), 2, "user + assistant: {messages:?}");
        assert_eq!(messages[0].role, crate::agent::provider::Role::User);
        assert_eq!(messages[1].role, crate::agent::provider::Role::Assistant);

        app.run_command(Command::Undo, "");
        // Exact boundary: nothing of the undone turn is left in the context the
        // next prompt is built from - not the user message, not the reply.
        assert_eq!(app.history_snapshot().len(), 0);
        assert!(app.entries.iter().all(|e| e.role != EntryRole::User));

        // A second turn now sees an empty context, and its own boundary is exact.
        app.composer.insert_str("hello there");
        app.submit();
        for _ in 0..2_000 {
            app.pump_engine();
            if !app.is_busy() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let after = app.history_snapshot();
        assert_eq!(after.len(), 2, "{after:?}");
        assert!(after[0].content.contains("hello there"), "{after:?}");

        // Redo is last-in-first-out: undo turn 2, then replay both turns. Each
        // restores exactly the messages it cut, no more and no less.
        app.run_command(Command::Undo, "");
        assert_eq!(app.history_snapshot().len(), 0);
        app.run_command(Command::Redo, "");
        app.run_command(Command::Redo, "");
        let redone = app.history_snapshot();
        assert_eq!(redone.len(), 4, "{redone:?}");
        assert_eq!(app.entries.len(), 4, "{:?}", app.entries.len());
        assert!(
            redone.iter().any(|m| m.content.contains("hello there")),
            "{redone:?}"
        );
        assert_eq!(
            redone
                .iter()
                .filter(|m| m.content == messages[0].content)
                .count(),
            1,
            "the undone user message must come back exactly once"
        );
        assert_eq!(
            redone
                .iter()
                .filter(|m| m.content == messages[1].content)
                .count(),
            1,
            "the undone reply must come back exactly once"
        );
    }

    #[tokio::test]
    async fn copy_hands_the_last_assistant_message_to_the_terminal() {
        let mut app = test_app().await;
        let (tx, mut rx) = unbounded_channel();
        app.set_control_channel(tx);

        app.run_command(Command::Copy, "");
        assert!(
            rx.try_recv().is_err(),
            "nothing to copy before an assistant reply"
        );

        app.composer.insert_str("say something");
        app.submit();
        for _ in 0..400 {
            app.pump_engine();
            if !app.is_busy() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        app.run_command(Command::Copy, "");
        match rx.try_recv().expect("clipboard payload") {
            Control::Copy(text) => assert!(text.contains("Scripted test reply"), "{text}"),
            other => panic!("unexpected control: {other:?}"),
        }
    }

    // -- follow-up suggestions (opencode-style) ------------------------------

    async fn run_turn_to_completion(app: &mut App) {
        for _ in 0..400 {
            app.pump_engine();
            if !app.is_busy() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!app.is_busy(), "turn never finished");
    }

    #[tokio::test]
    async fn suggestion_strip_mounts_after_a_finished_turn() {
        let mut app = test_app().await;
        app.composer.insert_str("hello there");
        app.submit();
        run_turn_to_completion(&mut app).await;

        // The side-call runs through the same offline provider and mounts
        // once its reply has been parsed.
        for _ in 0..400 {
            app.pump_engine();
            if app.suggestions.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let strip = app
            .suggestions
            .as_ref()
            .expect("finished turn must mount suggestions");
        assert!(!strip.items.is_empty(), "no suggestions parsed");
        assert_eq!(strip.selected, 0);
        assert!(strip.items.len() <= 3, "capped at the promised three");

        // Submitting a new prompt clears the strip immediately.
        app.composer.insert_str("next question");
        app.submit();
        assert!(app.suggestions.is_none(), "a new turn clears the strip");
    }

    #[tokio::test]
    async fn stale_or_busy_suggestion_results_never_mount() {
        let mut app = test_app().await;
        app.suggest_gen = 7;
        app.apply_suggest_result(SuggestResult {
            gen: 6,
            items: vec!["stale".into()],
        });
        assert!(app.suggestions.is_none(), "stale gen must be dropped");

        // A mid-turn reply waits: the strip would otherwise pop up under a
        // turn that already started.
        app.composer.insert_str("go");
        app.submit();
        app.apply_suggest_result(SuggestResult {
            gen: app.suggest_gen,
            items: vec!["mid-turn".into()],
        });
        assert!(app.suggestions.is_none(), "busy app must not mount");
        run_turn_to_completion(&mut app).await;
        // And the cleared-on-start rule keeps it away after the turn too.
        assert!(app.suggestions.is_none());
    }

    #[tokio::test]
    async fn suggestion_keys_cycle_send_and_dismiss() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let key = |code: KeyCode| crossterm::event::KeyEvent::new(code, KeyModifiers::NONE);

        let mut app = test_app().await;
        app.suggestions = Some(SuggestionStrip {
            items: vec!["run the tests".into(), "commit the fix".into()],
            selected: 0,
        });

        // Plain Tab cycles the highlight (wrapping), not ghost text.
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.suggestions.as_ref().unwrap().selected, 1);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.suggestions.as_ref().unwrap().selected, 0, "wraps");
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.suggestions.as_ref().unwrap().selected, 1);

        // Enter submits the highlighted prompt through the normal path.
        app.on_key(key(KeyCode::Enter));
        assert!(app.suggestions.is_none(), "accept clears the strip");
        assert_eq!(app.entries[0].role, EntryRole::User);
        assert_eq!(app.entries[0].text, "commit the fix");
        run_turn_to_completion(&mut app).await;

        // Typing dismisses and types normally; Esc just dismisses.
        app.suggestions = Some(SuggestionStrip {
            items: vec!["another".into()],
            selected: 0,
        });
        app.on_key(key(KeyCode::Char('x')));
        assert!(app.suggestions.is_none(), "typing dismisses");
        assert_eq!(app.composer.value(), "x");

        app.suggestions = Some(SuggestionStrip {
            items: vec!["another".into()],
            selected: 0,
        });
        app.on_key(key(KeyCode::Esc));
        assert!(app.suggestions.is_none(), "esc dismisses");
        assert_eq!(app.composer.value(), "x", "esc must not clear the composer");
    }
}
