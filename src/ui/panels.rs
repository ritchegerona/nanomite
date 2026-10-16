use super::text;
use super::theme::{glyph, Theme};
use crate::app::{Action, App, Overlay, OverlayKind, PickerItem, PickerValue, CONNECT_FIELDS};
use crate::commands::Command;

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

/// One row: brand + workspace on the left, model · effort · status on the right.
/// The model label is truncated with its tail kept visible (`…/gpt-oss-20b`),
/// so long ids never push the status off a narrow terminal.
pub fn header(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let width = area.width as usize;

    let mut left = vec![
        Span::styled(glyph::HEX, Style::default().fg(theme.accent())),
        Span::styled(" nanomite", theme.bold()),
    ];
    let path_budget = width.saturating_sub(24).min(40);
    if path_budget >= 6 {
        let path = text::display_path(&app.cwd, path_budget);
        left.push(Span::styled(
            format!(" {} ", glyph::CHEVRON),
            Style::default().fg(theme.faint()),
        ));
        left.push(Span::styled(path, theme.faint_style()));
    }
    frame.render_widget(Paragraph::new(Line::from(left)), area);

    // Right side: spinner while a turn runs, a quiet ready dot otherwise.
    let mark = if app.is_busy() {
        glyph::SPINNER[app.view.spinner % glyph::SPINNER.len()]
    } else {
        glyph::DOT
    };
    let fixed = glyph::DOT.width() + 2 * glyph::CHEVRON.width() + 2;
    let model_budget = width
        .saturating_sub(fixed + app.cfg.reasoning.label().len() + 2)
        .max(8);
    let model = text::truncate_head(&app.model_label(), model_budget);
    let right = Line::from(vec![
        Span::styled(
            format!("{mark} "),
            if app.is_busy() {
                Style::default().fg(theme.accent())
            } else {
                Style::default().fg(theme.faint())
            },
        ),
        Span::styled(model, theme.dim()),
        Span::styled(
            format!(" {} ", glyph::CHEVRON),
            Style::default().fg(theme.faint()),
        ),
        Span::styled(app.cfg.reasoning.label(), theme.faint_style()),
    ])
    .right_aligned();
    frame.render_widget(Paragraph::new(right), area);
}

/// Hairline separator under the header.
pub fn rule(frame: &mut Frame, area: Rect, theme: &Theme) {
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            glyph::RULE.repeat(area.width as usize),
            Style::default().fg(theme.border()),
        ))),
        area,
    );
}

/// Status strip above the composer. Rebuilt only when its inputs change.
pub fn footer(frame: &mut Frame, area: Rect, app: &mut App) {
    let theme = app.theme;
    let key = footer_key(app);
    if let Some((k, line)) = &app.view.footer {
        if *k == key {
            frame.render_widget(Paragraph::new(line.clone()), area);
            return;
        }
    }

    let mut left: Vec<Span<'static>> = Vec::new();
    match &app.toast {
        Some((msg, _)) if !msg.is_empty() => {
            let lower = msg.to_lowercase();
            let bad = lower.contains("failed")
                || lower.contains("error")
                || lower.contains("unknown")
                || lower.contains("refused");
            if bad {
                left.push(Span::styled(
                    format!("{} ", glyph::CROSS),
                    Style::default().fg(theme.danger()),
                ));
            } else if lower.contains("connected")
                || lower.contains("saved")
                || lower.contains("switched")
                || lower.contains("applied")
            {
                left.push(Span::styled(
                    format!("{} ", glyph::CHECK),
                    Style::default().fg(theme.success()),
                ));
            } else {
                left.push(Span::styled(
                    format!("{} ", glyph::DOT),
                    Style::default().fg(theme.faint()),
                ));
            }
            left.push(Span::styled(msg.clone(), theme.dim()));
        }
        _ if app.is_busy() => {
            let elapsed = app
                .turn_started
                .map(|t| text::format_duration(t.elapsed()))
                .unwrap_or_default();
            left.push(Span::styled(
                format!(
                    "{} ",
                    glyph::SPINNER[app.view.spinner % glyph::SPINNER.len()]
                ),
                Style::default().fg(theme.accent()),
            ));
            left.push(Span::styled("working", Style::default().fg(theme.muted())));
            left.push(Span::styled(format!(" {elapsed}"), theme.faint_style()));
            // Steps and tool calls so far this turn, when there are any.
            let (steps, tools) = app
                .active
                .and_then(|i| app.entries.get(i))
                .map(|e| (e.steps, e.tools.iter().filter(|t| !t.running).count()))
                .unwrap_or((0, 0));
            if steps > 0 || tools > 0 {
                left.push(Span::styled(
                    format!(" · {steps} step{}", if steps == 1 { "" } else { "s" }),
                    theme.faint_style(),
                ));
                if tools > 0 {
                    left.push(Span::styled(
                        format!(" · {tools} tool{}", if tools == 1 { "" } else { "s" }),
                        theme.faint_style(),
                    ));
                }
            }
            if app.pending_editor {
                left.push(Span::styled(" · editor open", theme.faint_style()));
            }
        }
        _ => {
            left.push(Span::styled(
                format!("{} ready", glyph::DOT),
                Style::default().fg(theme.faint()),
            ));
            if !app.view.follow {
                // `end` (or clicking the scrollbar's bottom) re-follows the live tail.
                left.push(Span::styled(
                    format!("  {} scrolled up (end)", glyph::ARROW_UP),
                    Style::default().fg(theme.warn()),
                ));
            }
        }
    }

    let usage = app.usage.total_tokens as usize;
    let limit = app.cfg.context_limit.max(1);
    let ctx = usage.min(limit);
    let pct = (usage * 100 / limit).min(999);
    // Context meter: proportional solid bar, amber past 80%, red past 95%.
    let meter_cells = 10usize;
    let filled = (ctx * meter_cells / limit).min(meter_cells);
    let meter_style = if pct >= 95 {
        Style::default().fg(theme.danger())
    } else if pct >= 80 {
        Style::default().fg(theme.warn())
    } else {
        Style::default().fg(theme.border_focus())
    };
    let meter = format!(
        "[{}{}] ",
        "\u{2588}".repeat(filled),
        "\u{2591}".repeat(meter_cells - filled)
    );
    // A real budget readout: absolute tokens over the limit, so the bar is
    // never the only signal (and an empty-looking bar still carries numbers).
    let ctx_tail = format!(
        "{}/{} · {}%",
        text::format_tokens(ctx as u64),
        text::format_tokens(limit as u64),
        pct
    );
    let right = Line::from(vec![
        Span::styled(format!("{} ", glyph::ARROW_UP), theme.faint_style()),
        Span::styled(
            text::format_tokens(app.usage.prompt_tokens as u64),
            theme.dim(),
        ),
        Span::raw("  "),
        Span::styled(format!("{} ", glyph::ARROW_DOWN), theme.faint_style()),
        Span::styled(
            text::format_tokens(app.usage.completion_tokens as u64),
            theme.dim(),
        ),
        Span::styled(
            format!("  {}", text::format_tokens(ctx as u64)),
            theme.faint_style(),
        ),
        Span::styled(format!(" {meter}"), meter_style),
        Span::styled(format!(" {ctx_tail}"), theme.faint_style()),
    ])
    .right_aligned();

    frame.render_widget(Paragraph::new(Line::from(left.clone())), area);
    frame.render_widget(Paragraph::new(right), area);

    app.view.footer = Some((key, Line::from(left)));
}

/// Keyboard sheet: command registry first, then keybindings.
pub fn help(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let offset = match &app.overlay {
        Some(Overlay::Help(state)) => state.offset,
        _ => 0,
    };
    let inner_width = area.width.saturating_sub(4) as usize;
    let keys: &[(&str, &str)] = &[
        ("ctrl+c / ctrl+d", "quit (ctrl+d only on an empty prompt)"),
        ("enter", "send · run the highlighted command"),
        ("ctrl+j / alt+enter", "newline in the prompt"),
        ("/ or ctrl+x", "command menu"),
        ("esc", "interrupt · close an overlay · clear the prompt"),
        ("ctrl+t / ctrl+l", "new session · clear transcript"),
        ("ctrl+r", "cycle reasoning effort"),
        ("ctrl+p / ctrl+n", "previous · next prompt from history"),
        ("ctrl+a / ctrl+e", "prompt home · prompt end"),
        (
            "ctrl+w / ctrl+u / ctrl+k",
            "delete word · clear line · delete to end",
        ),
        ("left / right (with ctrl)", "move by word"),
        ("up / down", "cursor within a multi-row prompt, else scroll"),
        ("pgup / pgdn / home / end", "scroll the transcript"),
        ("mouse wheel", "scroll the transcript"),
        ("tab", "next picker row (shift+tab back)"),
        ("f1", "this sheet"),
    ];

    let commands = crate::commands::COMMANDS;
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(commands.len() + keys.len() + 5);
    lines.push(Line::from(vec![
        Span::styled("  commands", theme.bold()),
        Span::styled("   type / to run", theme.faint_style()),
    ]));
    let cmd_w = commands
        .iter()
        .flat_map(|c| std::iter::once(c.command).chain(c.aliases.iter().copied()))
        .map(|c| c.chars().count())
        .max()
        .unwrap_or(12)
        .min(26);
    let hint_w = inner_width.saturating_sub(cmd_w + 4);
    for entry in commands {
        // Aliases are a nicety: drop them rather than wrap the row.
        let with_aliases = format!("{} ({})", entry.command, entry.aliases.join(", "));
        let names = if entry.aliases.is_empty()
            || with_aliases.chars().count() + entry.hint.chars().count() + 6 > inner_width
        {
            entry.command.to_string()
        } else {
            with_aliases
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {names:<cmd_w$}  "),
                Style::default().fg(theme.accent()),
            ),
            Span::styled(text::truncate(entry.hint, hint_w), theme.dim()),
        ]));
    }

    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled("  keys", theme.bold()),
        Span::styled("   terminal shortcuts", theme.faint_style()),
    ]));
    let key_w = keys
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(24)
        .min(28);
    for (key, desc) in keys {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:<key_w$}  "),
                Style::default().fg(theme.accent()),
            ),
            Span::styled((*desc).to_string(), theme.dim()),
        ]));
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus()))
        .title(
            Line::from(Span::styled(
                " keyboard ",
                Style::default().fg(theme.accent()),
            ))
            .centered(),
        )
        .title_bottom(Line::from(Span::styled(" esc close ", theme.faint_style())).right_aligned())
        .style(Style::default().bg(theme.panel_bg()));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .scroll((offset as u16, 0)),
        inner,
    );
}

/// Command / provider / model picker overlay.
pub fn picker(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let Some(Overlay::Picker(ref picker)) = app.overlay else {
        return;
    };

    let inner_w = area.width.saturating_sub(4) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;

    let query = picker.filter.value().to_string();
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" {} ", glyph::PROMPT),
                Style::default().fg(theme.accent()),
            ),
            Span::styled(
                if query.is_empty() {
                    picker.placeholder().to_string()
                } else {
                    query.clone()
                },
                if query.is_empty() {
                    theme.faint_style()
                } else {
                    theme.base()
                },
            ),
        ]),
        Line::default(),
    ];

    let visible = inner_h.saturating_sub(2);
    let start = picker.cursor.saturating_sub(visible.saturating_sub(1));
    if picker.loading {
        lines.push(Line::from(Span::styled(
            "  fetching models…",
            theme.faint_style(),
        )));
    }
    if picker.items.is_empty() && !picker.loading {
        lines.push(Line::from(Span::styled(
            if picker.accepts_free_text() {
                "  no match - enter to use the typed id"
            } else {
                "  no matching commands"
            },
            theme.faint_style(),
        )));
    }
    for (i, item) in picker.items.iter().enumerate().skip(start).take(visible) {
        let selected = i == picker.cursor;
        let marker = if selected { glyph::GUTTER } else { " " };
        // Reserve the check column so hints stay aligned whether or not a row is current.
        let check = if item.current { "✓" } else { " " };
        let label = format!("{check} {}", item.label);
        // The label is never squeezed: on a tight row the hint gives up its head
        // (the host) and keeps its tail (the key name) instead.
        let label_chars = text::width(&label);
        let avail = inner_w.saturating_sub(3 + label_chars + 2);
        let hint = text::truncate_head(&item.hint, avail);
        let label_w = inner_w.saturating_sub(3 + text::width(&hint));
        let label_style = if selected {
            Style::default().fg(theme.strong())
        } else if item.current {
            Style::default().fg(theme.accent_soft())
        } else {
            theme.dim()
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} "), Style::default().fg(theme.accent())),
            Span::styled(
                format!("{:<label_w$}", text::truncate(&label, label_w)),
                label_style,
            ),
            Span::styled(hint, theme.faint_style()),
        ]));
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus()))
        .title(
            Line::from(Span::styled(
                format!(" {} ", picker.title()),
                Style::default().fg(theme.accent()),
            ))
            .centered(),
        )
        .title_bottom(
            Line::from(Span::styled(
                " ↑↓ move · enter apply · esc close ",
                theme.faint_style(),
            ))
            .right_aligned(),
        )
        .style(Style::default().bg(theme.panel_bg()));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// `/connect` form: active provider (read-only), base URL, masked API key.
pub fn connect(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let Some(Overlay::Connect(ref form)) = app.overlay else {
        return;
    };

    let inner_w = area.width.saturating_sub(4) as usize;
    let label_w = ["provider"]
        .iter()
        .chain(CONNECT_FIELDS.iter())
        .map(|f| f.len())
        .max()
        .unwrap_or(9)
        + 2;
    let field_w = inner_w.saturating_sub(label_w + 2);

    let mut lines = Vec::with_capacity(CONNECT_FIELDS.len() + 2);

    // Who we are talking to: the active provider's name, never editable.
    lines.push(Line::from(vec![
        Span::styled(
            format!("   {:<label_w$}", "provider"),
            Style::default().fg(theme.faint()),
        ),
        Span::styled(
            text::truncate(&app.provider_label(), field_w),
            Style::default().fg(theme.strong()),
        ),
    ]));

    for (i, label) in CONNECT_FIELDS.iter().enumerate() {
        let focused = i == form.focus;
        let composer = form.field(i);
        let raw = composer.value().to_string();

        // Secrets stay masked except for the last four characters.
        let (display, value_style) = if *label == "api key" && !raw.is_empty() {
            let tail: String = raw
                .chars()
                .skip(raw.chars().count().saturating_sub(4))
                .collect();
            (
                format!(
                    "{}{}",
                    "•".repeat(raw.chars().count().saturating_sub(4)),
                    tail
                ),
                Style::default().fg(theme.text()),
            )
        } else if raw.is_empty() {
            (
                if *label == "api key" {
                    "(unset)"
                } else {
                    "(empty)"
                }
                .to_string(),
                Style::default().fg(theme.faint()),
            )
        } else {
            (raw.clone(), Style::default().fg(theme.text()))
        };

        let caret = if focused { "▏" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {caret} {label:<label_w$}"),
                if focused {
                    Style::default().fg(theme.accent())
                } else {
                    Style::default().fg(theme.faint())
                },
            ),
            Span::styled(text::truncate(&display, field_w), value_style),
        ]));
    }

    lines.push(Line::default());
    let key_hint = match (&app.cfg.api_key, &app.cfg.store_path) {
        // The form itself persists on save; say where, honestly.
        (Some(_), Some(path)) => format!("saved to {}", path.display()),
        (Some(_), None) => "key loaded from the environment".to_string(),
        (None, _) => "no key set - keyless endpoints still work".to_string(),
    };
    lines.push(Line::from(Span::styled(
        text::truncate(&key_hint, inner_w.saturating_sub(2)),
        theme.faint_style(),
    )));

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus()))
        .title(
            Line::from(Span::styled(
                " connect ",
                Style::default().fg(theme.accent()),
            ))
            .centered(),
        )
        .title_bottom(
            Line::from(Span::styled(
                " ↑↓ field · enter save · esc cancel ",
                theme.faint_style(),
            ))
            .right_aligned(),
        )
        .style(Style::default().bg(theme.panel_bg()));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Cache key for the status strip.
fn footer_key(app: &App) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        app.usage.prompt_tokens,
        app.usage.completion_tokens,
        app.usage.total_tokens,
        app.is_busy(),
        app.turn_started
            .map(|t| t.elapsed().as_millis() / 100)
            .unwrap_or(0),
        app.toast.as_ref().map(|(m, _)| m.as_str()).unwrap_or(""),
    )
}

/// Rows shown by `ctrl+x`: every slash command plus a few local actions.
pub fn command_items(app: &App) -> Vec<PickerItem> {
    let mut items: Vec<PickerItem> = crate::commands::COMMANDS
        .iter()
        .map(|entry| PickerItem {
            label: entry.command.to_string(),
            hint: entry.hint.to_string(),
            value: PickerValue::Command(entry.kind),
            current: false,
        })
        .collect();

    items.push(PickerItem {
        label: format!("Provider  ·  {}", app.provider_label()),
        hint: "/connect".into(),
        value: PickerValue::Command(Command::Connect),
        current: false,
    });
    items.push(PickerItem {
        label: format!("Model  ·  {}", app.model_label()),
        hint: "/model".into(),
        value: PickerValue::Model(app.cfg.model.clone()),
        current: false,
    });
    items.push(PickerItem {
        label: format!("Reasoning  ·  {}", app.cfg.reasoning.label()),
        hint: "/reasoning".into(),
        value: PickerValue::Action(Action::CycleReasoning),
        current: false,
    });
    items.push(PickerItem::action(
        "Interrupt run",
        "esc",
        Action::Interrupt,
    ));
    items.push(PickerItem::action("Quit", "ctrl+c", Action::Quit));
    items
}

/// opencode-style "connect a provider" menu: search box, Popular, Providers sections.
pub fn providers(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let Some(Overlay::Providers(ref state)) = app.overlay else {
        return;
    };

    let inner_w = area.width.saturating_sub(4) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;

    let rows = app.provider_menu_rows(state.filter.value());
    let query = state.filter.value().to_string();

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(
                format!(" {} ", glyph::PROMPT),
                Style::default().fg(theme.accent()),
            ),
            Span::styled(
                if query.is_empty() {
                    "search".to_string()
                } else {
                    query.clone()
                },
                if query.is_empty() {
                    theme.faint_style()
                } else {
                    theme.base()
                },
            ),
        ]),
        Line::default(),
    ];

    let mut row_lines: Vec<usize> = Vec::with_capacity(rows.len());
    let mut last_popular: Option<bool> = None;
    for (i, row) in rows.iter().enumerate() {
        if last_popular != Some(row.popular) {
            if lines.len() > 2 {
                lines.push(Line::default());
            }
            lines.push(Line::from(Span::styled(
                format!(" {} ", if row.popular { "Popular" } else { "Providers" }),
                Style::default()
                    .fg(theme.accent())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )));
            last_popular = Some(row.popular);
        }
        row_lines.push(lines.len());
        let check = if row.current { "✓" } else { " " };
        let label = format!("{check} {}", row.label);
        let hint = if row.hint.is_empty() {
            String::new()
        } else {
            format!("({})", row.hint)
        };
        if i == state.cursor {
            // Selected row: full-row highlight like opencode.
            let text = format!(" {label}  {hint}");
            let width = inner_w;
            let padded = format!("{:<width$}", text::truncate(&text, width), width = width);
            lines.push(Line::from(Span::styled(
                padded,
                Style::default().fg(theme.panel_bg()).bg(theme.accent()),
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled(label, Style::default().fg(theme.muted())),
                Span::styled(hint, Style::default().fg(theme.faint())),
            ]));
        }
    }

    let viewport = inner_h.saturating_sub(2); // two lines reserved for search

    // Handle empty filtered results gracefully
    if rows.is_empty() {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border_focus()))
            .title(
                Line::from(Span::styled(
                    " connect a provider ",
                    Style::default().fg(theme.accent()),
                ))
                .centered(),
            )
            .title_bottom(
                Line::from(Span::styled(
                    " \u{2191}\u{2193} move \u{b7} enter apply \u{b7} esc close ",
                    Style::default().fg(theme.faint()),
                ))
                .right_aligned(),
            )
            .style(Style::default().bg(theme.panel_bg()));
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no matching providers",
                theme.faint_style(),
            ))),
            inner,
        );
        return;
    }

    let cursor_line = row_lines.get(state.cursor).copied().unwrap_or(0);
    let top = if cursor_line < viewport {
        0
    } else {
        cursor_line - viewport + 1
    };
    let shown = lines
        .into_iter()
        .skip(top + 2)
        .take(viewport)
        .collect::<Vec<_>>();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus()))
        .title(
            Line::from(Span::styled(
                " connect a provider ",
                Style::default().fg(theme.accent()),
            ))
            .centered(),
        )
        .title_bottom(
            Line::from(Span::styled(
                " \u{2191}\u{2193} move \u{b7} enter apply \u{b7} esc close ",
                Style::default().fg(theme.faint()),
            ))
            .right_aligned(),
        )
        .style(Style::default().bg(theme.panel_bg()));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(shown), inner);
}

pub fn overlay_rect(area: Rect, kind: OverlayKind, content_rows: u16) -> Rect {
    let (w_pct, max_h) = match kind {
        OverlayKind::Picker => (72, 18),
        OverlayKind::Providers => (72, 22),
        OverlayKind::Connect => (70, 10),
        OverlayKind::Help => (76, 40),
    };
    let width = (area.width * w_pct / 100).clamp(40.min(area.width), 100.min(area.width));
    let height = (content_rows + 2).clamp(7.min(area.height), max_h.min(area.height));
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}
