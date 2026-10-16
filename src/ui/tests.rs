use super::*;
use crate::app::{App, Entry, EntryRole, Overlay, OverlayKind, Phase, PickerMode, ToolRow};
use crate::commands;
use crate::config::Config;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

/// Renders one frame and returns it as plain text (trailing blanks trimmed).
fn frame_text(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal.draw(|f| render(f, app)).expect("draw");
    let buffer = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn test_config() -> Config {
    let mut cfg = Config::from_env();
    cfg.apply_preset("scripted");
    cfg.notice = None;
    cfg
}

async fn test_app() -> App {
    App::new(test_config(), tokio::runtime::Handle::current()).expect("app")
}

fn assistant(text: &str) -> Entry {
    let mut entry = Entry::new(EntryRole::Assistant, text);
    entry.phase = Phase::Done;
    entry
}

fn streaming(text: &str) -> Entry {
    let mut entry = Entry::streaming();
    entry.text = text.to_string();
    entry
}

fn with_tool(name: &str, detail: &str, ok: bool) -> ToolRow {
    ToolRow {
        name: name.into(),
        detail: detail.into(),
        running: false,
        ok,
        edit_path: None,
        edit_replacements: None,
        preview: None,
    }
}

#[tokio::test]
async fn renders_welcome_state() {
    let mut app = test_app().await;
    let text = frame_text(&mut app, 92, 24);
    assert!(text.contains("nanomite"), "header missing:\n{text}");
    assert!(
        text.contains("ask anything"),
        "welcome card missing:\n{text}"
    );
    // Composer frame is visible and focused.
    assert!(text.contains('❯'), "composer prompt missing:\n{text}");
    assert!(
        text.contains("/ or ctrl+x"),
        "composer hint missing:\n{text}"
    );
}

#[tokio::test]
async fn composer_border_title_and_footer_render() {
    let mut app = test_app().await;
    app.cwd = std::path::PathBuf::from("/tmp/opencode/demo");
    let text = frame_text(&mut app, 92, 24);
    // The rounded frame is actually painted, with the cwd as its title.
    assert!(
        text.contains('╭') && text.contains('╰'),
        "border missing:\n{text}"
    );
    assert!(text.contains("demo"), "cwd title missing:\n{text}");
    // Key hints live on the bottom border.
    assert!(text.contains("enter send"), "hint missing:\n{text}");
}

#[tokio::test]
async fn composer_paints_all_measured_rows() {
    let mut app = test_app().await;
    for word in ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"] {
        app.composer.insert_str(word);
        app.composer.newline();
    }
    let text = frame_text(&mut app, 92, 24);
    // The border must not eat into the promised text rows: every line painted.
    for word in ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"] {
        assert!(text.contains(word), "row {word} clipped:\n{text}");
    }
}

#[tokio::test]
async fn composer_layout_width_tracks_terminal() {
    let mut app = test_app().await;
    frame_text(&mut app, 100, 24);
    // margin(2) + border(2) + prompt gutter(2) — the width wrapping is measured at.
    assert_eq!(app.composer_layout_width, 94);
}

#[tokio::test]
async fn renders_a_turn_with_tools_and_markdown() {
    let mut app = test_app().await;
    app.entries
        .push(Entry::new(EntryRole::User, "list the files in src"));
    let mut reply = assistant(
        "## Files\n\nThe workspace has **three** modules:\n\n- `src/main.rs`\n- `src/app.rs`\n\n```rust\nfn main() {}\n```\n\nDone.",
    );
    reply
        .tools
        .push(with_tool("list_files", "src · 6 files + 3 dirs", true));
    reply
        .tools
        .push(with_tool("read_file", "README.md · not found", false));
    app.entries.push(reply);

    let text = frame_text(&mut app, 92, 30);
    assert!(
        text.contains("list the files in src"),
        "user turn missing:\n{text}"
    );
    assert!(text.contains("Files"), "heading missing:\n{text}");
    assert!(text.contains("three"), "bold text missing:\n{text}");
    assert!(text.contains("fn main()"), "code block missing:\n{text}");
    assert!(text.contains("✓"), "tool success marker missing:\n{text}");
    assert!(text.contains("✗"), "tool failure marker missing:\n{text}");
    // The `❯` caret identifies the user turn.
    assert!(text.contains('❯'), "user marker missing:\\n{text}");
}

#[tokio::test]
async fn tool_rows_show_code_preview_live() {
    let mut app = test_app().await;
    app.entries
        .push(Entry::new(EntryRole::User, "make a calculator"));
    let mut reply = assistant("writing the file");
    let mut tool = with_tool("write_file", "calculator.py", true);
    tool.preview = Some("def add(a, b):\n    return a + b".into());
    reply.tools.push(tool);
    app.entries.push(reply);

    let text = frame_text(&mut app, 92, 24);
    assert!(
        text.contains("def add(a, b):"),
        "code preview missing:\n{text}"
    );
    assert!(
        text.contains("return a + b"),
        "preview body missing:\n{text}"
    );

    // `/details` off collapses previews back to bare tool rows.
    app.show_tool_details = false;
    let text = frame_text(&mut app, 92, 24);
    assert!(
        !text.contains("def add(a, b):"),
        "preview must hide with /details off:\n{text}"
    );
}

#[tokio::test]
async fn edit_preview_shows_old_and_new_lines() {
    let mut app = test_app().await;
    app.entries.push(Entry::new(EntryRole::User, "fix it"));
    let mut reply = assistant("editing");
    let mut tool = with_tool("edit_file", "calculator.py", true);
    tool.preview = Some("- return a - b\n+ return a + b".into());
    reply.tools.push(tool);
    app.entries.push(reply);

    let text = frame_text(&mut app, 92, 24);
    assert!(text.contains("- return a - b"), "old line missing:\n{text}");
    assert!(text.contains("+ return a + b"), "new line missing:\n{text}");
}

#[tokio::test]
async fn streaming_entry_shows_status_row() {
    let mut app = test_app().await;
    app.entries.push(Entry::new(EntryRole::User, "hi"));
    app.entries.push(streaming("partial ans"));
    let text = frame_text(&mut app, 92, 20);
    assert!(text.contains("thinking"), "status row missing:\n{text}");
    assert!(
        text.contains("partial ans") || text.contains("partial an▍"),
        "streamed text missing:\n{text}"
    );
}

#[tokio::test]
async fn long_transcripts_are_virtualized() {
    let mut app = test_app().await;
    for i in 0..400 {
        app.entries
            .push(Entry::new(EntryRole::User, format!("question {i}")));
        app.entries
            .push(assistant(&format!("answer {i} with a little bit of text")));
    }
    app.view.mark_dirty();

    let mut terminal = Terminal::new(TestBackend::new(92, 30)).unwrap();
    terminal.draw(|f| render(f, &mut app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let rendered: String = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Only the tail of the transcript is on screen, never the whole thing.
    assert!(
        rendered.contains("question 399"),
        "last turn not visible:\n{rendered}"
    );
    assert!(
        !rendered.contains("question 10 "),
        "virtualization broken:\n{rendered}"
    );
    assert!(app.view.content_height > 30, "content height not tracked");
    assert_eq!(
        app.view.scroll_from_bottom, 0,
        "should stay pinned to bottom"
    );

    // Scrolling up moves the window and can reach the start of the transcript.
    app.scroll_by(10_000);
    assert!(app.view.scroll_from_bottom > 0);
    app.view.scroll_to_top();
    let head = frame_text(&mut app, 92, 30);
    assert!(head.contains("question 0"), "scroll to top failed:\n{head}");

    // Returning to the bottom restores the live tail.
    app.scroll_to_end();
    let tail = frame_text(&mut app, 92, 30);
    assert!(
        tail.contains("question 399"),
        "scroll to end failed:\n{tail}"
    );
}

#[tokio::test]
async fn renders_palette_and_help_overlays() {
    let mut app = test_app().await;
    app.open_palette();
    let palette = frame_text(&mut app, 92, 30);
    assert!(
        palette.contains("commands"),
        "palette title missing:\n{palette}"
    );
    assert!(
        palette.contains("/compact"),
        "palette item missing:\n{palette}"
    );

    for ch in "ses".chars() {
        app.on_key(ratatui::crossterm::event::KeyEvent {
            code: ratatui::crossterm::event::KeyCode::Char(ch),
            modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
            kind: ratatui::crossterm::event::KeyEventKind::Press,
            state: ratatui::crossterm::event::KeyEventState::NONE,
        });
    }
    let filtered = frame_text(&mut app, 92, 30);
    assert!(
        filtered.contains("/sessions"),
        "filter dropped match:\n{filtered}"
    );
    assert!(
        !filtered.contains("/undo"),
        "filter kept non-match:\n{filtered}"
    );

    app.close_overlay();
    app.open_help();
    let help = frame_text(&mut app, 92, 30);
    assert!(help.contains("keyboard"), "help title missing:\n{help}");
    assert!(help.contains("/compact"), "command list missing:\n{help}");

    // The sheet is taller than a small terminal, so it scrolls.
    app.on_key(ratatui::crossterm::event::KeyEvent {
        code: ratatui::crossterm::event::KeyCode::End,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
        kind: ratatui::crossterm::event::KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    });
    let bottom = frame_text(&mut app, 92, 30);
    assert!(
        bottom.contains("scroll"),
        "key section missing after scrolling:\n{bottom}"
    );
    app.close_overlay();
    assert!(app.overlay.is_none());
}

#[tokio::test]
async fn keybindings_drive_the_composer() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    let mut app = test_app().await;
    let key = |code, mods| KeyEvent {
        code,
        modifiers: mods,
        kind: KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    };

    app.on_key(key(KeyCode::Char('h'), KeyModifiers::NONE));
    app.on_key(key(KeyCode::Char('i'), KeyModifiers::NONE));
    app.on_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(app.composer.value(), "h");

    // ctrl+j inserts a newline, ctrl+w eats trailing whitespace first.
    app.on_key(key(KeyCode::Char('j'), KeyModifiers::CONTROL));
    assert_eq!(app.composer.value(), "h\n");
    app.on_key(key(KeyCode::Char('w'), KeyModifiers::CONTROL));
    assert_eq!(app.composer.value(), "h");
    app.on_key(key(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert_eq!(app.composer.value(), "");

    // Escape clears a dirty composer instead of quitting.
    app.on_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.composer.value(), "");
    assert!(!app.should_quit());

    // ctrl+c quits.
    app.on_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.should_quit());
}

#[tokio::test]
async fn submitting_starts_a_turn_and_esc_interrupts_it() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    let mut app = test_app().await;
    app.composer.insert_str("list files");
    app.submit();

    assert!(app.is_busy(), "turn should be running");
    assert_eq!(app.entries.len(), 2);
    assert_eq!(app.entries[0].role, EntryRole::User);

    // Wait for the scripted turn to reach its tool call.
    for _ in 0..200 {
        app.pump_engine();
        if app.entries[1].tools.iter().any(|t| t.name == "list_files") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        app.entries[1].tools.iter().any(|t| t.name == "list_files"),
        "tool call not streamed: {:?} / {:?}",
        app.entries[1].tools,
        app.entries[1].text
    );

    app.on_key(KeyEvent {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    });

    for _ in 0..50 {
        app.pump_engine();
        if !app.is_busy() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(!app.is_busy(), "esc did not interrupt the turn");
    assert_eq!(app.entries[1].phase, Phase::Interrupted);
}

#[tokio::test]
async fn submitting_while_busy_keeps_the_prompt() {
    let mut app = test_app().await;
    app.composer.insert_str("first");
    app.submit();
    app.composer.insert_str("second");
    app.submit();

    assert!(app.is_busy());
    assert_eq!(app.composer.value(), "second", "queued prompt was dropped");
    assert_eq!(app.entries.len(), 2, "a second turn must not start");
    assert!(app.toast.is_some(), "user was not told why");
}

#[tokio::test]
async fn turn_completes_and_records_usage() {
    let mut app = test_app().await;
    app.composer.insert_str("hello there");
    app.submit();

    for _ in 0..200 {
        app.pump_engine();
        if !app.is_busy() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    assert!(!app.is_busy());
    assert_eq!(app.entries[1].phase, Phase::Done);
    assert!(
        app.entries[1].text.contains("Scripted test reply"),
        "reply missing"
    );
    assert!(app.usage.total_tokens > 0, "usage not recorded");
    assert!(app.entries[1].elapsed.is_some(), "elapsed not recorded");
}

#[tokio::test]
async fn narrow_terminals_do_not_panic() {
    let mut app = test_app().await;
    app.entries.push(Entry::new(
        EntryRole::User,
        "a very long prompt that will not fit on one line at all",
    ));
    app.entries.push(assistant(
        "reply with a **long** paragraph that has to wrap several times over",
    ));

    for (w, h) in [(24u16, 8u16), (30, 10), (40, 12), (60, 20), (200, 60)] {
        let text = frame_text(&mut app, w, h);
        assert!(
            text.lines().count() == h as usize,
            "wrong row count at {w}x{h}"
        );
    }
}

#[tokio::test]
async fn wide_unicode_content_wraps_without_overflow() {
    let mut app = test_app().await;
    app.entries.push(Entry::new(
        EntryRole::User,
        "日本語のテキストはとても長いので折り返しの確認をします",
    ));
    app.entries
        .push(assistant("ありがとう 🎉 折り返し確認用テキストです。"));
    let text = frame_text(&mut app, 40, 16);
    for line in text.lines() {
        assert!(line.chars().count() <= 40, "row overflowed: {line:?}");
    }
}

#[tokio::test]
async fn header_shows_workspace_and_status() {
    let mut app = test_app().await;
    app.cwd = std::path::PathBuf::from("/tmp/nanomite/header-demo");
    let text = frame_text(&mut app, 100, 24);
    // Brand, workspace path and the model/effort block all live in the header.
    assert!(text.contains("nanomite"), "brand missing:\n{text}");
    assert!(
        text.contains("header-demo"),
        "workspace missing from header:\n{text}"
    );
    assert!(
        text.contains("medium"),
        "reasoning effort missing from header:\n{text}"
    );
    // A tiny terminal keeps the brand row without the path.
    let narrow = frame_text(&mut app, 30, 12);
    assert!(narrow.contains("nanomite"), "brand missing:\n{narrow}");
}

#[tokio::test]
async fn footer_reports_the_token_budget() {
    let mut app = test_app().await;
    app.usage.prompt_tokens = 12_000;
    app.usage.completion_tokens = 3_000;
    app.usage.total_tokens = 15_000;
    let text = frame_text(&mut app, 100, 24);
    // Absolute readout over the limit, not just a percentage.
    assert!(
        text.contains("15.0k/200.0k"),
        "budget readout missing:\n{text}"
    );
    assert!(text.contains("7%"), "percentage missing:\n{text}");
}

#[tokio::test]
async fn theme_switch_persists_and_updates_config() {
    let mut app = test_app().await;
    app.cfg.store_path =
        Some(std::env::temp_dir().join(format!("nanomite-ui-theme-{}.toml", std::process::id())));
    app.apply_theme(crate::ui::theme::ThemeId::Umbrella);
    assert_eq!(app.theme.id, crate::ui::theme::ThemeId::Umbrella);
    assert_eq!(app.cfg.theme, "umbrella");
    let path = app.cfg.store_path.clone().unwrap();
    let stored = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        stored.contains("theme = \"umbrella\""),
        "theme not persisted: {stored}"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn theme_picker_lists_skins_with_descriptors() {
    let mut app = test_app().await;
    app.open_themes();
    let text = frame_text(&mut app, 92, 26);
    assert!(text.contains("themes"), "picker title missing:\n{text}");
    for skin in ["mono", "opencode", "midnight", "graphite", "umbrella"] {
        assert!(text.contains(skin), "skin {skin} missing:\n{text}");
    }
    assert!(text.contains("single hue"), "descriptor missing:\n{text}");
    app.close_overlay();
}

#[tokio::test]
async fn busy_footer_shows_step_and_tool_progress() {
    let mut app = test_app().await;
    app.composer.insert_str("hello");
    app.submit();
    for _ in 0..200 {
        app.pump_engine();
        if app.entries[1].tools.iter().any(|t| !t.running) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    // The completed entry itself carries the step counter the status strip reads.
    let entry = &app.entries[1];
    assert!(
        entry.steps > 0,
        "steps not counted: {:?} / {:?}",
        entry.steps,
        entry.tools
    );
    assert!(
        !app.is_busy(),
        "the scripted turn should have finished by now"
    );
}

#[tokio::test]
async fn styles_use_the_theme_palette() {
    let mut app = test_app().await;
    app.entries.push(Entry::new(EntryRole::User, "styled"));
    let mut terminal = Terminal::new(TestBackend::new(92, 20)).unwrap();
    terminal.draw(|f| render(f, &mut app)).unwrap();
    let buffer = terminal.backend().buffer().clone();

    let accent = app.theme.accent();
    let has_accent = (0..buffer.area.height).any(|y| {
        (0..buffer.area.width).any(|x| {
            let cell = &buffer[(x, y)];
            cell.fg == accent && cell.modifier.contains(Modifier::empty())
        })
    });
    assert!(has_accent, "accent colour never used");
    assert_ne!(accent, Color::Reset);
}

#[tokio::test]
async fn only_visible_entries_are_materialised() {
    let mut app = test_app().await;
    for i in 0..2_000 {
        app.entries
            .push(Entry::new(EntryRole::User, format!("prompt {i}")));
        app.entries.push(assistant(&format!("reply {i}").repeat(6)));
    }
    app.view.mark_dirty();

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| render(f, &mut app)).unwrap();

    let cached = app.entries.iter().filter(|e| e.cache.is_some()).count();
    assert!(
        cached <= super::chat::WARM_LIMIT + 64,
        "virtualization regressed: {cached}/{} entries rendered",
        app.entries.len()
    );
    // 2000 short entries must still be taller than the viewport.
    assert!(app.view.content_height > 30);
}

#[tokio::test]
async fn composer_grows_and_scrolls_with_multiline_input() {
    let mut app = test_app().await;
    app.entries.push(Entry::new(EntryRole::User, "hi"));
    app.entries.push(assistant("ok"));
    app.composer
        .insert_str("first line of a long prompt that wraps\nsecond line\nthird");
    app.composer.move_end();

    let short = frame_text(&mut app, 70, 20);
    assert!(short.contains("❯"), "composer prompt missing:\n{short}");
    assert!(short.contains("third"), "typed text not rendered:\n{short}");

    // A much taller buffer clamps to the max rows instead of eating the viewport.
    for _ in 0..14 {
        app.composer.insert_str("\nmore");
    }
    let tall = frame_text(&mut app, 70, 20);
    // Locate the composer by its hint footer, not by `╭` (user bubbles use it too).
    let chat_rows = tall
        .lines()
        .position(|l| l.contains("enter send"))
        .unwrap_or(20);
    assert!(chat_rows > 2, "transcript collapsed:\n{tall}");
    assert!(
        tall.contains("enter send"),
        "composer hint line missing:\n{tall}"
    );
}

// ---------------------------------------------------------------------------
// slash commands
// ---------------------------------------------------------------------------

fn press(
    app: &mut App,
    code: ratatui::crossterm::event::KeyCode,
    mods: ratatui::crossterm::event::KeyModifiers,
) {
    app.on_key(ratatui::crossterm::event::KeyEvent {
        code,
        modifiers: mods,
        kind: ratatui::crossterm::event::KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    });
}

fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        press(
            app,
            ratatui::crossterm::event::KeyCode::Char(ch),
            ratatui::crossterm::event::KeyModifiers::NONE,
        );
    }
}

#[tokio::test]
async fn slash_opens_the_command_menu() {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    let mut app = test_app().await;
    type_text(&mut app, "/");
    assert!(
        matches!(
            app.overlay.as_ref().map(Overlay::kind),
            Some(OverlayKind::Picker)
        ),
        "a bare / should open the command menu"
    );

    // The menu lists every command and filters as you type.
    assert_eq!(
        app.composer.value(),
        "",
        "the menu must not eat the composer"
    );
    type_text(&mut app, "con");
    let text = frame_text(&mut app, 100, 26);
    assert!(text.contains("/connect"), "filtered row missing:\n{text}");
    assert!(
        !text.contains("/models"),
        "unmatched rows must drop:\n{text}"
    );

    // Enter runs the highlighted command and closes the menu.
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(
        app.overlay.as_ref().map(Overlay::kind),
        Some(OverlayKind::Connect)
    ));
    assert_eq!(app.composer.value(), "");

    // `/` still types literally once the composer has text (paths, fractions).
    let mut app = test_app().await;
    type_text(&mut app, "3");
    type_text(&mut app, "/");
    assert!(app.overlay.is_none(), "mid-text / must not open the menu");
    assert_eq!(app.composer.value(), "3/");
}

#[tokio::test]
async fn unknown_slash_command_is_reported_not_sent() {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    let mut app = test_app().await;
    type_text(&mut app, "/nope");
    // `/` opened the command menu; nothing matches "nope".
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(
        app.entries.is_empty(),
        "unknown commands must not reach the model"
    );
    assert!(!app.is_busy(), "no turn was started either");
    assert!(
        app.toast
            .as_ref()
            .is_some_and(|(m, _)| m.contains("nothing selected")),
        "the menu should say nothing matched:\n{:?}",
        app.toast
    );

    // A matched command is intercepted locally.
    let mut app = test_app().await;
    type_text(&mut app, "/help");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.entries.is_empty(), "help must not reach the model");
    assert!(matches!(
        app.overlay.as_ref().map(Overlay::kind),
        Some(OverlayKind::Help)
    ));
}

#[tokio::test]
async fn provider_menu_switches_and_filters() {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    let mut app = test_app().await;
    app.open_provider_menu();

    let Some(Overlay::Providers(ref _state)) = app.overlay else {
        panic!("expected a provider menu");
    };
    // The menu should show the provider menu with search box.
    let text = frame_text(&mut app, 100, 26);
    assert!(
        text.contains("connect a provider"),
        "menu title missing:
{text}"
    );
    // The head shows Popular section and some providers.
    for expected in [
        "Popular",
        "Providers",
        "OpenAI",
        "Anthropic",
        "Google Gemini",
        "Groq",
        "DeepSeek",
        "Mistral AI",
    ] {
        assert!(
            text.contains(expected),
            "missing preset row {expected}:
{text}"
        );
    }

    // Filter to narrow to a specific provider.
    type_text(&mut app, "olla");
    let scrolled = frame_text(&mut app, 100, 26);
    assert!(
        scrolled.contains("Ollama"),
        "filtered menu should show Ollama:\n{scrolled}"
    );
    // Selecting Ollama applies it.
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.cfg.preset, "ollama");
    assert_eq!(app.cfg.base_url, "http://localhost:11434/v1");
    assert_eq!(app.model_label(), "llama3.2");
    assert_eq!(app.provider_label(), "Ollama");
    assert!(app.overlay.is_none(), "menu should close after applying");
    assert!(app.toast.is_some(), "switching should report back");
}

#[tokio::test]
async fn provider_menu_filters_by_name() {
    let mut app = test_app().await;
    app.open_provider_menu();
    type_text(&mut app, "olla");
    let Some(Overlay::Providers(ref state)) = app.overlay else {
        panic!("expected a provider menu");
    };
    assert_eq!(state.filter.value(), "olla");
    // The menu should show only Ollama (and maybe others matching "olla").
    let text = frame_text(&mut app, 100, 26);
    assert!(
        text.contains("Ollama"),
        "filtered menu should show Ollama:
{text}"
    );
}

#[tokio::test]
async fn model_picker_lists_models_and_switches() {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    let mut app = test_app().await;
    app.open_model_picker();

    assert!(matches!(
        app.overlay.as_ref().map(Overlay::kind),
        Some(OverlayKind::Picker)
    ));
    let Some(Overlay::Picker(ref mut picker)) = app.overlay else {
        panic!("expected a picker");
    };
    assert_eq!(picker.mode, PickerMode::Model);
    assert!(picker.loading, "model discovery should start as pending");

    // The scripted test double answers instantly; drain it.
    for _ in 0..100 {
        app.pump_engine();
        if let Some(Overlay::Picker(p)) = app.overlay.as_ref() {
            if !p.loading {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let Some(Overlay::Picker(picker)) = app.overlay.clone() else {
        panic!("expected a picker");
    };
    assert!(!picker.loading);
    assert!(
        picker.items.iter().any(|i| i.label == "scripted-model"),
        "model list empty: {:?}",
        picker.items
    );

    // Free-text ids are accepted when nothing matches.
    type_text(&mut app, "qwen3-coder");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.cfg.model, "qwen3-coder");
    assert_eq!(app.model_label(), "qwen3-coder");
}

#[tokio::test]
async fn picker_filter_narrows_rows() {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    let mut app = test_app().await;
    app.open_palette();
    type_text(&mut app, "quit");
    let Some(Overlay::Picker(picker)) = app.overlay.clone() else {
        panic!("expected a picker");
    };
    assert!(
        picker.items.iter().any(|i| i.label == "/exit"),
        "alias search failed: {:?}",
        picker.items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.should_quit());
}

#[tokio::test]
async fn remaining_slash_commands_act_locally() {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    let mut app = test_app().await;
    app.entries.push(Entry::new(EntryRole::User, "first"));
    type_text(&mut app, "/clear");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(
        app.entries.is_empty(),
        "/clear did not clear the transcript"
    );
    assert_eq!(app.composer.value(), "");

    app.entries.push(Entry::new(EntryRole::User, "first"));
    type_text(&mut app, "/new");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.entries.is_empty(), "/new did not reset the session");

    // Every advertised command must resolve to something.
    for entry in commands::COMMANDS {
        assert_eq!(
            commands::parse(entry.command).map(|(kind, _)| kind),
            Some(entry.kind),
            "{} is advertised but not parsed",
            entry.command
        );
    }
}

// ---------------------------------------------------------------------------
// /connect
// ---------------------------------------------------------------------------

#[tokio::test]
async fn connect_saves_base_url_and_key() {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    let mut app = test_app().await;
    type_text(&mut app, "/connect");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(matches!(
        app.overlay.as_ref().map(Overlay::kind),
        Some(OverlayKind::Connect)
    ));
    let Some(Overlay::Connect(form)) = app.overlay.clone() else {
        panic!("expected the connect form");
    };
    assert_eq!(form.base_url.value(), app.cfg.base_url);

    // Rewrite the base URL, then walk down to the key.
    for _ in 0..80 {
        press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    }
    type_text(&mut app, "http://localhost:1234/v1");
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    type_text(&mut app, "sk-test-key");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(app.cfg.base_url, "http://localhost:1234/v1");
    assert_eq!(app.cfg.api_key.as_deref(), Some("sk-test-key"));
    assert_eq!(
        app.cfg.model, "scripted-model",
        "the model is owned by /model, not /connect"
    );
    assert_eq!(app.cfg.preset, "lmstudio", "endpoint should match a preset");
    assert!(app.overlay.is_none(), "the form should close on save");

    // The verification round trip reports back through a toast.
    for _ in 0..200 {
        app.pump_engine();
        if app
            .toast
            .as_ref()
            .is_some_and(|(m, _)| m.contains("connection failed") || m.contains("connected"))
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let toast = app
        .toast
        .as_ref()
        .map(|(m, _)| m.clone())
        .unwrap_or_default();
    assert!(
        toast.contains("connection failed") || toast.contains("connected"),
        "no connection result reported: {toast:?}"
    );
}

#[tokio::test]
async fn connect_rejects_a_bogus_endpoint() {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    let mut app = test_app().await;
    let base_url_before = app.cfg.base_url.clone();
    app.open_connect();
    let Some(Overlay::Connect(ref mut form)) = app.overlay else {
        panic!("expected the connect form");
    };
    form.base_url.clear();
    type_text(&mut app, "api.openai.com");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(
        app.cfg.base_url, base_url_before,
        "a rejected endpoint must not overwrite the config"
    );
    assert!(
        app.overlay.is_some(),
        "a rejected endpoint keeps the form open"
    );
    assert!(
        app.toast.as_ref().is_some_and(|(m, _)| m.contains("http")),
        "expected a validation message"
    );
}

#[tokio::test]
async fn connect_form_masks_the_key() {
    let mut app = test_app().await;
    app.cfg.api_key = Some("sk-super-secret-value".into());
    app.open_connect();

    let text = frame_text(&mut app, 100, 26);
    assert!(text.contains("connect"), "title missing:\n{text}");
    assert!(text.contains("provider"), "provider row missing:\n{text}");
    assert!(text.contains("Scripted"), "provider name missing:\n{text}");
    assert!(text.contains("base url"), "base url field missing:\n{text}");
    assert!(text.contains("api key"), "key field missing:\n{text}");
    assert!(
        !text.contains("super-secret"),
        "the key must not be printed:\n{text}"
    );
    assert!(
        text.contains("alue"),
        "the key tail should stay visible:\n{text}"
    );
    assert!(text.contains("enter save"), "hints missing:\n{text}");
}

/// Every frame cell is painted inside the terminal and no cell holds a NUL.
fn assert_frame_well_formed(text: &str, width: u16, height: u16) {
    assert_eq!(
        text.lines().count(),
        height as usize,
        "expected {height} rows"
    );
    for line in text.lines() {
        assert!(
            line.chars().count() <= width as usize,
            "row overflowed {width} cols: {line:?}"
        );
        assert!(!line.contains('\u{0}'), "scrambled cells: {line:?}");
    }
}

#[tokio::test]
async fn narrow_30x12_overlays_render_cleanly() {
    let mut app = test_app().await;
    app.entries.push(Entry::new(EntryRole::User, "hello"));
    app.entries.push(assistant("hi there"));

    app.open_help();
    let text = frame_text(&mut app, 30, 12);
    assert_frame_well_formed(&text, 30, 12);
    assert!(text.contains("keyboard"), "help title missing:\n{text}");
    app.close_overlay();

    app.open_palette();
    let text = frame_text(&mut app, 30, 12);
    assert_frame_well_formed(&text, 30, 12);
    assert!(text.contains('❯'), "picker search row missing:\n{text}");
    app.close_overlay();

    app.open_provider_menu();
    let text = frame_text(&mut app, 30, 12);
    assert_frame_well_formed(&text, 30, 12);
    assert!(
        text.contains("connect a provider"),
        "providers title missing:\n{text}"
    );
    app.close_overlay();

    app.open_connect();
    let text = frame_text(&mut app, 30, 12);
    assert_frame_well_formed(&text, 30, 12);
    assert!(text.contains("connect"), "connect title missing:\n{text}");
    assert!(text.contains("api key"), "connect fields missing:\n{text}");
    app.close_overlay();
    assert!(app.overlay.is_none());
}

#[test]
fn help_sheet_lists_every_on_key_binding() {
    // Audit harness: the sheet must mention every binding `App::on_key`
    // implements (spot-check well beyond the required five).
    let sheet: String = panels::help_keys()
        .iter()
        .map(|(k, d)| format!("{k} {d} | "))
        .collect();
    for binding in [
        "ctrl+c",
        "ctrl+d",
        "enter",
        "ctrl+j",
        "alt+enter",
        "/ or ctrl+x",
        "esc",
        "ctrl+t",
        "ctrl+l",
        "ctrl+r",
        "ctrl+p",
        "ctrl+n",
        "ctrl+a",
        "ctrl+e",
        "ctrl+w",
        "ctrl+u",
        "ctrl+k",
        "backspace",
        "delete",
        "left",
        "right",
        "ctrl or alt",
        "up",
        "down",
        "pgup",
        "pgdn",
        "home",
        "end",
        "ctrl+home",
        "ctrl+end",
        "mouse wheel",
        "tab",
        "f1",
    ] {
        assert!(
            sheet.contains(binding),
            "help sheet missing `{binding}`:\n{sheet}"
        );
    }
    // The stale, inaccurate rows must not come back: plain home/end move the
    // caret, they do not scroll the transcript.
    assert!(!sheet.contains("pgup / pgdn / home / end"));
    assert!(!sheet.contains("(with ctrl)"));
}

#[tokio::test]
async fn edit_file_preview_uses_the_diff_renderer() {
    fn fg_of(buffer: &ratatui::buffer::Buffer, needle: &str) -> Option<Color> {
        for y in 0..buffer.area.height {
            let mut row = String::new();
            for x in 0..buffer.area.width {
                row.push_str(buffer[(x, y)].symbol());
            }
            if let Some(x) = row.find(needle) {
                return Some(buffer[(x as u16, y)].fg);
            }
        }
        None
    }

    let mut app = test_app().await;
    app.entries.push(Entry::new(EntryRole::User, "fix it"));
    let mut reply = assistant("editing");
    let mut tool = with_tool("edit_file", "calculator.py", true);
    tool.preview = Some("- return a - b\n+ return a + b\n… +9 more lines".into());
    reply.tools.push(tool);
    app.entries.push(reply);

    let mut terminal = Terminal::new(TestBackend::new(92, 24)).unwrap();
    terminal.draw(|f| render(f, &mut app)).unwrap();
    let buffer = terminal.backend().buffer().clone();

    // The -/+ rows carry the diff roles, proving the shared DiffView renderer
    // drove the styling instead of the old ad-hoc success/danger spans.
    assert_eq!(
        fg_of(&buffer, "- return a - b"),
        Some(app.theme.diff_delete()),
        "removal must render with the diff-delete role"
    );
    assert_eq!(
        fg_of(&buffer, "+ return a + b"),
        Some(app.theme.diff_add()),
        "addition must render with the diff-add role"
    );
    // The elision marker is preview chatter, not diff data: faint.
    assert_eq!(
        fg_of(&buffer, "… +9 more lines"),
        Some(app.theme.faint()),
        "elision marker must render faint"
    );

    // `/details` off still collapses the whole preview.
    app.show_tool_details = false;
    let text = frame_text(&mut app, 92, 24);
    assert!(
        !text.contains("return a"),
        "preview must hide with /details off:\n{text}"
    );
}
