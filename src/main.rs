use nanomite::app::{App, Control};
use nanomite::ui;
use ratatui::crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyEventKind,
};
use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};
use std::time::Duration;

/// Frame interval: only used to animate the spinner; the loop is otherwise idle.
const ANIMATION_TICK: Duration = Duration::from_millis(90);

type Backend = CrosstermBackend<Stdout>;

#[tokio::main]
async fn main() -> io::Result<()> {
    install_panic_hook();
    let cfg = nanomite::config::Config::load();
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
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(ANIMATION_TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let result = loop {
        if app.needs_redraw() {
            terminal.draw(|frame| ui::render(frame, &mut app))?;
            app.clear_dirty();
        }

        tokio::select! {
            Some(Ok(event)) = futures_util::StreamExt::next(&mut events) => {
                match event {
                    Event::Key(key) if key.kind != KeyEventKind::Release => app.on_key(key),
                    Event::Mouse(m) => app.on_mouse(m.kind),
                    Event::Resize(_, _) => app.on_resize(),
                    _ => {}
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
                app.pump_engine();
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
    let (target, seed) = match &control {
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

fn suspend(terminal: &mut Terminal<Backend>) -> io::Result<()> {
    ratatui::crossterm::terminal::disable_raw_mode()?;
    ratatui::crossterm::execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        ratatui::crossterm::terminal::Clear(ratatui::crossterm::terminal::ClearType::All)
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume(terminal: &mut Terminal<Backend>) -> io::Result<()> {
    ratatui::crossterm::execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    ratatui::crossterm::terminal::enable_raw_mode()?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    Ok(())
}

/// A panic must never leave the terminal in raw mode inside the alternate screen.
/// The release profile uses `panic = "abort"`, so recovery has to happen here.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = ratatui::crossterm::terminal::disable_raw_mode();
        let _ = ratatui::crossterm::execute!(
            io::stdout(),
            DisableMouseCapture,
            LeaveAlternateScreen,
            ratatui::crossterm::terminal::Clear(ratatui::crossterm::terminal::ClearType::All)
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
        EnableMouseCapture
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<Backend>) -> io::Result<()> {
    ratatui::crossterm::terminal::disable_raw_mode()?;
    ratatui::crossterm::execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        ratatui::crossterm::terminal::Clear(ratatui::crossterm::terminal::ClearType::All)
    )?;
    terminal.show_cursor()?;
    Ok(())
}
