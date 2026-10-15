use crate::agent::openai::OpenAiProvider;
use crate::agent::provider::{AgentProvider, ChatMessage, Usage};
use crate::agent::runtime::{AgentEvent, AgentRuntime, Cancel};
#[cfg(test)]
use crate::agent::testdouble::ScriptedProvider;
use crate::commands::{self, Command};
use crate::config::{find_preset, host_of, Config, ProviderKind, ProviderPreset, PRESETS};
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
            PickerMode::Memory => "memory entries",
        }
    }

    /// True when the filter box doubles as a free-text value (custom model ids).
    pub fn accepts_free_text(&self) -> bool {
        self.mode == PickerMode::Model || self.mode == PickerMode::Memory
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
}

impl Overlay {
    #[cfg(test)]
    pub fn kind(&self) -> OverlayKind {
        match self {
            Overlay::Picker(_) => OverlayKind::Picker,
            Overlay::Providers(_) => OverlayKind::Providers,
            Overlay::Connect(_) => OverlayKind::Connect,
            Overlay::Help(_) => OverlayKind::Help,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Overlay {
    Picker(Picker),
    Providers(ProvidersState),
    Connect(ConnectForm),
    Help(HelpState),
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
}

enum ModelsResult {
    Ok(Vec<String>),
    Err(String),
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
    undo_stack: Vec<Vec<Entry>>,
    control_tx: Option<UnboundedSender<Control>>,

    runtime: AgentRuntime,
    cancel: Option<Cancel>,
    rx: UnboundedReceiver<AgentEvent>,
    tx: UnboundedSender<AgentEvent>,
    models_rx: UnboundedReceiver<ModelsResult>,
    models_tx: UnboundedSender<ModelsResult>,
    handle: tokio::runtime::Handle,
    history: Vec<ChatMessage>,
    active: Option<usize>,
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
        let theme = Theme::detect();
        let cwd = cfg.workspace.clone();

        let provider: Arc<dyn crate::agent::provider::AgentProvider> = match cfg.provider {
            ProviderKind::OpenAi => Arc::new(OpenAiProvider::new(&cfg)?),
            #[cfg(test)]
            ProviderKind::Scripted => Arc::new(ScriptedProvider::default()),
        };

        let runtime = Self::build_runtime(provider, &cfg, &cwd);

        let (tx, rx) = unbounded_channel();
        let (models_tx, models_rx) = unbounded_channel();

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
            cancel: None,
            rx,
            tx,
            models_rx,
            models_tx,
            handle,
            history: Vec::new(),
            active: None,
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

    /// Drains engine events and model-discovery results; cheap to call every tick.
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
    }

    fn rebuild_runtime(&mut self) -> Result<(), String> {
        let provider: Arc<dyn AgentProvider> = match self.cfg.provider {
            ProviderKind::OpenAi => {
                Arc::new(OpenAiProvider::new(&self.cfg).map_err(|e| e.to_string())?)
            }
            #[cfg(test)]
            ProviderKind::Scripted => Arc::new(ScriptedProvider::default()),
        };
        self.runtime = Self::build_runtime(provider, &self.cfg, &self.cwd);
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
            AgentEvent::ToolStart { name, detail } => {
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
            self.composer.clear();
            self.run_command(command);
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
    pub fn run_command(&mut self, command: Command) {
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
            Command::Reasoning => self.cycle_reasoning(),
            Command::Copy => self.copy_selection(),
            Command::Redo => self.redo(),
            Command::Sessions => self.open_sessions(),
            Command::Share => self.share_session(),
            Command::Themes => self.open_themes(),
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
        let removed: Vec<Entry> = self.entries.split_off(last_user);
        self.history.truncate(
            self.history
                .len()
                .saturating_sub(removed.iter().filter(|e| e.role == EntryRole::User).count()),
        );
        self.undo_stack.push(removed);
        self.toast(format!(
            "undid {} entr{}",
            self.undo_stack.last().map(|v| v.len()).unwrap_or(0),
            if self.undo_stack.last().is_some_and(|v| v.len() == 1) {
                "y"
            } else {
                "ies"
            }
        ));
        self.invalidate_transcript();
    }

    fn redo(&mut self) {
        let Some(restored) = self.undo_stack.pop() else {
            self.toast("nothing to redo");
            return;
        };
        let count = restored
            .iter()
            .filter(|e| e.role == EntryRole::User)
            .count();
        self.entries.extend(restored);
        for entry in self.entries.iter().rev().take(count + 1) {
            if entry.role == EntryRole::User {
                self.history.push(ChatMessage::user(entry.text.clone()));
            }
        }
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

    fn open_themes(&mut self) {
        let rows = ThemeId::ALL
            .iter()
            .map(|id| PickerItem {
                label: id.label().to_string(),
                hint: String::new(),
                value: PickerValue::Theme(*id),
                current: *id == self.theme.id,
            })
            .collect();
        self.overlay = Some(Overlay::Picker(Picker::new(PickerMode::Themes, rows)));
        self.mark_dirty();
    }

    fn apply_theme(&mut self, id: ThemeId) {
        if self.theme.id == id {
            self.toast(format!("theme: {}", id.label()));
            return;
        }
        self.theme = Theme::with_id(id);
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
            SystemTime::now()
                .checked_sub(Duration::from_secs(0))
                .unwrap_or_else(SystemTime::now),
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
        let for_picker = matches!(
            &self.overlay,
            Some(Overlay::Picker(p)) if p.mode == PickerMode::Model || p.mode == PickerMode::Memory
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

    fn apply_picker_value(&mut self, value: PickerValue) {
        match value {
            PickerValue::Action(action) => self.run_action(action),
            PickerValue::Command(command) => self.run_command(command),
            PickerValue::Session(path) => self.restore_session(&path),
            PickerValue::Theme(id) => self.apply_theme(id),
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
        // Memory rows load on demand; the picker opens empty and the
        // status line reports record counts instead of prebuilt items.
        let mut picker = Picker::new(PickerMode::Memory, Vec::new());
        picker.loading = true;
        self.overlay = Some(Overlay::Picker(picker));
        self.mark_dirty();
    }

    fn scroll_help(&mut self, delta: i32) {
        if let Some(Overlay::Help(state)) = &mut self.overlay {
            let max = crate::ui::help_row_count().saturating_sub(1);
            state.offset = (state.offset as i32 + delta).clamp(0, max as i32) as usize;
        }
        self.mark_dirty();
    }

    /// Copy the current selection or transcript to the clipboard
    fn copy_selection(&mut self) {
        // Use the system clipboard or toast a message
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
            KeyCode::PageUp => self.scroll_page(-1),
            KeyCode::PageDown => self.scroll_page(1),

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
        self.scroll_by(delta);
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
            // Reversed: scroll up moves content down (natural/"inverted" direction)
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

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}
