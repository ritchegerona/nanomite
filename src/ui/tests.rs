use super::*;
use crate::app::{
    App, Entry, EntryRole, Overlay, OverlayKind, Phase, PickerMode, SudoPasswordState,
    SuggestionStrip, ToolRow,
};
use crate::commands;
use crate::commands::Command;
use crate::config::Config;
use crate::improve::testing::{temp_dir, TempDir};
use crate::improve::{FakeRunner, ImproveEngine, Roots};
use crate::ui::theme::glyph;
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
        started: std::time::Instant::now(),
        elapsed: None,
    }
}

#[tokio::test]
async fn renders_welcome_state() {
    let mut app = test_app().await;
    let text = frame_text(&mut app, 92, 24);
    assert!(text.contains("nanomite"), "header missing:\n{text}");
    assert!(
        text.contains("ask anything to begin"),
        "welcome card missing:\n{text}"
    );
    // Composer frame is visible and focused.
    assert!(text.contains('❯'), "composer prompt missing:\n{text}");
    assert!(text.contains("ctrl+x"), "composer hint missing:\n{text}");
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
    // opencode-style rows: the read row carries the `→` glyph, the failure
    // goes red with `✗`.
    assert!(text.contains("→"), "tool read marker missing:\n{text}");
    assert!(text.contains("✗"), "tool failure marker missing:\n{text}");
    // The user turn is a left-bar block, like opencode's coloured prompt.
    assert!(text.contains('▌'), "user bar missing:\n{text}");
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
async fn user_message_renders_as_a_raised_block() {
    let mut app = test_app().await;
    let mut user = Entry::new(EntryRole::User, "list the files");
    user.at = std::time::SystemTime::now();
    app.entries.push(user);

    let text = frame_text(&mut app, 92, 24);
    assert!(text.contains("list the files"), "text missing:\n{text}");
    assert!(text.contains('▌'), "left bar missing:\n{text}");
    // opencode drops the YOU/AI badges and timestamps entirely.
    assert!(!text.contains("YOU"), "badge should be gone:\n{text}");
}

#[tokio::test]
async fn assistant_footer_shows_mode_duration_and_rate() {
    let mut app = test_app().await;
    let mut entry = assistant("done work");
    entry.elapsed = Some(std::time::Duration::from_secs(5));
    entry.usage = Some(crate::agent::provider::Usage {
        prompt_tokens: 10,
        completion_tokens: 50,
        total_tokens: 60,
    });
    app.entries.push(entry);

    let text = frame_text(&mut app, 92, 24);
    // opencode's message footer: agent/mode name, then duration and tok/s.
    assert!(text.contains("Build"), "mode label missing:\n{text}");
    assert!(text.contains("5.0s"), "duration missing:\n{text}");
    assert!(text.contains("10.0 tok/s"), "token rate missing:\n{text}");
}

#[tokio::test]
async fn streaming_cursor_keeps_the_last_char() {
    let mut app = test_app().await;
    app.entries.push(streaming("xyzzy"));
    let text = frame_text(&mut app, 92, 20);
    // The caret is appended, so the streamed text survives intact; the old
    // pop-a-character cursor would have rendered `xyzz▍`.
    assert!(text.contains("xyzzy"), "cursor ate the last char:\n{text}");
}

#[tokio::test]
async fn tool_pills_show_their_own_duration() {
    let mut app = test_app().await;
    let mut entry = assistant("edited the file");
    let mut tool = with_tool("edit_file", "calculator.py", true);
    tool.started = std::time::Instant::now();
    tool.elapsed = Some(std::time::Duration::from_secs(5));
    entry.tools.push(tool);
    // The turn total must not leak into the pill.
    entry.elapsed = None;
    app.entries.push(entry);

    let text = frame_text(&mut app, 92, 24);
    assert!(text.contains("5.0s"), "per-tool duration missing:\n{text}");
}

#[tokio::test]
async fn transcript_search_finds_jumps_and_closes() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    let mut app = test_app().await;
    app.entries
        .push(Entry::new(EntryRole::User, "the needle is here"));
    for i in 0..40 {
        app.entries
            .push(Entry::new(EntryRole::User, format!("filler row {i}")));
    }
    // Paint once so the viewport offsets exist for the jump math.
    let _ = frame_text(&mut app, 92, 24);
    app.scroll_to_end();

    let key = |code, mods| KeyEvent {
        code,
        modifiers: mods,
        kind: KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    };

    // ctrl+f opens the bar; typing goes to the query, never the composer.
    app.on_key(key(KeyCode::Char('f'), KeyModifiers::CONTROL));
    assert!(app.search.is_some(), "ctrl+f did not open search");
    for ch in "needle".chars() {
        app.on_key(key(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    assert_eq!(app.composer.value(), "", "search leaked into the composer");
    let state = app.search.as_ref().expect("search open");
    assert_eq!(state.matches, vec![0], "match list wrong");

    let text = frame_text(&mut app, 92, 24);
    assert!(text.contains("/needle"), "search bar missing:\n{text}");
    assert!(text.contains("1/1 matches"), "match count missing:\n{text}");

    // Enter jumps to the only match, which sits far above the viewport.
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        app.view.scroll_from_bottom > 0,
        "enter did not jump to the match"
    );

    // esc closes the bar; the composer was never touched.
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.search.is_none(), "esc did not close search");
    assert_eq!(app.composer.value(), "");

    // Typing lands in the composer again once the bar is closed.
    app.on_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
    assert_eq!(app.composer.value(), "x");
}

#[tokio::test]
async fn search_bar_reports_a_miss() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    let mut app = test_app().await;
    app.entries
        .push(Entry::new(EntryRole::User, "nothing here"));

    let key = |code, mods| KeyEvent {
        code,
        modifiers: mods,
        kind: KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    };

    app.on_key(key(KeyCode::Char('f'), KeyModifiers::CONTROL));
    for ch in "zzzz".chars() {
        app.on_key(key(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let text = frame_text(&mut app, 92, 24);
    assert!(text.contains("/zzzz"), "query missing:\n{text}");
    assert!(text.contains("no matches"), "miss not reported:\n{text}");
}

#[tokio::test]
async fn scrollbar_click_jumps_and_bottom_follows() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut app = test_app().await;
    for i in 0..40 {
        app.entries
            .push(Entry::new(EntryRole::User, format!("question {i}")));
    }
    // Paint once so the layout rects and offsets exist.
    let _ = frame_text(&mut app, 92, 24);
    let rect = app.view.cached_chat_rect().expect("chat rect cached");
    let sb_col = rect.x + rect.width - 1;
    let mouse = |kind, column, row| MouseEvent {
        kind,
        column,
        row,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
    };

    // The middle of the track jumps away from the tail.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        sb_col,
        rect.y + rect.height / 2,
    ));
    assert!(
        app.view.scroll_from_bottom > 0,
        "scrollbar click did not jump"
    );

    // The bottom edge re-follows the tail, as the footer promises.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        sb_col,
        rect.y + rect.height - 1,
    ));
    assert_eq!(
        app.view.scroll_from_bottom, 0,
        "bottom click did not follow"
    );
    assert!(app.view.follow);

    // A press on the track never starts a text selection.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        sb_col,
        rect.y + 2,
    ));
    app.on_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        sb_col,
        rect.y + 2,
    ));
    assert!(app.selection.is_none(), "scrollbar press made a selection");
}

#[tokio::test]
async fn scrolled_up_pill_counts_new_lines() {
    let mut app = test_app().await;
    for i in 0..40 {
        app.entries
            .push(Entry::new(EntryRole::User, format!("question {i}")));
    }
    // Paint once so the layout rects and offsets exist before scrolling.
    let _ = frame_text(&mut app, 92, 24);
    // Start a turn so the app is busy while the user scrolls away.
    app.composer.insert_str("go");
    app.submit();
    assert!(app.is_busy(), "turn should be running");

    app.scroll_by(10_000);
    assert!(!app.view.follow, "setup: should be scrolled away");

    // New content arrives while scrolled away; the rebuild counts it.
    app.entries
        .push(Entry::new(EntryRole::User, "late arrival"));
    let text = frame_text(&mut app, 92, 24);
    assert!(app.view.new_since_scroll > 0, "no new rows counted");
    assert!(text.contains("new lines"), "pill missing:\n{text}");
    assert!(text.contains("end to follow"), "pill hint missing:\n{text}");

    // Following the tail clears the counter and the pill.
    app.scroll_to_end();
    assert_eq!(app.view.new_since_scroll, 0, "counter did not reset");
    let text = frame_text(&mut app, 92, 24);
    assert!(
        !text.contains("end to follow"),
        "pill stayed visible:\n{text}"
    );
}

#[tokio::test]
async fn error_block_collapses_and_expands() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut app = test_app().await;
    // Long enough to wrap past the two collapsed lines, with a unique tail
    // word that can only be visible when expanded.
    let long_error = format!("{}zomega", "alpha beta gamma delta epsilon ".repeat(12));
    let mut entry = assistant("tried and failed");
    entry.phase = Phase::Failed;
    entry.error = Some(long_error.to_string());
    app.entries.push(entry);

    let text = frame_text(&mut app, 92, 24);
    assert!(text.contains("Error:"), "error missing:\n{text}");
    assert!(
        text.contains("click to expand"),
        "collapsed hint missing:\n{text}"
    );
    assert!(
        !text.contains("omega"),
        "full error should be collapsed:\n{text}"
    );

    // A plain click on the entry expands the full error, opencode-style.
    let rect = app.view.cached_chat_rect().expect("chat rect cached");
    let mouse = |kind, column, row| MouseEvent {
        kind,
        column,
        row,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
    };
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 8,
        rect.y + 2,
    ));
    app.on_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        rect.x + 8,
        rect.y + 2,
    ));
    assert!(app.entries[0].error_expanded, "click did not expand");

    let text = frame_text(&mut app, 92, 24);
    assert!(text.contains("omega"), "expanded error missing:\n{text}");
    assert!(text.contains("click to collapse"), "hint missing:\n{text}");
}

#[tokio::test]
async fn alt_arrows_jump_between_messages() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    let mut app = test_app().await;
    for i in 0..30 {
        app.entries
            .push(Entry::new(EntryRole::User, format!("question {i}")));
    }
    let _ = frame_text(&mut app, 92, 24);
    app.scroll_to_end();

    let key = |code, mods| KeyEvent {
        code,
        modifiers: mods,
        kind: KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    };

    app.on_key(key(KeyCode::Up, KeyModifiers::ALT));
    let at = app.view.scroll_from_bottom;
    assert!(at > 0, "alt+up did not jump");

    app.on_key(key(KeyCode::Up, KeyModifiers::ALT));
    assert!(
        app.view.scroll_from_bottom > at,
        "second alt+up did not move further"
    );

    app.on_key(key(KeyCode::Down, KeyModifiers::ALT));
    assert_eq!(
        app.view.scroll_from_bottom, at,
        "alt+down did not jump back"
    );
}

#[tokio::test]
async fn clicking_a_code_block_copies_it() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut app = test_app().await;
    app.entries.push(assistant(
        "here:\n\n```rust\nlet x = 1;\nlet y = 2;\n```\nafter",
    ));
    let _ = frame_text(&mut app, 92, 24);

    // The box's recorded region gives the exact row to click.
    let (block_row, source) = {
        let cached = app.entries[0].cache.as_ref().expect("cached render");
        let (s, _e, src) = cached.code_blocks[0].clone();
        (usize::from(s), src)
    };
    assert_eq!(source, "let x = 1;\nlet y = 2;");

    let rect = app.view.cached_chat_rect().expect("chat rect cached");
    let mouse = |kind, column, row| MouseEvent {
        kind,
        column,
        row,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
    };
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 8,
        rect.y + block_row as u16,
    ));
    app.on_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        rect.x + 8,
        rect.y + block_row as u16,
    ));

    let (msg, _) = app.toast.clone().expect("toast after click");
    assert!(
        msg.contains("code block copied (2 lines)"),
        "unexpected toast: {msg}"
    );
}

#[tokio::test]
async fn mouse_selection_lands_on_the_clicked_character() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut app = test_app().await;
    app.entries.push(Entry::new(EntryRole::User, "hello world"));
    let _ = frame_text(&mut app, 92, 24);
    let rect = app.view.cached_chat_rect().expect("chat rect cached");
    let mouse = |kind, column, row| MouseEvent {
        kind,
        column,
        row,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
    };

    // Body row 1 of the user block renders `▌ hello world`; the 'w' of
    // "world" sits at display column 8. The old mapping subtracted a
    // hard-coded gutter, dragging every anchor 4 chars out of place.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 8,
        rect.y + 1,
    ));
    app.on_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        rect.x + 13,
        rect.y + 1,
    ));

    let sel = app.selection.as_ref().expect("selection active");
    assert_eq!(sel.start, (0, 1, 8), "anchor lands on the clicked char");
    assert_eq!(sel.end, (0, 1, 13));
    let text = app.get_selected_text(sel);
    assert_eq!(text, "world", "selection text matches the drag: {text:?}");

    app.on_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        rect.x + 13,
        rect.y + 1,
    ));
}

#[tokio::test]
async fn mouse_selection_snaps_inside_wide_glyphs() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut app = test_app().await;
    app.entries
        .push(Entry::new(EntryRole::User, "日本語テスト"));
    let _ = frame_text(&mut app, 92, 24);
    let rect = app.view.cached_chat_rect().expect("chat rect cached");
    let mouse = |kind, column, row| MouseEvent {
        kind,
        column,
        row,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
    };

    // Body row 1 renders `▌ 日本語テスト`: 日 occupies columns 2-3, 本 4-5,
    // 語 6-7, テ 8-9. A click inside a wide glyph snaps to that glyph.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 3,
        rect.y + 1,
    ));
    app.on_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        rect.x + 8,
        rect.y + 1,
    ));

    let sel = app.selection.as_ref().expect("selection active");
    assert_eq!(sel.start, (0, 1, 2), "click inside 日 snaps to 日");
    assert_eq!(sel.end, (0, 1, 5), "click inside テ snaps to テ");
    let text = app.get_selected_text(sel);
    assert_eq!(text, "日本語", "wide-glyph selection: {text:?}");
}

#[tokio::test]
async fn mouse_selection_survives_blank_gap_rows() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut app = test_app().await;
    app.entries.push(Entry::new(EntryRole::User, "first"));
    app.entries.push(Entry::new(EntryRole::User, "second"));
    let _ = frame_text(&mut app, 92, 24);
    let rect = app.view.cached_chat_rect().expect("chat rect cached");
    let mouse = |kind, column, row| MouseEvent {
        kind,
        column,
        row,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
    };

    // The user block is bar/text/bar (3 rows) + 1 separator row. A drag that
    // starts on the separator (row 4) must anchor at the entry's final line
    // instead of dying, and keep tracking into the next entry.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 4,
        rect.y + 3, // the gap row of entry 0
    ));
    app.on_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        rect.x + 4,
        rect.y + 6, // the text row of entry 1
    ));

    let sel = app.selection.as_ref().expect("selection active");
    // The gap click anchored at entry 0's final line and the drag kept
    // tracking into entry 1 — no dead zone in between.
    assert_eq!(
        sel.start.0, 0,
        "anchor on entry 0 (clamped to its last line)"
    );
    assert_eq!(sel.end.0, 1, "drag tracked into entry 1");
    let text = app.get_selected_text(sel);
    assert!(text.contains("second"), "kept tracking: {text:?}");
}

#[tokio::test]
async fn focus_mode_shows_only_the_latest_exchange() {
    let mut app = test_app().await;
    for i in 0..3 {
        app.entries
            .push(Entry::new(EntryRole::User, format!("question {i}")));
        app.entries
            .push(assistant(&format!("answer {i} with a little bit of text")));
    }
    let _ = frame_text(&mut app, 92, 40);
    assert!(frame_text(&mut app, 92, 40).contains("question 0"));

    app.toggle_focus();
    let text = frame_text(&mut app, 92, 40);
    assert!(text.contains("question 2"), "latest turn missing:\n{text}");
    assert!(
        !text.contains("question 0"),
        "older turns must hide in focus mode:\n{text}"
    );

    app.toggle_focus();
    assert!(frame_text(&mut app, 92, 40).contains("question 0"));
}

#[tokio::test]
async fn time_machine_records_and_reports_reverts() {
    let mut app = test_app().await;
    // The runtime posts a snapshot per mutating tool call.
    app.record_snapshot("src/lib.rs".into(), Some(b"old contents".to_vec()));
    app.record_snapshot("src/new.rs".into(), None);
    // Same-turn snapshots dedupe on the first content seen.
    app.record_snapshot("src/lib.rs".into(), Some(b"other".to_vec()));
    assert_eq!(app.snapshots.len(), 1);

    app.on_revert_done(1, 2, 0);
    let last = app.entries.last().expect("entry");
    assert!(
        last.text.contains("time machine: turn 1 restored 2 files"),
        "revert notice missing: {}",
        last.text
    );
}

#[tokio::test]
async fn fork_trims_the_session_to_the_requested_turn() {
    use crate::agent::provider::ChatMessage;

    let mut app = test_app().await;
    for i in 0..3 {
        app.entries
            .push(Entry::new(EntryRole::User, format!("question {i}")));
        app.entries
            .push(assistant(&format!("answer {i} with a little bit of text")));
        app.history.push(ChatMessage::user(format!("question {i}")));
        app.history
            .push(ChatMessage::assistant(format!("answer {i}")));
    }
    app.turn_history = vec![2, 2, 2];

    app.fork_session_at(2);
    assert_eq!(app.turn_count(), 2, "session trimmed to two turns");
    assert_eq!(app.entries.len(), 4, "two exchanges remain");
    assert_eq!(app.turn_history.len(), 2);
    assert_eq!(app.history.len(), 4, "history trimmed with the turns");
    assert!(!app.is_busy(), "forking while idle stays idle");
}

#[tokio::test]
async fn doctor_report_lands_as_notes() {
    let mut app = test_app().await;
    app.on_doctor_report(vec![
        "✓ provider scripted · http://localhost".into(),
        "✗ api key missing - /connect".into(),
    ]);
    assert_eq!(app.entries.len(), 2);
    let last = app.entries.last().expect("entry");
    assert!(last.text.contains("api key missing"), "{}", last.text);
    let (msg, _) = app.toast.clone().expect("toast");
    assert!(msg.contains("1 check"), "toast: {msg}");
}

#[tokio::test]
async fn ab_with_empty_prompt_is_refused() {
    let mut app = test_app().await;
    app.run_ab_test("   ");
    assert!(app.overlay.is_none(), "no board without a prompt");
    let (msg, _) = app.toast.clone().expect("toast");
    assert!(msg.contains("usage"), "toast: {msg}");
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

    let mut terminal = Terminal::new(TestBackend::new(92, 40)).unwrap();
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
    // Prompt is now queued, not kept in composer
    assert!(
        app.composer.is_empty(),
        "composer should be cleared when busy"
    );
    assert_eq!(app.prompt_queue_len(), 1, "prompt should be queued");
    assert_eq!(app.prompt_queue_front().unwrap(), "second");
    assert_eq!(app.entries.len(), 2, "a second turn must not start");
    assert!(app.toast.is_some(), "user was not told why");
    assert!(app.toast.as_ref().unwrap().0.contains("queued"));
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
    // Umbrella is the default skin, so switch to Mono: applying the current
    // theme is a deliberate no-op and must not touch the disk.
    app.apply_theme(crate::ui::theme::ThemeId::Mono);
    assert_eq!(app.theme.id, crate::ui::theme::ThemeId::Mono);
    assert_eq!(app.cfg.theme, "mono");
    let path = app.cfg.store_path.clone().unwrap();
    let stored = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        stored.contains("theme = \"mono\""),
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
    for skin in ["mono", "midnight", "graphite", "umbrella"] {
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

// ---------------------------------------------------------------------------
// /tools — the tool library
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sudo_password_modal_renders_with_theme_chrome() {
    let mut app = test_app().await;
    app.overlay = Some(Overlay::SudoPassword(SudoPasswordState {
        command: "sudo apt-get install -y nmap".into(),
        cwd: None,
        timeout_ms: None,
        password: "hunter2".into(),
        submitting: false,
        error: None,
    }));

    let text = frame_text(&mut app, 90, 24);
    assert!(
        text.contains("sudo password required"),
        "title missing:\n{text}"
    );
    assert!(text.contains("command"), "command label missing:\n{text}");
    assert!(
        text.contains("apt-get install"),
        "command line missing:\n{text}"
    );
    assert!(text.contains("password"), "password label missing:\n{text}");
    // The password is masked: no plaintext anywhere.
    assert!(!text.contains("hunter2"), "password leaked:\n{text}");
    // The masked bullets and cursor glyph are drawn.
    assert!(text.contains(glyph::BULLET), "mask missing:\n{text}");
    // Key hints render like the other overlays.
    assert!(text.contains("esc cancel"), "hints missing:\n{text}");

    // Theme chrome: the border is drawn with border_focus, the title with
    // accent, and the panel background is applied - verify styled spans.
    let mut terminal = ratatui::Terminal::new(TestBackend::new(90, 24)).expect("terminal");
    terminal.draw(|f| render(f, &mut app)).expect("draw");
    let buffer = terminal.backend().buffer().clone();
    let theme = app.theme;

    // Some cell inside the frame must carry the focused-border color (the
    // rounded border box) and the title cell must carry the accent color.
    let mut saw_border = false;
    let mut saw_accent_title = false;
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            if cell.style().fg == Some(theme.border_focus()) {
                saw_border = true;
            }
            if cell.symbol() == "s" && cell.style().fg == Some(theme.accent()) {
                saw_accent_title = true;
            }
        }
    }
    assert!(
        saw_border,
        "border_focus color not used - modal is not themed"
    );
    assert!(
        saw_accent_title,
        "accent title color not used - modal is not themed"
    );
}

#[tokio::test]
async fn sudo_password_modal_shows_error_and_empty_hint() {
    let mut app = test_app().await;
    app.overlay = Some(Overlay::SudoPassword(SudoPasswordState {
        command: "sudo -v".into(),
        cwd: None,
        timeout_ms: None,
        password: String::new(),
        submitting: false,
        error: None,
    }));

    let text = frame_text(&mut app, 90, 24);
    assert!(
        text.contains("type your sudo password"),
        "empty hint missing:\n{text}"
    );

    app.overlay = Some(Overlay::SudoPassword(SudoPasswordState {
        command: "sudo -v".into(),
        cwd: None,
        timeout_ms: None,
        password: String::new(),
        submitting: false,
        error: Some("Password cannot be empty".into()),
    }));
    let text = frame_text(&mut app, 90, 24);
    assert!(
        text.contains("Password cannot be empty"),
        "error missing:\n{text}"
    );
}

#[tokio::test]
async fn shift_tab_cycles_all_four_modes() {
    use crate::agent::policy::ToolPolicy;
    let mut app = test_app().await;
    // Default is build.
    assert_eq!(app.cfg.tool_policy, ToolPolicy::Build);

    // Real terminals send Shift+Tab as backtab (CSI Z) with NO shift
    // modifier - crossterm reports KeyCode::BackTab. That is the press that
    // must actually fire on a keyboard.
    press(
        &mut app,
        ratatui::crossterm::event::KeyCode::BackTab,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    assert_eq!(app.cfg.tool_policy, ToolPolicy::Hacker);

    // Terminals with modifier reporting (Kitty protocol) send Tab + SHIFT.
    press(
        &mut app,
        ratatui::crossterm::event::KeyCode::Tab,
        ratatui::crossterm::event::KeyModifiers::SHIFT,
    );
    assert_eq!(app.cfg.tool_policy, ToolPolicy::Plan);

    // And the rest of the cycle via the real-terminal BackTab press:
    // plan -> coder -> build, wrapping to the start.
    press(
        &mut app,
        ratatui::crossterm::event::KeyCode::BackTab,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    assert_eq!(app.cfg.tool_policy, ToolPolicy::Coder);
    press(
        &mut app,
        ratatui::crossterm::event::KeyCode::BackTab,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    assert_eq!(app.cfg.tool_policy, ToolPolicy::Build);

    // The setting persists with the config.
    let path =
        std::env::temp_dir().join(format!("nanomite-mode-cycle-{}.toml", std::process::id()));
    app.cfg.store_path = Some(path.clone());
    app.cfg.tool_policy = ToolPolicy::Hacker;
    app.cfg.save();
    let stored = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        stored.contains("tool_policy = \"hacker\""),
        "mode not persisted: {stored}"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn footer_shows_mode_badge_for_non_default_modes() {
    use crate::agent::policy::ToolPolicy;
    let mut app = test_app().await;

    // Every mode shows its badge with its dedicated palette colour:
    // amber plan, blue coder, green build, red hacker.
    for (mode, badge, color) in [
        (ToolPolicy::Plan, "plan mode", app.theme.mode_plan()),
        (ToolPolicy::Coder, "coder mode", app.theme.mode_coder()),
        (ToolPolicy::Build, "build mode", app.theme.mode_build()),
        (ToolPolicy::Hacker, "hacker mode", app.theme.mode_hacker()),
    ] {
        app.cfg.tool_policy = mode;
        let mut terminal = ratatui::Terminal::new(TestBackend::new(100, 24)).expect("terminal");
        terminal.draw(|f| render(f, &mut app)).expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let text: String = (0..buffer.area.height)
            .map(|y| {
                let mut line = String::new();
                for x in 0..buffer.area.width {
                    line.push_str(buffer[(x, y)].symbol());
                }
                line
            })
            .collect();
        assert!(
            text.contains(badge),
            "{badge} missing for {mode:?}:
{text}"
        );

        // The badge must draw the mode's dedicated colour, not a borrowed
        // role colour.
        let mut saw_mode_color = false;
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if buffer[(x, y)].style().fg == Some(color) {
                    saw_mode_color = true;
                }
            }
        }
        assert!(
            saw_mode_color,
            "{mode:?}: dedicated mode colour {color:?} not drawn"
        );
    }
    app.cfg.tool_policy = ToolPolicy::Build;
}

#[tokio::test]
async fn queued_prompt_dispatches_when_the_turn_finishes() {
    use crate::app::{EntryRole, Phase};
    let mut app = test_app().await;
    app.composer.insert_str("first");
    app.submit();
    assert!(app.is_busy());
    app.composer.insert_str("second");
    app.submit();
    assert_eq!(app.prompt_queue_len(), 1);

    // The turn finishes: the queued prompt must start as a fresh turn
    // without the user touching anything. The scripted provider trickles,
    // so pump until the first turn actually completes.
    for _ in 0..300 {
        app.pump_engine();
        if app.prompt_queue_len() == 0 && !app.prompt_queue_front().is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        app.prompt_queue_len() == 0,
        "queue must drain after the turn finishes"
    );
    assert!(app.is_busy(), "the queued prompt must start a new turn");
    // The new turn's user entry is the queued prompt.
    let user_texts: Vec<&str> = app
        .entries
        .iter()
        .filter(|e| e.role == EntryRole::User)
        .map(|e| e.text.as_str())
        .collect();
    assert_eq!(
        user_texts,
        vec!["first", "second"],
        "queue order must be FIFO"
    );
    // The streaming entry for the second turn exists.
    assert!(
        app.entries
            .iter()
            .any(|e| e.role == EntryRole::Assistant && e.phase == Phase::Streaming),
        "a streaming assistant entry must exist for the new turn"
    );
}

#[tokio::test]
async fn queue_strip_renders_queued_messages_quietly() {
    let mut app = test_app().await;
    app.composer.insert_str("first");
    app.submit();
    app.composer.insert_str("second message");
    app.submit();
    app.composer.insert_str("third");
    app.submit();
    assert_eq!(app.prompt_queue_len(), 2);

    let text = frame_text(&mut app, 100, 24);
    // The strip names the event and shows what is waiting.
    assert!(
        text.contains("queued for after this turn"),
        "queue header missing:\n{text}"
    );
    assert!(
        text.contains("second message"),
        "queued message preview missing:\n{text}"
    );
    // Quiet styling: the strip must not render the old highlighted chrome.
    assert!(
        !text.contains("Queued prompts ("),
        "old highlighted queue UI is back:\n{text}"
    );
}

#[tokio::test]
async fn tools_overlay_lists_all_tools_and_toggles() {
    let mut app = test_app().await;
    let path = std::env::temp_dir().join(format!("nanomite-ui-tools-{}.toml", std::process::id()));
    app.cfg.store_path = Some(path.clone());

    // `/` opens the palette; the filter "tools" narrows it to the one command.
    type_text(&mut app, "/tools");
    press(
        &mut app,
        ratatui::crossterm::event::KeyCode::Enter,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    assert!(matches!(
        app.overlay.as_ref().map(Overlay::kind),
        Some(OverlayKind::Tools)
    ));

    let text = frame_text(&mut app, 100, 80);
    assert!(text.contains("tools"), "overlay title missing:\n{text}");
    for spec in crate::tools::ToolSet::catalog() {
        assert!(
            text.contains(spec.name),
            "tool {} missing:\n{text}",
            spec.name
        );
    }
    assert!(text.contains('✓'), "enabled marker missing:\n{text}");
    assert!(app.cfg.disabled_tools.is_empty(), "nothing disabled yet");

    // Enter toggles the highlighted row (list_files is first in the catalog).
    press(
        &mut app,
        ratatui::crossterm::event::KeyCode::Enter,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    assert_eq!(app.cfg.disabled_tools, vec!["list_files".to_string()]);
    assert!(
        app.toast
            .as_ref()
            .is_some_and(|(msg, _)| msg.contains("list_files disabled")),
        "no disable toast: {:?}",
        app.toast
    );

    let stored = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        stored.contains("disabled_tools = \"list_files\""),
        "disable not persisted: {stored}"
    );

    // Enter again re-enables and the persisted line disappears.
    press(
        &mut app,
        ratatui::crossterm::event::KeyCode::Enter,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    assert!(app.cfg.disabled_tools.is_empty(), "toggle back on failed");
    let stored = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        !stored.contains("disabled_tools"),
        "empty list still persisted: {stored}"
    );

    app.close_overlay();
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// /panel — the agent message board
// ---------------------------------------------------------------------------

fn board_dir_for_tests() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nanomite-board-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("board dir");
    std::env::set_var("NANOMITE_BOARD_DIR", &dir);
    dir
}

#[tokio::test]
async fn panel_convenes_board_and_renders_thread() {
    let board_dir = board_dir_for_tests();

    let mut app = test_app().await;
    app.composer
        .insert_str("/panel should we split the runtime?");
    app.submit();
    assert!(matches!(
        app.overlay.as_ref().map(Overlay::kind),
        Some(OverlayKind::Board)
    ));

    // 1 topic + 4 personas x 2 rounds + 1 chair = 10. The scripted provider
    // answers instantly, so pump until the done marker lands (mirrors
    // busy_footer).
    for _ in 0..900 {
        app.pump_engine();
        let done = matches!(
            &app.overlay,
            Some(Overlay::Board(b)) if !b.running && b.messages.len() >= 10
        );
        if done {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let Some(Overlay::Board(board)) = &app.overlay else {
        panic!("board overlay missing");
    };
    assert!(!board.running, "convene never finished");
    assert_eq!(board.messages.len(), 10);
    assert_eq!(board.messages[0].persona, "topic");
    assert_eq!(board.messages[0].body, "should we split the runtime?");
    assert_eq!(board.messages.last().unwrap().persona, "Chair");
    assert!(board.thread_id.is_some(), "thread id missing");
    let seen: std::collections::BTreeSet<&str> =
        board.messages.iter().map(|m| m.persona.as_str()).collect();
    for persona in ["Architect", "Engineer", "Scientist", "Critic", "Chair"] {
        assert!(seen.contains(persona), "persona {persona} never spoke");
    }

    // The store was opened through the env override, not the real data dir.
    assert!(
        board_dir.join("board.db").exists(),
        "board store was not created"
    );

    // Rendering: topic title, persona headers, and the scripted body — the
    // tail (newest messages) is pinned, so the chair is on screen.
    let text = frame_text(&mut app, 110, 44);
    assert!(
        text.contains("panel · should we split the runtime?"),
        "topic title missing:\n{text}"
    );
    assert!(
        text.contains("Chair (round 3):"),
        "chair header missing:\n{text}"
    );
    assert!(
        text.contains("Scripted test reply"),
        "scripted body missing:\n{text}"
    );
    app.close_overlay();
}

#[tokio::test]
async fn panel_esc_cancels_and_closes_cleanly() {
    let _board_dir = board_dir_for_tests();

    let mut app = test_app().await;
    app.composer.insert_str("/panel long running debate");
    app.submit();
    assert!(matches!(
        app.overlay.as_ref().map(Overlay::kind),
        Some(OverlayKind::Board)
    ));
    assert!(
        app.board_cancel.is_some(),
        "cancel flag must be shared with the engine"
    );

    press(
        &mut app,
        ratatui::crossterm::event::KeyCode::Esc,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    assert!(app.overlay.is_none(), "esc must close the board");
    assert!(
        app.board_cancel.is_none(),
        "esc must retire the cancel flag"
    );

    // The in-flight convene observes the flag and retires itself; pumping the
    // stragglers must neither panic nor reopen anything.
    for _ in 0..60 {
        app.pump_engine();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(app.overlay.is_none());
    }
}

#[tokio::test]
async fn panel_without_args_toasts_usage() {
    let mut app = test_app().await;
    app.composer.insert_str("/panel");
    app.submit();
    assert!(app.overlay.is_none(), "no board without a topic");
    assert!(
        app.toast
            .as_ref()
            .is_some_and(|(msg, _)| msg.contains("usage")),
        "no usage toast: {:?}",
        app.toast
    );
}

/// A memory engine owned by the test process only: no test may ever read or
/// write the real `~/.config/nanomite/memory` tree.
fn in_memory_engine() -> std::sync::Arc<crate::memory::MemoryEngine> {
    std::sync::Arc::new(
        crate::memory::MemoryEngine::open_in_memory(crate::memory::policy::MemoryPolicy {
            mirror_markdown: false,
            ..Default::default()
        })
        .expect("in-memory memory engine"),
    )
}

fn seed_record(engine: &crate::memory::MemoryEngine, content: &str, summary: &str) {
    engine
        .save(&crate::memory::model::NewMemory {
            category: crate::memory::model::MemoryCategory::Core,
            content: content.to_string(),
            summary: Some(summary.to_string()),
            tags: vec!["arch".to_string()],
            source: crate::memory::model::MemorySource::new(
                crate::memory::model::SourceKind::UserInstruction,
            ),
            project: Some("nanomite".to_string()),
            confidence: Some(1.0),
            importance: Some(0.8),
            expires_at: None,
            sensitivity: Some(crate::memory::model::Sensitivity::Internal),
            mirror_markdown: false,
        })
        .expect("seed memory");
}

#[tokio::test]
async fn memory_picker_lists_records_and_opens_the_body() {
    let mut app = test_app().await;
    let engine = in_memory_engine();
    seed_record(
        &engine,
        "nanomite keeps memory in sqlite",
        "memory lives in sqlite",
    );
    app.attach_memory_for_test(engine);

    app.run_command(crate::commands::Command::Memory, "");
    let text = frame_text(&mut app, 92, 24);
    // Still loading: the rows come from the background task.
    assert!(text.contains("memory entries"), "picker missing:\n{text}");

    for _ in 0..200 {
        app.pump_engine();
        if let Some(Overlay::Picker(p)) = &app.overlay {
            if !p.items.is_empty() {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let text = frame_text(&mut app, 92, 24);
    assert!(
        text.contains("memory lives in sqlite"),
        "row missing:\n{text}"
    );
    assert!(text.contains("core"), "category missing:\n{text}");
    assert!(text.contains("nanomite"), "project missing:\n{text}");

    app.on_key(ratatui::crossterm::event::KeyEvent {
        code: ratatui::crossterm::event::KeyCode::Enter,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
        kind: ratatui::crossterm::event::KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    });
    assert_eq!(
        app.overlay.as_ref().map(Overlay::kind),
        Some(OverlayKind::Memory)
    );
    let text = frame_text(&mut app, 92, 30);
    assert!(text.contains("memory"), "overlay title missing:\n{text}");
    assert!(
        text.contains("keeps memory in sqlite"),
        "record body missing:\n{text}"
    );

    // Esc closes it.
    app.on_key(ratatui::crossterm::event::KeyEvent {
        code: ratatui::crossterm::event::KeyCode::Esc,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
        kind: ratatui::crossterm::event::KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    });
    assert!(app.overlay.is_none(), "esc must close the record overlay");
}

#[tokio::test]
async fn memory_picker_closes_on_esc_without_rows() {
    let mut app = test_app().await;
    app.attach_memory_for_test(in_memory_engine());
    app.open_memory_picker();
    assert!(matches!(app.overlay, Some(Overlay::Picker(_))));
    app.on_key(ratatui::crossterm::event::KeyEvent {
        code: ratatui::crossterm::event::KeyCode::Esc,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
        kind: ratatui::crossterm::event::KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    });
    assert!(app.overlay.is_none());
}

#[tokio::test]
async fn memory_picker_without_an_engine_explains_itself() {
    let mut app = test_app().await;
    assert!(!app.memory_attached());
    app.open_memory_picker();
    assert!(app.overlay.is_none(), "no engine, no picker");
    let text = frame_text(&mut app, 92, 24);
    // The toast message is shown in the footer
    assert!(
        text.contains("memory not available"),
        "toast missing:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// /improve — self-edit proposals
// ---------------------------------------------------------------------------

/// A disposable tree for the self-improvement tests: the anchor files the engine
/// binds to plus a source file inside `repo_root`. No test ever touches the real
/// workspace or the user's config.
fn improve_fixture(tag: &str) -> (TempDir, Roots) {
    let temp = temp_dir(tag);
    let root = &temp.path;
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(root.join("prompt.md"), "base instructions\n").unwrap();
    std::fs::write(root.join("config.toml"), "model = \"test-model\"\n").unwrap();
    let roots = Roots {
        config_path: root.join("config.toml"),
        prompt_path: root.join("prompt.md"),
        repo_root: root.clone(),
    };
    (temp, roots)
}

/// Dispatches a tool call through the same layer a model turn uses.
fn dispatch(app: &App, name: &str, args: serde_json::Value) -> Result<(String, String), String> {
    let tools = app.tool_set_for_test();
    crate::agent::runtime::run_tool(
        &tools,
        &[],
        crate::agent::policy::ToolPolicy::Hacker,
        &[],
        std::path::Path::new("."),
        &crate::agent::provider::ToolCall {
            id: "test-call".into(),
            name: name.into(),
            arguments: args.to_string(),
            index: 0,
        },
    )
}

/// Stages a source edit the way `propose_self_edit` does, returning its id.
fn stage_source_edit(app: &App, new_content: &str, rationale: &str) -> String {
    let (payload, summary) = dispatch(
        app,
        "propose_self_edit",
        serde_json::json!({
            "target": "source",
            "path": "src/lib.rs",
            "new_content": new_content,
            "rationale": rationale,
        }),
    )
    .expect("propose_self_edit dispatched");
    let value: serde_json::Value = serde_json::from_str(&payload).expect("payload is json");
    assert_eq!(value["status"], "pending", "{payload}");
    assert!(
        value["notice"]
            .as_str()
            .unwrap_or_default()
            .contains("staged for user review"),
        "the model must be told nothing is applied automatically: {payload}"
    );
    assert!(
        summary.contains("imp-") && summary.contains("+1"),
        "summary missing id and size: {summary}"
    );
    value["id"].as_str().expect("proposal id").to_string()
}

/// Keypress with no modifiers; the `/improve` paths under test never need any.
fn improve_key(app: &mut App, code: ratatui::crossterm::event::KeyCode) {
    app.on_key(ratatui::crossterm::event::KeyEvent {
        code,
        modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
        kind: ratatui::crossterm::event::KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    });
}

/// Pumps until `ready` holds, so an async list/apply result can land.
async fn pump_until(app: &mut App, what: &str, mut ready: impl FnMut(&App) -> bool) {
    for _ in 0..200 {
        app.pump_engine();
        if ready(app) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    app.pump_engine();
    panic!("{what} never arrived");
}

fn improve_state(app: &App) -> crate::app::ImproveState {
    match app.overlay.as_ref() {
        Some(Overlay::Improve(state)) => state.clone(),
        _ => panic!("the /improve overlay is not open"),
    }
}

#[tokio::test]
async fn proposed_self_edits_reach_the_improve_list() {
    let mut app = test_app().await;
    let (_temp, roots) = improve_fixture("ui-improve-list");
    let engine = std::sync::Arc::new(ImproveEngine::new(roots.repo_root.join(".improve-data")));
    app.attach_improve_for_test(
        engine,
        roots.clone(),
        FakeRunner {
            passed: true,
            detail: String::new(),
        },
    );

    let id = stage_source_edit(&app, "pub fn b() {}\n", "rename the entry point");
    app.run_command(Command::Improve, "");
    assert_eq!(
        app.overlay.as_ref().map(Overlay::kind),
        Some(OverlayKind::Improve)
    );
    pump_until(&mut app, "the pending list", |app| {
        !improve_state(app).rows.is_empty()
    })
    .await;

    let state = improve_state(&app);
    assert_eq!(state.rows.len(), 1);
    assert_eq!(state.rows[0].id, id);
    assert_eq!(state.rows[0].target, "source");
    assert_eq!((state.rows[0].added, state.rows[0].removed), (1, 1));
    assert_eq!(state.rows[0].rationale, "rename the entry point");
    // Staging changed nothing on disk.
    assert_eq!(
        std::fs::read_to_string(roots.repo_root.join("src/lib.rs")).unwrap(),
        "pub fn a() {}\n"
    );

    let text = frame_text(&mut app, 100, 28);
    assert!(text.contains("improve"), "overlay title missing:\n{text}");
    assert!(text.contains("source"), "target missing:\n{text}");
    assert!(
        text.contains("rename the entry point"),
        "rationale missing:\n{text}"
    );
    assert!(text.contains("+1"), "diff size missing:\n{text}");
}

#[tokio::test]
async fn improve_review_view_renders_the_staged_diff() {
    let mut app = test_app().await;
    let (_temp, roots) = improve_fixture("ui-improve-review");
    let engine = std::sync::Arc::new(ImproveEngine::new(roots.repo_root.join(".improve-data")));
    app.attach_improve_for_test(
        engine,
        roots.clone(),
        FakeRunner {
            passed: true,
            detail: String::new(),
        },
    );
    stage_source_edit(&app, "pub fn b() {}\n", "rename the entry point");

    app.run_command(Command::Improve, "");
    pump_until(&mut app, "the pending list", |app| {
        !improve_state(app).rows.is_empty()
    })
    .await;

    improve_key(&mut app, ratatui::crossterm::event::KeyCode::Enter);
    pump_until(&mut app, "the review diff", |app| {
        improve_state(app).review.is_some()
    })
    .await;

    let review = improve_state(&app).review.expect("review view");
    assert_eq!((review.added, review.removed), (1, 1));
    assert!(review
        .lines
        .iter()
        .any(|l| l.starts_with('-') && l.contains("pub fn a()")));
    assert!(review
        .lines
        .iter()
        .any(|l| l.starts_with('+') && l.contains("pub fn b()")));
    assert!(review.lines.iter().any(|l| l.starts_with("@@")));

    let text = frame_text(&mut app, 100, 30);
    assert!(
        text.contains("-pub fn a() {}"),
        "removal line missing:\n{text}"
    );
    assert!(
        text.contains("+pub fn b() {}"),
        "addition line missing:\n{text}"
    );
    assert!(text.contains("a approve"), "approve hint missing:\n{text}");
    assert!(text.contains("r reject"), "reject hint missing:\n{text}");
}

#[tokio::test]
async fn approving_a_proposal_applies_it_when_the_regression_gate_passes() {
    let mut app = test_app().await;
    let (_temp, roots) = improve_fixture("ui-improve-approve");
    let engine = std::sync::Arc::new(ImproveEngine::new(roots.repo_root.join(".improve-data")));
    app.attach_improve_for_test(
        engine,
        roots.clone(),
        FakeRunner {
            passed: true,
            detail: "test result: ok".into(),
        },
    );
    stage_source_edit(&app, "pub fn b() {}\n", "rename the entry point");

    app.run_command(Command::Improve, "");
    pump_until(&mut app, "the pending list", |app| {
        !improve_state(app).rows.is_empty()
    })
    .await;
    improve_key(&mut app, ratatui::crossterm::event::KeyCode::Enter);
    pump_until(&mut app, "the review diff", |app| {
        improve_state(app).review.is_some()
    })
    .await;

    improve_key(&mut app, ratatui::crossterm::event::KeyCode::Char('a'));
    // The verify runs off the UI thread and says so while it is in flight.
    let in_flight = improve_state(&app);
    assert!(app.improve_applying());
    assert_eq!(
        in_flight.status.as_ref().map(|s| s.message.as_str()),
        Some("verifying… (cargo test --lib)")
    );
    pump_until(&mut app, "the apply outcome", |app| {
        !app.improve_applying() && improve_state(app).status.is_some()
    })
    .await;

    let state = improve_state(&app);
    let status = state.status.expect("a status line");
    assert_eq!(status.level, crate::app::ImproveLevel::Success);
    assert!(status.message.contains("applied"), "{}", status.message);
    // The regression gate passed, so the change is on disk.
    assert_eq!(
        std::fs::read_to_string(roots.repo_root.join("src/lib.rs")).unwrap(),
        "pub fn b() {}\n"
    );
    // And the applied proposal left the queue.
    assert!(state.rows.is_empty());
}

#[tokio::test]
async fn a_failed_regression_gate_rolls_the_change_back_byte_for_byte() {
    let mut app = test_app().await;
    let (_temp, roots) = improve_fixture("ui-improve-rollback");
    let engine = std::sync::Arc::new(ImproveEngine::new(roots.repo_root.join(".improve-data")));
    app.attach_improve_for_test(
        engine,
        roots.clone(),
        FakeRunner {
            passed: false,
            detail: "test result: FAILED. 1 failed\nthread 'x' panicked".into(),
        },
    );
    stage_source_edit(&app, "pub fn b() {}\n", "break it on purpose");
    let original = std::fs::read(roots.repo_root.join("src/lib.rs")).unwrap();

    app.run_command(Command::Improve, "");
    pump_until(&mut app, "the pending list", |app| {
        !improve_state(app).rows.is_empty()
    })
    .await;
    improve_key(&mut app, ratatui::crossterm::event::KeyCode::Enter);
    pump_until(&mut app, "the review diff", |app| {
        improve_state(app).review.is_some()
    })
    .await;
    improve_key(&mut app, ratatui::crossterm::event::KeyCode::Char('a'));
    pump_until(&mut app, "the rollback outcome", |app| {
        !app.improve_applying() && improve_state(app).status.is_some()
    })
    .await;

    let state = improve_state(&app);
    let status = state.status.expect("a status line");
    assert_eq!(status.level, crate::app::ImproveLevel::Warn);
    assert!(status.message.contains("rolled back"), "{}", status.message);
    assert!(
        status.message.contains("panicked"),
        "the failure tail must reach the user: {}",
        status.message
    );
    assert_eq!(
        std::fs::read(roots.repo_root.join("src/lib.rs")).unwrap(),
        original,
        "a rolled-back proposal must restore the original bytes"
    );
    // The list reloads with the failed proposal gone from the queue.
    assert!(state.rows.is_empty());
}

#[tokio::test]
async fn rejecting_a_proposal_leaves_the_file_alone() {
    let mut app = test_app().await;
    let (_temp, roots) = improve_fixture("ui-improve-reject");
    let engine = std::sync::Arc::new(ImproveEngine::new(roots.repo_root.join(".improve-data")));
    app.attach_improve_for_test(
        engine,
        roots.clone(),
        FakeRunner {
            passed: true,
            detail: String::new(),
        },
    );
    stage_source_edit(&app, "pub fn b() {}\n", "never mind");

    app.run_command(Command::Improve, "");
    pump_until(&mut app, "the pending list", |app| {
        !improve_state(app).rows.is_empty()
    })
    .await;
    improve_key(&mut app, ratatui::crossterm::event::KeyCode::Enter);
    pump_until(&mut app, "the review diff", |app| {
        improve_state(app).review.is_some()
    })
    .await;
    improve_key(&mut app, ratatui::crossterm::event::KeyCode::Char('r'));
    pump_until(&mut app, "the rejection outcome", |app| {
        improve_state(app).status.is_some()
    })
    .await;

    let state = improve_state(&app);
    assert!(
        state
            .status
            .as_ref()
            .expect("status")
            .message
            .contains("rejected"),
        "{:?}",
        state.status
    );
    assert!(
        state.rows.is_empty(),
        "a rejected proposal leaves the queue"
    );
    assert!(
        state.review.is_none(),
        "the review view closes once the proposal is gone"
    );
    assert_eq!(
        std::fs::read_to_string(roots.repo_root.join("src/lib.rs")).unwrap(),
        "pub fn a() {}\n",
        "rejecting must never write"
    );
}

#[tokio::test]
async fn approve_and_reject_are_unreachable_from_the_list_view() {
    let mut app = test_app().await;
    let (_temp, roots) = improve_fixture("ui-improve-gate");
    let engine = std::sync::Arc::new(ImproveEngine::new(roots.repo_root.join(".improve-data")));
    app.attach_improve_for_test(
        engine,
        roots.clone(),
        FakeRunner {
            passed: true,
            detail: String::new(),
        },
    );
    stage_source_edit(&app, "pub fn b() {}\n", "should not be applied");

    app.run_command(Command::Improve, "");
    pump_until(&mut app, "the pending list", |app| {
        !improve_state(app).rows.is_empty()
    })
    .await;
    // No review open: `a` is inert, so no approval can happen.
    improve_key(&mut app, ratatui::crossterm::event::KeyCode::Char('a'));
    improve_key(&mut app, ratatui::crossterm::event::KeyCode::Char('r'));
    assert!(!app.improve_applying());
    assert_eq!(
        std::fs::read_to_string(roots.repo_root.join("src/lib.rs")).unwrap(),
        "pub fn a() {}\n"
    );
    assert_eq!(improve_state(&app).rows.len(), 1, "nothing was decided");
}

#[tokio::test]
async fn an_empty_queue_says_so() {
    let mut app = test_app().await;
    let (_temp, roots) = improve_fixture("ui-improve-empty");
    let engine = std::sync::Arc::new(ImproveEngine::new(roots.repo_root.join(".improve-data")));
    app.attach_improve_for_test(
        engine,
        roots.clone(),
        FakeRunner {
            passed: true,
            detail: String::new(),
        },
    );

    app.run_command(Command::Improve, "");
    pump_until(&mut app, "the pending list", |app| {
        !improve_state(app).loading
    })
    .await;
    assert!(
        app.toast
            .as_ref()
            .is_some_and(|(msg, _)| msg.contains("no pending proposals")),
        "{:?}",
        app.toast
    );
    let text = frame_text(&mut app, 100, 28);
    assert!(
        text.contains("no pending proposals"),
        "empty state missing:\n{text}"
    );
}

#[tokio::test]
async fn the_denylist_is_enforced_before_anything_is_staged() {
    let mut app = test_app().await;
    let (_temp, roots) = improve_fixture("ui-improve-denylist");
    let engine = std::sync::Arc::new(ImproveEngine::new(roots.repo_root.join(".improve-data")));
    app.attach_improve_for_test(
        engine,
        roots.clone(),
        FakeRunner {
            passed: true,
            detail: String::new(),
        },
    );
    std::fs::create_dir_all(roots.repo_root.join("src/memory")).unwrap();
    std::fs::write(
        roots.repo_root.join("src/memory/policy.rs"),
        "pub fn safe() {}\n",
    )
    .unwrap();

    let err = dispatch(
        &app,
        "propose_self_edit",
        serde_json::json!({
            "target": "source",
            "path": "src/memory/policy.rs",
            "new_content": "pub fn unsafe() {}\n",
            "rationale": "loosen a rule",
        }),
    )
    .expect_err("the safety layer must stay off limits");
    assert!(err.contains("improve_policy"), "{err}");
    assert!(app.improve_pending_rows().is_empty());
}

#[tokio::test]
async fn the_help_sheet_lists_the_improve_command() {
    let mut app = test_app().await;
    app.open_help();
    let text = frame_text(&mut app, 92, 30);
    assert!(text.contains("/improve"), "command row missing:\n{text}");
    let help = frame_text(&mut app, 92, 44);
    assert!(
        help.contains("review self-edit proposals"),
        "hint missing:\n{help}"
    );
}

#[tokio::test]
async fn esc_interrupts_during_initial_streaming() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    let mut app = test_app().await;
    app.composer.insert_str("hello");
    app.submit();

    assert!(app.is_busy(), "turn should be running");

    // Send ESC immediately after submit, during initial streaming
    app.on_key(KeyEvent {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: ratatui::crossterm::event::KeyEventState::NONE,
    });

    for _ in 0..100 {
        app.pump_engine();
        if !app.is_busy() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(!app.is_busy(), "esc did not interrupt during streaming");
    assert_eq!(app.entries[1].phase, Phase::Interrupted);
}

#[tokio::test]
async fn suggestion_strip_renders_above_the_composer() {
    let mut app = test_app().await;
    app.entries.push(Entry::new(EntryRole::User, "fix the bug"));
    app.entries.push(assistant("done, here is the diff"));
    app.suggestions = Some(SuggestionStrip {
        items: vec!["run the tests".into(), "commit the fix".into()],
        selected: 1,
    });

    let text = frame_text(&mut app, 80, 24);
    assert_frame_well_formed(&text, 80, 24);
    assert!(text.contains("next steps"), "strip header missing:\n{text}");
    assert!(text.contains("run the tests"), "item missing:\n{text}");
    assert!(text.contains("commit the fix"), "item missing:\n{text}");
    assert!(text.contains("tab cycle"), "key hints missing:\n{text}");
    // The composer box still renders below the strip.
    assert!(text.contains('❯'), "composer prompt missing:\n{text}");
}

#[tokio::test]
async fn no_suggestions_no_strip_rows() {
    let mut app = test_app().await;
    app.entries.push(Entry::new(EntryRole::User, "hello"));
    app.entries.push(assistant("hi there"));

    let plain = frame_text(&mut app, 80, 24);
    assert_frame_well_formed(&plain, 80, 24);
    assert!(
        !plain.contains("next steps"),
        "strip must stay hidden:\n{plain}"
    );
    assert!(!plain.contains("tab cycle"));

    app.suggestions = Some(SuggestionStrip {
        items: vec!["one".into()],
        selected: 0,
    });
    let with = frame_text(&mut app, 80, 24);
    assert!(with.contains("next steps"));
    assert!(with.contains("one"));
}
