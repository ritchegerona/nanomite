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
use std::path::PathBuf;
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
    /// Tool-mode colour the cache was built for: the user-message bar and the
    /// assistant footer follow the active mode, like opencode's per-agent
    /// colouring.
    pub mode_color: ratatui::style::Color,
    pub mode_label: String,
    /// Fenced code blocks as `(first body row, last border row, raw source)`,
    /// rows indexing `lines` — the click-to-copy target list.
    pub code_blocks: Vec<(u16, u16, String)>,
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
        }
    }
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

        let runtime = Self::build_runtime(provider, &cfg, &cwd);

        let (tx, rx) = unbounded_channel();
        let (models_tx, models_rx) = unbounded_channel();
        let (board_tx, board_rx) = unbounded_channel();
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
        while let Ok(result) = self.suggest_rx.try_recv() {
            changed = true;
            self.apply_suggest_result(result);
        }
        if changed {
            self.mark_dirty();
        }
    }

    fn build_runtime(
        provider: Arc<dyn AgentProvider>,
        cfg: &Config,
        cwd: &std::path::Path,
    ) -> AgentRuntime {
        AgentRuntime::new(
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
        // Tool authorization policy (auto/plan/readonly).
        .with_tool_policy(cfg.tool_policy)
        // User-defined lifecycle hooks (loaded once here; NANOMITE_HOOKS=0
        // disables the whole subsystem).
        .with_hooks(Arc::new(crate::hooks::load(&crate::hooks::resolve_dir())))
        // User-defined external tools from TOML manifests
        // (NANOMITE_TOOLS_DIR overrides ~/.config/nanomite/tools).
        .with_external_tools(Arc::new(crate::tools::external_tools::ExternalTools::load(
            &crate::tools::external_tools::tools_dir(),
        )))
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
        self.runtime = match &self.memory {
            Some(engine) => {
                Self::build_runtime(provider, &self.cfg, &self.cwd).with_memory(Arc::clone(engine))
            }
            None => Self::build_runtime(provider, &self.cfg, &self.cwd),
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
                if let Some(entry) = self.entries.get_mut(index) {
                    if entry.text.trim().is_empty() {
                        entry.text = text.clone();
                    }
                    entry.phase = Phase::Done;
                    entry.elapsed = Some(elapsed);
                    entry.usage = Some(usage.clone());
                    entry.touch();
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
        self.entries
            .push(Entry::new(EntryRole::User, prompt.clone()));
        self.history.push(ChatMessage::user(prompt.clone()));
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
            Command::Revert => self.revert_turn(parse_turn_arg(args)),
            Command::Fork => self.fork_session_at(parse_turn_arg(args).unwrap_or(0)),
            Command::History => self.search_history(args),
            Command::Doctor => self.run_doctor(),
            Command::Ab => self.run_ab_test(args),
            Command::Focus => self.toggle_focus(),
            Command::Editor => self.edit_composer(),
            Command::Exit => self.quit = true,
            Command::Export => self.export_session(),
            Command::Help => self.open_help(),
            Command::Init => self.run_init(),
            Command::Models => self.open_model_picker(),
            Command::Memory => self.open_memory_picker(),
            Command::New => self.new_session(),
            Command::Panel => self.open_board(args),
            Command::Reasoning => self.cycle_reasoning(),
            Command::Copy => self.copy_selection(),
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

    fn export_session(&mut self) {
        match self.write_export() {
            Ok(path) => {
                self.open_in_editor_if_any(&path);
                self.toast(format!("exported {}", path.display()));
            }
            Err(e) => self.toast(format!("export failed: {e}")),
        }
    }

    fn share_session(&mut self) {
        match self.write_export() {
            Ok(path) => {
                self.open_in_editor_if_any(&path);
                self.toast(format!("local share: {}", path.display()));
            }
            Err(e) => self.toast(format!("share failed: {e}")),
        }
    }

    fn write_export(&self) -> std::io::Result<std::path::PathBuf> {
        let path = session::export_path();
        std::fs::write(&path, session::to_markdown(&self.stored_session()))?;
        Ok(path)
    }

    fn open_in_editor_if_any(&mut self, path: &std::path::Path) {
        if editor_available() {
            self.control_send(Control::EditFile(path.to_path_buf()));
        }
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
            // `/` opens the command menu; with text on the composer it types literally.
            KeyCode::Char('/') if self.composer.is_empty() => self.open_palette(),
            KeyCode::Char('x') if ctrl => self.open_palette(),
            KeyCode::Char('t') if ctrl => self.new_session(),
            KeyCode::Char('l') if ctrl => self.clear_transcript(),
            KeyCode::Char('r') if ctrl => self.cycle_reasoning(),
            KeyCode::Char('f') if ctrl => self.open_search(),
            KeyCode::Char('o') if ctrl => self.toggle_tool_details(),

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
            let entry_h = entry.status_rows() as usize + entry.height() as usize + 1; // +1 for separator
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
        cfg
    }

    async fn test_app() -> App {
        App::new(test_config(), tokio::runtime::Handle::current()).expect("app")
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
