use crate::agent::openai::OpenAiProvider;
use crate::agent::provider::{AgentProvider, ChatMessage, Usage};
use crate::agent::runtime::{AgentEvent, AgentRuntime, Cancel};
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
use crate::ui::composer::Composer;
use crate::ui::theme::Theme;
use crate::ui::theme::ThemeId;
use crate::ui::View;
use ratatui::text::Line;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

const MAX_HISTORY_MESSAGES: usize = 40;
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
}

/// Cached, width-dependent rendering of one transcript entry.
#[derive(Debug, Clone, Default)]
pub struct Rendered {
    pub rev: u64,
    pub width: u16,
    /// `/details` state this cache was built for.
    pub details: bool,
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
    pub elapsed: Option<Duration>,
    pub usage: Option<Usage>,
    /// Agent steps taken this turn (one per tool round-trip). Shown live in
    /// the status strip so long runs are visibly making progress.
    pub steps: usize,
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
            elapsed: None,
            usage: None,
            steps: 0,
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
    Themes,
    Memory,
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
            PickerMode::Themes => "themes",
            PickerMode::Memory => "memory entries",
        }
    }

    pub fn placeholder(&self) -> &'static str {
        match self.mode {
            PickerMode::Commands => "type to filter",
            PickerMode::Model => "filter, or type a custom model id",
            PickerMode::Sessions => "pick a session to resume",
            PickerMode::Themes => "pick a colour theme",
            PickerMode::Memory => "filter stored memories",
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

/// Work the main loop must do on the UI thread (it owns the terminal).
#[derive(Debug, Clone)]
pub enum Control {
    /// Open $EDITOR seeded with the composer buffer.
    EditComposer(String),
    /// Open $EDITOR on a file.
    EditFile(std::path::PathBuf),
    /// Write `text` to the terminal clipboard with an OSC 52 sequence.
    Copy(String),
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
    history: Vec<ChatMessage>,
    /// Per user turn, how many `ChatMessage`s that turn appended (a user
    /// message plus its assistant/tool-bearing reply). `/undo` and `/redo`
    /// cut on these exact counts instead of guessing from user-message counts.
    turn_history: Vec<usize>,
    /// Index of the in-flight transcript entry while a turn runs.
    pub(crate) active: Option<usize>,
    /// Index of the current turn for browsing (0 = latest, increments toward older).
    turn_index: usize,
    dirty: bool,
    quit: bool,
    /// Painted composer text width, synced each frame by `ui::render`.
    pub composer_layout_width: usize,
}

impl App {
    pub fn new(
        cfg: Config,
        handle: tokio::runtime::Handle,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let theme = Theme::with_id(ThemeId::from_label(&cfg.theme).unwrap_or(ThemeId::Mono));
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
        self.overlay.is_none()
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
                    });
                    entry.touch();
                }
            }
            AgentEvent::ToolEnd { name, detail, ok } => {
                if let Some(entry) = self.entries.get_mut(index) {
                    if let Some(row) = entry
                        .tools
                        .iter_mut()
                        .rev()
                        .find(|r| r.running && r.name == name)
                    {
                        row.running = false;
                        row.ok = ok;
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
                        });
                    }
                    entry.touch();
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
        self.active = None;
        self.cancel = None;
        self.turn_started = None;
        self.view.scroll_from_bottom = 0;
        self.mark_dirty();
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

    pub fn on_tick(&mut self) {
        if self.is_busy() {
            self.mark_dirty();
        }
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
        if self.is_busy() {
            // One turn at a time: requeue by telling the user instead of dropping text.
            self.composer.insert_str(&prompt);
            self.toast("still working - press esc to interrupt");
            self.mark_dirty();
            return;
        }

        self.entries
            .push(Entry::new(EntryRole::User, prompt.clone()));
        self.history.push(ChatMessage::user(prompt.clone()));
        // This turn starts here; every message the engine appends for it is
        // counted onto this turn, so `/undo` cuts at the exact boundary.
        self.turn_history.push(1);
        self.trim_history();

        let index = self.entries.len();
        self.entries.push(Entry::streaming());
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
            Command::Details => {
                self.show_tool_details = !self.show_tool_details;
                self.toast(format!("tool details: {}", on_off(self.show_tool_details)));
                self.invalidate_transcript();
            }
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
        self.entries.push(Entry::streaming());
        let last = self.entries.len() - 1;
        self.entries[last].text = summary;
        self.entries.push(Entry::streaming());
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
        for meta in session::list() {
            rows.push(PickerItem {
                label: meta.preview,
                hint: format!("{} · {} turns", meta.label, meta.turns),
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
                self.mark_dirty();
                self.toast(format!("resumed {}", self.turn_count()));
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
        session::StoredSession::capture(
            &self.entries,
            &self.provider_label(),
            &self.model_label(),
            &self.cwd,
            SystemTime::now(),
        )
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
        self.entries.clear();
        self.history.clear();
        self.turn_history.clear();
        self.usage = Usage::default();
        self.session_started = Instant::now();
        self.undo_stack.clear();
        self.active = None;
        self.turn_started = None;
        self.view.scroll_from_bottom = 0;
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
        self.entries.clear();
        self.turn_history.clear();
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
        self.toast("selection copied to clipboard");
        self.mark_dirty();
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
            // `/` opens the command menu; with text on the composer it types literally.
            KeyCode::Char('/') if self.composer.is_empty() => self.open_palette(),
            KeyCode::Char('x') if ctrl => self.open_palette(),
            KeyCode::Char('t') if ctrl => self.new_session(),
            KeyCode::Char('l') if ctrl => self.clear_transcript(),
            KeyCode::Char('r') if ctrl => self.cycle_reasoning(),

            KeyCode::Esc => {
                if self.is_busy() {
                    self.interrupt();
                } else if !self.composer.is_empty() {
                    self.composer.clear();
                    self.mark_dirty();
                }
            }

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

            KeyCode::Left => {
                if ctrl || alt {
                    self.composer.move_word_left();
                } else {
                    self.composer.move_left();
                }
            }
            KeyCode::Right => {
                if ctrl || alt {
                    self.composer.move_word_right();
                } else {
                    self.composer.move_right();
                }
            }
            KeyCode::Home if !ctrl => self.composer.move_home(),
            KeyCode::End if !ctrl => self.composer.move_end(),
            KeyCode::Home => self.view.scroll_to_top(),
            KeyCode::End => self.scroll_to_end(),
            KeyCode::Backspace => self.composer.backspace(),
            KeyCode::Delete => self.composer.delete_forward(),

            KeyCode::Up => self.composer_or_scroll(key, -1),
            KeyCode::Down => self.composer_or_scroll(key, 1),
            KeyCode::PageUp => self.scroll_page(1),
            KeyCode::PageDown => self.scroll_page(-1),

            KeyCode::Char(c) => {
                if ctrl || alt {
                    return;
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
        }
    }

    pub fn on_mouse(&mut self, kind: crossterm::event::MouseEventKind) {
        use crossterm::event::MouseEventKind;
        match kind {
            // OpenCode/standard direction: wheel up scrolls toward older
            // messages, wheel down toward newer (scroll_by(+) moves back
            // into history; delta 0 pins to the live bottom).
            MouseEventKind::ScrollUp => self.scroll_by(3),
            MouseEventKind::ScrollDown => self.scroll_by(-3),
            _ => {}
        }
    }
}

impl View {
    pub fn viewport_rows(&self) -> u16 {
        self.cached_chat_height().unwrap_or(10)
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
        ThemeId::Opencode => "warm gold on near-black",
        ThemeId::Midnight => "cool slate · periwinkle accent",
        ThemeId::Graphite => "teal · orange on deep charcoal",
        ThemeId::Umbrella => "blood red · clinical white",
    }
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
}
