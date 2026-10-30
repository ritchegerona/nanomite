use nanomite::app::{App, Control};
use nanomite::ui;
use ratatui::crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};
use std::sync::mpsc;
use std::time::Duration;

/// Frame interval: only used to animate the spinner; the loop is otherwise idle.
const ANIMATION_TICK: Duration = Duration::from_millis(90);

type Backend = CrosstermBackend<Stdout>;

#[tokio::main]
async fn main() -> io::Result<()> {
    install_panic_hook();
    log_event(&format!(
        "start v{} (pid {})",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    ));

    // `nanomite --worktree <branch>`: create/reuse a managed git worktree and
    // open the agent inside it, so parallel agents never share a checkout.
    let mut args = std::env::args().skip(1);
    let mut worktree_branch: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--worktree" => match args.next() {
                Some(b) if !b.starts_with('-') => worktree_branch = Some(b),
                Some(other) => {
                    eprintln!("nanomite: --worktree needs a branch name (got {other})");
                    return Ok(());
                }
                None => {
                    eprintln!("nanomite: --worktree needs a branch name");
                    return Ok(());
                }
            },
            "update" => {
                return nanomite::update::run();
            }
            "--version" | "-V" => {
                println!("nanomite {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!(
                    "nanomite {} - terminal coding agent",
                    env!("CARGO_PKG_VERSION")
                );
                println!("usage: nanomite [--worktree <branch>] [update]");
                return Ok(());
            }
            other => {
                eprintln!("nanomite: unknown argument `{other}` (try --help)");
                return Ok(());
            }
        }
    }
    let mut cfg = nanomite::config::Config::load();
    if let Some(branch) = &worktree_branch {
        match nanomite::worktree::ensure(&cfg.workspace, branch) {
            Ok(path) => {
                eprintln!("nanomite: worktree ready at {}", path.display());
                // Open the agent in the worktree: workspace = worktree path.
                cfg.workspace = path;
            }
            Err(e) => {
                eprintln!("nanomite: {e}");
                return Ok(());
            }
        }
    }
    let handle = tokio::runtime::Handle::current();

    let mut terminal = setup_terminal()?;
    let mut app = match App::new(cfg, handle) {
        Ok(app) => app,
        Err(e) => {
            restore_terminal(&mut terminal)?;
            eprintln!("nanomite: {e}");
            return Err(io::Error::other(e.to_string()));
        }
    };

    let (control_tx, mut control_rx) = tokio::sync::mpsc::unbounded_channel::<Control>();
    app.set_control_channel(control_tx);
    let mut ticker = tokio::time::interval(ANIMATION_TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Terminal events are pumped on a plain OS thread and handed over through a
    // channel. crossterm's async `EventStream` cannot be polled inside
    // `tokio::select!` here: the 90 ms animation tick cancels its `next()`
    // future on every loop pass, and a cancelled read DROPS any bytes already
    // buffered from the tty. A mouse escape sequence (`ESC [ < b ; c ; d M`)
    // often arrives split across two reads, so dropping the tail leaves
    // `35;93;33M`-style fragments in the input queue that later surface as
    // garbage keypresses the moment the mouse moves. The synchronous
    // `event::read()` on a dedicated thread never drops anything: it blocks
    // until a complete event has parsed.
    let (event_tx, event_rx_async) = mpsc::channel::<Event>();
    std::thread::Builder::new()
        .name("crossterm-events".into())
        .spawn(move || {
            // Blocks until a complete event parses; never drops partial input.
            while let Ok(event) = ratatui::crossterm::event::read() {
                if event_tx.send(event).is_err() {
                    break; // receiver gone: app is shutting down
                }
            }
            // `read()` only errors when the tty is closed or raw mode is gone.
        })
        .expect("spawn crossterm event thread");

    // Async bridge so `select!` can await events. A blocking task relays from
    // the std channel into an unbounded tokio channel; a cancelled poll in the
    // select arm only releases the tokio receiver, never an event already
    // queued, so nothing is lost between loop passes.
    let (relay_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
    tokio::task::spawn_blocking(move || {
        while let Ok(event) = event_rx_async.recv() {
            if relay_tx.send(event).is_err() {
                break;
            }
        }
    });

    // Adaptive streaming throttle: while a turn streams, deltas arrive
    // faster than any terminal can paint, so engine-driven redraws coalesce
    // to ~30 fps and the loop spends its time ingesting tokens rather than
    // diffing frames. Input is exempt: a key or pointer event always paints
    // the very next frame, so selection and caret tracking never wait for
    // the tick — and a burst of drag events drains into ONE frame instead
    // of one frame per event.
    const STREAM_FRAME: std::time::Duration = std::time::Duration::from_millis(33);
    let mut last_draw = std::time::Instant::now() - STREAM_FRAME;
    let mut user_input = false;

    let result = loop {
        // Drain queued engine events (streaming deltas, tool calls, board
        // messages) every pass. Pumping only on the 90ms animation tick
        // capped visible text updates at ~11 fps, which read as a slow
        // response; try_recv-based draining is free when the queue is empty.
        app.pump_engine();

        if app.needs_redraw() {
            let now = std::time::Instant::now();
            if user_input || !app.is_busy() || now.duration_since(last_draw) >= STREAM_FRAME {
                let frame_start = std::time::Instant::now();
                terminal.draw(|frame| ui::render(frame, &mut app))?;
                app.record_frame(frame_start.elapsed().as_micros() as u64);
                app.clear_dirty();
                last_draw = now;
            }
            user_input = false;
        }

        tokio::select! {
            // Cancellation-safe: the unbounded channel's `recv` keeps any
            // event already delivered when the poll is dropped by a tick, so
            // nothing from the tty is ever lost between loop passes.
            Some(event) = event_rx.recv() => {
                let mut interactive = false;
                match event {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        app.on_key(key);
                        interactive = true;
                    }
                    Event::Mouse(m) => {
                        app.on_mouse(m);
                        interactive = true;
                    }
                    Event::Resize(_, _) => {
                        app.on_resize();
                        interactive = true;
                    }
                    Event::Paste(text) => {
                        // Dropped files arrive as bracketed paste: usually
                        // one or more paths (optionally file:// URIs). Route
                        // through the app so images attach and paths insert.
                        app.on_paste(&text);
                        interactive = true;
                    }
                    _ => {}
                }
                // Drain the rest of an input burst (a drag can queue dozens
                // of motion events) so the frame paints once, at the final
                // position, instead of once per event.
                loop {
                    match event_rx.try_recv() {
                        Ok(Event::Key(key)) if key.kind != KeyEventKind::Release => {
                            app.on_key(key)
                        }
                        Ok(Event::Mouse(m)) => app.on_mouse(m),
                        Ok(Event::Resize(_, _)) => app.on_resize(),
                        Ok(Event::Paste(text)) => app.on_paste(&text),
                        Ok(_) => {}
                        Err(_) => break, // queue drained
                    }
                }
                if interactive {
                    user_input = true;
                }
            }
            Some(control) = control_rx.recv() => {
                let result = run_control(&mut terminal, control, &mut app);
                if let Err(e) = result {
                    app.on_editor_result(None);
                    eprintln!("nanomite: {e}");
                }
                continue;
            }
            _ = ticker.tick() => {
                app.on_tick();
            }
            else => break Ok(()),
        }

        if app.should_quit() {
            break Ok(());
        }
    };

    restore_terminal(&mut terminal)?;
    result
}

/// Leaves the alternate screen, runs `$EDITOR`, and returns whatever was written.
///
/// The TUI is suspended rather than spawned blindly: the editor needs the real terminal.
fn run_control(
    terminal: &mut Terminal<Backend>,
    control: Control,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // OSC 52 needs no suspension: the sequence is consumed by the terminal,
    // not painted, so the alternate screen and raw mode stay exactly as they
    // were. Sudo results likewise: they only append a transcript entry.
    if let Control::Copy(text) = control {
        return write_clipboard(&text);
    }
    if let Control::SudoResult(text) = &control {
        app.on_sudo_result(text.clone());
        return Ok(());
    }
    if let Control::RevertDone {
        turn,
        restored,
        failed,
    } = &control
    {
        app.on_revert_done(*turn, *restored, *failed);
        return Ok(());
    }
    if let Control::DoctorReport(lines) = &control {
        app.on_doctor_report(lines.clone());
        return Ok(());
    }
    if let Control::HistoryResults { query, hits } = &control {
        app.on_history_results(query.clone(), hits.clone());
        return Ok(());
    }
    if let Control::ResumeHint(hint) = &control {
        app.on_resume_hint(hint.clone());
        return Ok(());
    }
    if let Control::ForkSaved { path } = &control {
        app.on_fork_saved(path.clone());
        return Ok(());
    }
    if let Control::ImageAttached(result) = &control {
        app.on_image_attached(result.clone());
        return Ok(());
    }
    if let Control::ExportDone(result) = &control {
        app.on_export_done(result.clone());
        return Ok(());
    }
    if let Control::OpenLink(url) = &control {
        open_in_browser(url, app);
        return Ok(());
    }

    let (target, seed) = match &control {
        // Handled above; kept explicit so a new control variant cannot fall
        // through to the editor path by accident.
        Control::Copy(_) => return Ok(()),
        Control::SudoResult(_) => return Ok(()),
        Control::RevertDone { .. } => return Ok(()),
        Control::DoctorReport(_) => return Ok(()),
        Control::HistoryResults { .. } => return Ok(()),
        Control::ResumeHint(_) => return Ok(()),
        Control::ForkSaved { .. } => return Ok(()),
        Control::ImageAttached(_) => return Ok(()),
        Control::ExportDone(_) => return Ok(()),
        Control::OpenLink(_) => return Ok(()),
        Control::EditComposer(text) => {
            let path =
                std::env::temp_dir().join(format!("nanomite-prompt-{}.md", std::process::id()));
            std::fs::write(&path, text)?;
            (path, Some(text.clone()))
        }
        Control::EditFile(path) => (path.clone(), None),
    };

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    let mut command = std::process::Command::new(program);
    command.args(parts);
    command.arg(&target);

    suspend(terminal)?;
    let status = command.status();
    resume(terminal)?;

    match control {
        Control::Copy(_) => {}
        Control::SudoResult(_) => {}
        Control::RevertDone { .. } => {}
        Control::DoctorReport(_) => {}
        Control::HistoryResults { .. } => {}
        Control::ResumeHint(_) => {}
        Control::ForkSaved { .. } => {}
        Control::ImageAttached(_) => {}
        Control::ExportDone(_) => {}
        Control::OpenLink(_) => {}
        Control::EditComposer(_) => {
            let text = std::fs::read_to_string(&target).unwrap_or_default();
            let _ = std::fs::remove_file(&target);
            let _ = seed;
            if status.map(|s| s.success()).unwrap_or(false) {
                app.on_editor_result(Some(text));
            } else {
                app.on_editor_result(None);
            }
        }
        Control::EditFile(_) => app.on_editor_result(None),
    }
    Ok(())
}

/// Writes an OSC 52 clipboard sequence straight to the terminal.
///
/// `\x1b]52;c;<base64>\x07`: terminals that support it copy the payload onto
/// the system clipboard; the rest ignore the sequence, so this is safe to emit
/// unconditionally and needs no clipboard binary.
fn write_clipboard(text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::io::Write;
    let mut stdout = io::stdout();
    stdout.write_all(nanomite::app::osc52(text).as_bytes())?;
    stdout.flush()?;
    Ok(())
}

/// Hands a clicked chat link to the system browser.
///
/// `xdg-open` forks and exits immediately, so the event loop never blocks and
/// the browser outlives nanomite; its output is discarded so the TUI stays
/// clean. The URL arrives as a single argument — no shell — so model-authored
/// text can never inject a command.
fn open_in_browser(url: &str, app: &mut App) {
    let spawned = std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if spawned.is_err() {
        app.on_link_opened(false);
    }
}

fn suspend(terminal: &mut Terminal<Backend>) -> io::Result<()> {
    // Mouse capture is disabled BEFORE raw mode so the terminal never sees a
    // mouse report land while the tty is back in echo mode: a report printed
    // then would appear as raw `ESC[<..M` text (the bug this fixes).
    ratatui::crossterm::execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        ratatui::crossterm::terminal::Clear(ratatui::crossterm::terminal::ClearType::All)
    )?;
    ratatui::crossterm::terminal::disable_raw_mode()?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume(terminal: &mut Terminal<Backend>) -> io::Result<()> {
    // Raw mode must be back BEFORE mouse capture is re-enabled; otherwise any
    // motion event squeezed into the gap is echoed as plain text.
    ratatui::crossterm::terminal::enable_raw_mode()?;
    ratatui::crossterm::execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        ratatui::crossterm::cursor::SetCursorStyle::BlinkingBlock
    )?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    Ok(())
}

/// Appends one line to the crash log. Best-effort: called from the panic
/// hook (which must never panic itself) and once at startup as a start
/// marker, so a log always reads start -> ... -> panic for a crash, and a
/// death with a start but no panic line means a clean-exit path or a kill.
fn log_event(message: &str) {
    use std::io::Write;
    let log = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("nanomite")
        .join("crash.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{stamp}] {message}");
    }
}

/// A panic must never leave the terminal in raw mode inside the alternate screen.
/// The release profile uses `panic = "abort"`, so recovery has to happen here.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Order matters: mouse capture OFF while the tty is still echoing makes
        // the terminal print pending motion reports as literal text. Stop
        // reporting first, then drop out of raw mode and the alternate screen.
        let _ = ratatui::crossterm::execute!(
            io::stdout(),
            DisableMouseCapture,
            LeaveAlternateScreen,
            ratatui::crossterm::cursor::SetCursorStyle::DefaultUserShape,
            ratatui::crossterm::terminal::Clear(ratatui::crossterm::terminal::ClearType::All)
        );
        let _ = ratatui::crossterm::terminal::disable_raw_mode();

        // Crash forensics: persist everything worth knowing before abort
        // takes the process. Best-effort — never panic inside this hook.
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());
        let message = if let Some(m) = info.payload().downcast_ref::<&str>() {
            (*m).to_string()
        } else if let Some(m) = info.payload().downcast_ref::<String>() {
            m.clone()
        } else {
            "unknown panic payload".to_string()
        };
        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        log_event(&format!(
            "PANIC thread={thread} at {location}\n{message}\nbacktrace:\n{backtrace}"
        ));
        eprintln!(
            "nanomite crashed: {message} ({location})\nfull report: ~/.local/share/nanomite/crash.log"
        );

        previous(info);
    }));
}

fn setup_terminal() -> io::Result<Terminal<Backend>> {
    ratatui::crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    ratatui::crossterm::execute!(
        stdout,
        EnterAlternateScreen,
        ratatui::crossterm::terminal::Clear(ratatui::crossterm::terminal::ClearType::All),
        // Without mouse capture the wheel never reaches the app: in the alternate
        // screen there is no scrollback, and terminals fall back to arrow-key
        // emulation. OpenCode's TUI enables mouse reporting for the same reason.
        EnableMouseCapture,
        // Bracketed paste: dropped files and multi-line pastes arrive as one
        // `Event::Paste` instead of a storm of keystrokes the composer would
        // mangle (enter-per-line would send the message mid-paste).
        EnableBracketedPaste,
        // A blinking block caret in the composer: unambiguous over text,
        // impossible to lose. Terminals without DECSCUSR ignore this.
        ratatui::crossterm::cursor::SetCursorStyle::BlinkingBlock
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<Backend>) -> io::Result<()> {
    // Same ordering as `suspend`: capture off first, raw mode off second.
    // The cursor shape is handed back too - the user's terminal setting
    // applies again once nanomite exits.
    ratatui::crossterm::execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        DisableBracketedPaste,
        ratatui::crossterm::cursor::SetCursorStyle::DefaultUserShape,
        ratatui::crossterm::terminal::Clear(ratatui::crossterm::terminal::ClearType::All)
    )?;
    ratatui::crossterm::terminal::disable_raw_mode()?;
    terminal.show_cursor()?;
    Ok(())
}
