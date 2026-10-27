use super::diff::DiffView;
use super::text;
use super::theme::{glyph, Theme};
use crate::app::{
    Action, App, BoardState, ImproveLevel, ImproveReview, ImproveState, Overlay, OverlayKind,
    PickerItem, PickerValue, CONNECT_FIELDS, IMPROVE_DETAIL_LINES,
};
use crate::commands::Command;
use crate::config::model_context_limit;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

/// Single-row header: brand · path | model · reasoning · ctx meter
pub fn header(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let width = area.width as usize;

    // Left: brand + workspace
    let mut left = vec![
        Span::styled(glyph::HEX, Style::default().fg(theme.accent())),
        Span::styled(
            " NANOMITE",
            Style::default()
                .fg(theme.accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" [{}]", app.theme.id.label()),
            Style::default()
                .fg(theme.accent())
                .add_modifier(Modifier::BOLD),
        ),
    ];
    let path_budget = width.saturating_sub(24).min(40);
    if path_budget >= 6 {
        let path = text::display_path(&app.cwd, path_budget);
        left.push(Span::styled(
            format!(" {} ", glyph::CHEVRON),
            Style::default().fg(theme.faint()),
        ));
        left.push(Span::styled(
            path,
            theme.faint_style().add_modifier(Modifier::DIM),
        ));
    }

    // Right: spinner/dot · model · reasoning · ctx meter
    let mark = if app.is_busy() {
        glyph::SPINNER[app.view.spinner % glyph::SPINNER.len()]
    } else {
        glyph::DOT
    };

    let ctx_limit = model_context_limit(&app.cfg.model).max(1);
    let usage = app.usage.total_tokens as usize;
    let ctx = usage.min(ctx_limit);
    let pct = (usage * 100 / ctx_limit).min(999);
    let ctx_str = text::format_tokens(ctx as u64);
    let limit_str = text::format_tokens(ctx_limit as u64);

    let meter_cells = 8usize;
    let filled = (ctx * meter_cells / ctx_limit).min(meter_cells);
    let meter = format!(
        "[{}{}]",
        glyph::BLOCK.repeat(filled),
        glyph::BLOCK_LIGHT.repeat(meter_cells - filled)
    );

    let ctx_display = format!(" {} {meter} {ctx_str}/{limit_str} · {pct}% ", glyph::BRAIN);
    let ctx_chars = ctx_display.chars().count();
    let fixed = glyph::DOT.width() + 2 * glyph::CHEVRON.width() + 2;
    let model_budget = width
        .saturating_sub(fixed + app.cfg.reasoning.label().len() + ctx_chars + 4)
        .max(8);
    let model = text::truncate_head(&app.model_label(), model_budget);

    let mut right = Line::from(vec![
        Span::styled(
            format!("{mark} "),
            if app.is_busy() {
                Style::default().fg(theme.accent())
            } else {
                Style::default().fg(theme.faint())
            },
        ),
        Span::styled(
            model,
            theme.dim().add_modifier(Modifier::DIM).fg(theme.accent()),
        ),
        Span::styled(
            format!(" {} ", glyph::CHEVRON),
            Style::default().fg(theme.faint()),
        ),
        Span::styled(
            app.cfg.reasoning.label(),
            theme
                .faint_style()
                .add_modifier(Modifier::DIM)
                .fg(theme.accent_soft()),
        ),
        Span::styled(
            format!(" {} ", glyph::CHEVRON),
            Style::default().fg(theme.faint()),
        ),
        Span::styled(ctx_display, theme.faint_style()),
    ])
    .right_aligned();

    // Combine left and right into one line. The right side must be budgeted
    // against what the left side already consumes - otherwise the context
    // meter gets clipped on narrow terminals.
    let mut spans = left;
    spans.push(Span::raw(" "));
    let left_width: usize = spans.iter().map(|s| s.content.width()).sum();
    let right_width: usize = right.spans.iter().map(|s| s.content.width()).sum();
    let right_budget = width.saturating_sub(left_width + 1);
    if right_width > right_budget {
        right.spans = truncate_spans_head(right.spans, right_width - right_budget);
    }
    spans.extend(right.spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Drops `drop` display columns from the front of a span list, keeping the
/// style of whatever survives. The tail of the header's right side carries
/// the absolute token readout, so truncation must eat the head, not the tail.
fn truncate_spans_head(spans: Vec<Span<'static>>, drop: usize) -> Vec<Span<'static>> {
    use unicode_width::UnicodeWidthChar;
    let mut remaining = drop;
    let mut out = Vec::with_capacity(spans.len());
    for span in spans {
        if remaining == 0 {
            out.push(span);
            continue;
        }
        let mut keep = String::new();
        for ch in span.content.chars() {
            let w = ch.width().unwrap_or(1);
            if remaining >= w && w > 0 {
                remaining -= w;
            } else {
                keep.push(ch);
            }
        }
        if !keep.is_empty() {
            out.push(Span::styled(keep, span.style));
        }
    }
    out
}

/// Hairline separator under the header - removed for compact mode
pub fn rule(frame: &mut Frame, area: Rect, theme: &Theme) {
    // Empty - header is now 1 row, no separator needed
    let _ = (frame, area, theme);
}

/// Status strip above the composer. Rebuilt only when its inputs change.
/// The footer badge for the active tool mode: `(label, color)` for every
/// non-default mode; `None` for build (the default stays quiet).
fn mode_badge(app: &App) -> Option<(String, Color)> {
    use crate::agent::policy::ToolPolicy;
    let theme = &app.theme;
    match app.cfg.tool_policy {
        ToolPolicy::Plan => Some(("◐ plan mode".to_string(), theme.warn())),
        ToolPolicy::Coder => Some((format!("{} coder mode", glyph::CHECK), theme.accent())),
        ToolPolicy::Build => None,
        ToolPolicy::Hacker => Some((format!("{} hacker mode", glyph::BRAIN), theme.danger())),
    }
}

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
    if app.is_busy() {
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
    } else {
        left.push(Span::styled(
            format!("{} ready", glyph::DOT),
            Style::default().fg(theme.faint()),
        ));
        // Quiet one-time note when persistent memory is not attached. It
        // sits next to "ready" rather than in the toast slot: an absent or
        // unwritable memory directory is a normal state, not a failure.
        if let Some(hint) = app.memory_hint().or_else(|| app.prompt_hint()) {
            left.push(Span::styled(format!("  {hint}"), theme.faint_style()));
        }
        if !app.view.follow {
            // `end` (or clicking the scrollbar's bottom) re-follows the live tail.
            left.push(Span::styled(
                format!("  {} scrolled up (end)", glyph::ARROW_UP),
                Style::default().fg(theme.warn()),
            ));
        }
    }

    // Show toast if present
    if let Some((toast_msg, _)) = &app.toast {
        left.push(Span::styled("  ", theme.faint_style()));
        left.push(Span::styled(
            toast_msg.clone(),
            Style::default().fg(theme.warn()),
        ));
    }

    // Mode badge: visible for every non-default mode (build is the default
    // and stays quiet). Distinct color + glyph per mode so the mode is
    // unmistakable at a glance.
    if let Some((badge, color)) = mode_badge(app) {
        left.push(Span::styled("  ", theme.faint_style()));
        left.push(Span::styled(
            badge,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
        if app.cfg.tool_policy == crate::agent::policy::ToolPolicy::Plan {
            if let Some(latest) = app.latest_plan_name() {
                left.push(Span::styled(format!(" · {latest}"), theme.faint_style()));
            }
        }
    }

    // Visual selection indicator
    if let Some(sel) = &app.selection {
        if let Some(mode) = sel.mode {
            let mode_label = match mode {
                crate::app::SelectionMode::Char => "VISUAL",
                crate::app::SelectionMode::Line => "VISUAL LINE",
                crate::app::SelectionMode::Block => "VISUAL BLOCK",
            };
            left.push(Span::styled("  ", theme.faint_style()));
            left.push(Span::styled(
                format!("-- {mode_label} --"),
                Style::default()
                    .fg(theme.accent())
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }

    // Right side: prompt / completion token counts only (context meter moved to header)
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
    ])
    .right_aligned();

    frame.render_widget(Paragraph::new(Line::from(left.clone())), area);
    frame.render_widget(Paragraph::new(right), area);

    app.view.footer = Some((key, Line::from(left)));
}

/// Keys table shown at the bottom of the sheet. Cross-checked against
/// `App::on_key` (src/app.rs): every row here describes a binding that key
/// handling actually implements, and every binding handled there appears
/// below. Plain `home`/`end` move the prompt caret; only `ctrl+home`/`ctrl+end`
/// jump the transcript.
pub fn help_keys() -> &'static [(&'static str, &'static str)] {
    &[
        (
            "ctrl+c / ctrl+d",
            "quit (ctrl+d deletes forward when text exists)",
        ),
        (
            "enter",
            "send · run the highlighted command · open the highlighted /memory record",
        ),
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
        (
            "backspace / delete",
            "delete the char before · after the caret",
        ),
        ("left / right", "move the caret · by word with ctrl or alt"),
        ("up / down", "cursor within a multi-row prompt, else scroll"),
        ("pgup / pgdn", "scroll the transcript"),
        ("home / end", "prompt start · prompt end"),
        ("ctrl+home / ctrl+end", "transcript top · bottom"),
        ("mouse wheel", "scroll the transcript"),
        (
            "tab",
            "next picker row · next /connect field (shift+tab back)",
        ),
        (
            "a / r",
            "approve · reject the self-edit under review in /improve",
        ),
        ("f1", "this sheet"),
    ]
}

/// Keyboard sheet: command registry first, then keybindings.
pub fn help(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let offset = match &app.overlay {
        Some(Overlay::Help(state)) => state.offset,
        _ => 0,
    };
    let inner_width = area.width.saturating_sub(4) as usize;
    let keys = help_keys();

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
    // Keep the highlighted row near the middle of the window (clamped at both
    // ends) instead of pinned to the bottom edge - otherwise a model sorted
    // deep in the endpoint's list opens as a frozen wall of rows above the
    // highlight with nothing visible below it.
    let start = if picker.items.len() > visible {
        picker
            .cursor
            .saturating_sub(visible / 2)
            .min(picker.items.len() - visible)
    } else {
        0
    };
    if picker.loading {
        lines.push(Line::from(Span::styled(
            if picker.mode == crate::app::PickerMode::Memory {
                "  loading memories…"
            } else {
                "  fetching models…"
            },
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
        let check = if item.current { glyph::CHECK } else { " " };
        let label = format!("{check} {}", item.label);
        // The label is never squeezed: on a tight row the hint gives up its head
        // (the host) and keeps its tail (the key name) instead.
        let hint = text::truncate_head(
            &item.hint,
            inner_w.saturating_sub(3 + text::width(&label) + 2),
        );
        let label_w = inner_w.saturating_sub(3 + text::width(&hint));
        let label_style = if selected {
            Style::default().fg(theme.strong())
        } else if item.current {
            Style::default().fg(theme.accent_soft())
        } else {
            theme.dim()
        };

        // Highlight query matches in label and hint
        let highlight_style = Style::default()
            .fg(theme.accent())
            .add_modifier(Modifier::BOLD);
        let label_spans = if query.is_empty() {
            vec![Span::styled(text::truncate(&label, label_w), label_style)]
        } else {
            text::highlight_matches(&label, &query, highlight_style)
                .into_iter()
                .map(|mut s| {
                    s.style = s.style.patch(label_style);
                    s
                })
                .collect()
        };
        let hint_spans = if query.is_empty() {
            vec![Span::styled(hint, theme.faint_style())]
        } else {
            text::highlight_matches(&hint, &query, Style::default().fg(theme.accent()))
        };

        // Build label spans with truncation
        let mut label_line_spans = Vec::new();
        let mut current_width = 0;
        for span in &label_spans {
            let span_width = text::width(&span.content);
            if current_width + span_width <= label_w {
                label_line_spans.push(Span::styled(
                    span.content.clone(),
                    span.style.patch(label_style),
                ));
                current_width += span_width;
            } else {
                // Truncate this span
                let mut remaining = label_w - current_width;
                let mut truncated = String::new();
                for ch in span.content.chars() {
                    let cw = text::width(&ch.to_string());
                    if remaining < cw {
                        break;
                    }
                    truncated.push(ch);
                    remaining -= cw;
                }
                if !truncated.is_empty() {
                    label_line_spans.push(Span::styled(truncated, span.style.patch(label_style)));
                }
                break;
            }
        }
        if label_line_spans.is_empty() {
            label_line_spans.push(Span::styled(String::new(), label_style));
        }

        // Build hint spans
        let mut hint_line_spans = Vec::new();
        for span in &hint_spans {
            hint_line_spans.push(span.clone());
        }

        lines.push(Line::from(
            vec![Span::styled(
                format!(" {marker} "),
                Style::default().fg(theme.accent()),
            )]
            .into_iter()
            .chain(label_line_spans)
            .chain(hint_line_spans)
            .collect::<Vec<_>>(),
        ));
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus()))
        .title(
            Line::from(Span::styled(
                // The live row count (and the filtered share, once a query is
                // typed) makes it obvious the list came from the endpoint just
                // now instead of being a fixed table.
                if picker.loading || picker.source.is_empty() {
                    format!(" {} ", picker.title())
                } else if picker.items.len() == picker.source.len() {
                    format!(" {} · {} ", picker.title(), picker.source.len())
                } else {
                    format!(
                        " {} · {}/{} ",
                        picker.title(),
                        picker.items.len(),
                        picker.source.len()
                    )
                },
                Style::default().fg(theme.accent()),
            ))
            .centered(),
        )
        .title_bottom(
            Line::from(Span::styled(
                format!(
                    " {}{} move {} enter apply {} esc close ",
                    glyph::ARROW_UP,
                    glyph::ARROW_DOWN,
                    glyph::DOT,
                    glyph::DOT
                ),
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
                    glyph::BULLET.repeat(raw.chars().count().saturating_sub(4)),
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

        let caret = if focused { glyph::CARET } else { " " };
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
                format!(
                    " {}{} field {} enter save {} esc cancel ",
                    glyph::ARROW_UP,
                    glyph::ARROW_DOWN,
                    glyph::DOT,
                    glyph::DOT
                ),
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
        // The mode badge changes the footer content; without this in the key
        // a cached build-mode footer would render after a mode switch.
        app.cfg.tool_policy.as_str(),
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
        let check = if row.current { glyph::CHECK } else { " " };
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
            // Highlight query matches in label and hint
            if !query.is_empty() {
                let highlight_style = Style::default()
                    .fg(theme.accent())
                    .add_modifier(Modifier::BOLD);
                let label_spans = text::highlight_matches(&label, &query, highlight_style);
                let hint_spans =
                    text::highlight_matches(&hint, &query, Style::default().fg(theme.accent()));

                let mut label_line_spans = Vec::new();
                let mut current_width = 0;
                for span in &label_spans {
                    let span_width = text::width(&span.content);
                    if current_width + span_width <= inner_w.saturating_sub(3 + text::width(&hint))
                    {
                        label_line_spans.push(Span::styled(span.content.clone(), span.style));
                        current_width += span_width;
                    } else {
                        let mut remaining =
                            inner_w.saturating_sub(3 + text::width(&hint)) - current_width;
                        let mut truncated = String::new();
                        for ch in span.content.chars() {
                            let cw = text::width(&ch.to_string());
                            if remaining < cw {
                                break;
                            }
                            truncated.push(ch);
                            remaining -= cw;
                        }
                        if !truncated.is_empty() {
                            label_line_spans.push(Span::styled(truncated, span.style));
                        }
                        break;
                    }
                }
                if label_line_spans.is_empty() {
                    label_line_spans.push(Span::styled(
                        String::new(),
                        Style::default().fg(theme.muted()),
                    ));
                }

                let mut hint_line_spans = Vec::new();
                for span in &hint_spans {
                    hint_line_spans.push(span.clone());
                }

                lines.push(Line::from(
                    vec![Span::styled("  ", Style::default().fg(theme.muted()))]
                        .into_iter()
                        .chain(label_line_spans)
                        .chain(hint_line_spans)
                        .collect::<Vec<_>>(),
                ));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(label, Style::default().fg(theme.muted())),
                    Span::styled(hint, Style::default().fg(theme.faint())),
                ]));
            }
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
                    format!(
                        " {}{} move {} enter apply {} esc close ",
                        glyph::ARROW_UP,
                        glyph::ARROW_DOWN,
                        glyph::DOT,
                        glyph::DOT
                    ),
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
                format!(
                    " {}{} move {} enter apply {} esc close ",
                    glyph::ARROW_UP,
                    glyph::ARROW_DOWN,
                    glyph::DOT,
                    glyph::DOT
                ),
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

// ---------------------------------------------------------------------------
// /tools — the tool library
// ---------------------------------------------------------------------------

/// Content rows of the `/tools` sheet: one line per tool, one header per
/// category group, one summary line. Drives overlay sizing.
pub fn tools_row_count() -> u16 {
    let catalog = crate::tools::ToolSet::catalog();
    let mut groups = 0usize;
    let mut last: Option<crate::tools::catalog::ToolCategory> = None;
    for spec in &catalog {
        if last != Some(spec.category) {
            groups += 1;
            last = Some(spec.category);
        }
    }
    // External tools render as one extra group (header + summary) when present.
    let external = external_tool_names().len();
    let external_rows = if external > 0 { external + 1 } else { 0 };
    (catalog.len() + groups + 1 + external_rows) as u16
}

/// External tool names for the /tools overlay, from the manifest dir.
fn external_tool_names() -> Vec<String> {
    crate::tools::external_tools::ExternalTools::load(&crate::tools::external_tools::tools_dir())
        .names()
}

/// `/tools` tool library: the catalog grouped by category, each row showing
/// name, one-line description and an on/off marker driven by
/// `cfg.disabled_tools`. `enter` toggles, `esc` closes.
pub fn tools(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let Some(Overlay::Tools(ref state)) = app.overlay else {
        return;
    };

    let inner_w = area.width.saturating_sub(4) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;

    let catalog = crate::tools::ToolSet::catalog();
    let is_disabled = |name: &str| app.cfg.disabled_tools.iter().any(|t| t == name);
    let enabled = catalog.iter().filter(|s| !is_disabled(s.name)).count();
    let name_w = catalog.iter().map(|s| s.name.len()).max().unwrap_or(10);

    // Summary line first, then category headers interleaved with tool rows.
    // `row_line[i]` is the display line of catalog row `i`, so the window can
    // follow the cursor.
    let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled(
        format!(
            "  {} tools · {enabled} enabled · enter toggles",
            catalog.len()
        ),
        theme.faint_style(),
    ))];
    let mut row_line: Vec<usize> = Vec::with_capacity(catalog.len());
    let mut last: Option<crate::tools::catalog::ToolCategory> = None;
    for (i, spec) in catalog.iter().enumerate() {
        if last != Some(spec.category) {
            lines.push(Line::from(Span::styled(
                format!(
                    " {}",
                    match spec.category {
                        crate::tools::catalog::ToolCategory::Files => "files",
                        crate::tools::catalog::ToolCategory::Exec => "exec",
                        crate::tools::catalog::ToolCategory::Web => "web",
                        crate::tools::catalog::ToolCategory::Memory => "memory",
                        crate::tools::catalog::ToolCategory::Document => "document",
                        crate::tools::catalog::ToolCategory::SelfImprove => "self-improve",
                        crate::tools::catalog::ToolCategory::Git => "git",
                        crate::tools::catalog::ToolCategory::Test => "test",
                        crate::tools::catalog::ToolCategory::Index => "index",
                        crate::tools::catalog::ToolCategory::Plan => "plan",
                    }
                ),
                Style::default()
                    .fg(theme.accent())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )));
            last = Some(spec.category);
        }
        row_line.push(lines.len());
        let selected = i == state.cursor;
        let disabled = is_disabled(spec.name);
        let marker = if disabled { glyph::CROSS } else { glyph::CHECK };
        let marker_style = if disabled {
            Style::default().fg(theme.danger())
        } else {
            Style::default().fg(theme.success())
        };
        let cursor_marker = if selected { glyph::GUTTER } else { " " };
        let desc_budget = inner_w.saturating_sub(6 + name_w + 2).max(12);
        let desc = text::truncate_head(spec.description, desc_budget);
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {cursor_marker} "),
                Style::default().fg(theme.accent()),
            ),
            Span::styled(format!("{marker} "), marker_style),
            Span::styled(
                format!("{:<name_w$}", spec.name),
                if selected {
                    Style::default().fg(theme.strong())
                } else {
                    theme.dim()
                },
            ),
            Span::styled(
                format!("  {desc}"),
                if selected {
                    theme.dim()
                } else {
                    theme.faint_style()
                },
            ),
        ]));
    }

    // External tools: one "external" group with manifest-loaded tools.
    let external = external_tool_names();
    if !external.is_empty() {
        lines.push(Line::from(Span::styled(
            " external",
            Style::default()
                .fg(theme.accent())
                .add_modifier(Modifier::BOLD),
        )));
        for name in &external {
            let disabled = is_disabled(name);
            let marker = if disabled { glyph::CROSS } else { glyph::CHECK };
            let marker_style = if disabled {
                Style::default().fg(theme.danger())
            } else {
                Style::default().fg(theme.success())
            };
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(format!("{marker} "), marker_style),
                Span::styled(
                    name.clone(),
                    if disabled {
                        theme.dim()
                    } else {
                        Style::default().fg(theme.strong())
                    },
                ),
                Span::styled("  from tools/*.toml", theme.faint_style()),
            ]));
        }
    }

    // Keep the highlighted row inside the viewport.
    let selected_line = row_line
        .get(state.cursor)
        .copied()
        .unwrap_or(lines.len().saturating_sub(1));
    let visible = inner_h.max(1);
    let start = selected_line
        .saturating_sub(visible.saturating_sub(1))
        .min(selected_line);
    let window: Vec<Line<'static>> = lines.into_iter().skip(start).take(visible).collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus()))
        .title(Line::from(Span::styled(" tools ", Style::default().fg(theme.accent()))).centered())
        .title_bottom(
            Line::from(Span::styled(
                format!(
                    " {}{} move {} enter toggle {} esc close ",
                    glyph::ARROW_UP,
                    glyph::ARROW_DOWN,
                    glyph::DOT,
                    glyph::DOT
                ),
                theme.faint_style(),
            ))
            .right_aligned(),
        )
        .style(Style::default().bg(theme.panel_bg()));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(window), inner);
}

// ---------------------------------------------------------------------------
// /panel — the agent message board
// ---------------------------------------------------------------------------

/// Hard cap on rendered board lines; the oldest are trimmed with a marker.
const BOARD_MAX_LINES: usize = 400;

/// Deterministic accent per persona: a fixed rotation over theme roles.
fn persona_color(theme: &Theme, persona: &str) -> Color {
    match persona {
        "Architect" => theme.accent(),
        "Engineer" => theme.success(),
        "Scientist" => theme.warn(),
        "Critic" => theme.danger(),
        "Chair" => theme.strong(),
        "topic" | "system" => theme.faint(),
        _ => theme.muted(),
    }
}

/// Renders a board thread into display lines, oldest first. Bodies are
/// untrusted model text: plain styled spans through `wrap_spans`, wrapped and
/// indented, never interpreted. Capped at [`BOARD_MAX_LINES`] with an elision
/// marker for the oldest rows.
fn board_lines(theme: &Theme, state: &BoardState, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, message) in state.messages.iter().enumerate() {
        let color = persona_color(theme, &message.persona);
        lines.push(Line::from(Span::styled(
            format!("  {} (round {}):", message.persona, message.round),
            Style::default().fg(color),
        )));
        // Wrap width leaves room for the indent column.
        let budget = width.saturating_sub(6).max(10);
        for raw in message.body.lines() {
            for wrapped in text::wrap_spans(vec![Span::raw(raw.to_string())], budget) {
                let mut spans = Vec::with_capacity(wrapped.spans.len() + 1);
                spans.push(Span::raw("     ".to_string()));
                spans.extend(wrapped.spans);
                lines.push(Line::from(spans));
            }
        }
        if i + 1 < state.messages.len() {
            lines.push(Line::default());
        }
    }
    if lines.len() > BOARD_MAX_LINES {
        lines.drain(..lines.len() - BOARD_MAX_LINES);
        lines.insert(
            0,
            Line::from(Span::styled(
                "  … older messages elided",
                theme.faint_style(),
            )),
        );
    }
    lines
}

/// Rendered board rows (after wrap, elide, plus the status row) — used to size
/// the overlay, which is why the width is the overlay's nominal inner width.
pub fn board_row_count(app: &App) -> u16 {
    let Some(Overlay::Board(ref state)) = app.overlay else {
        return 16;
    };
    let nominal_width = 84usize;
    let rows = board_lines(&app.theme, state, nominal_width.saturating_sub(4)).len();
    (rows as u16 + 1).clamp(8, 36)
}

/// `/panel` board overlay: one thread while it convenes. The topic is the
/// block title, messages are persona-headed blocks, and the status line at the
/// bottom carries the spinner plus the esc hint.
pub fn board(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let Some(Overlay::Board(ref state)) = app.overlay else {
        return;
    };

    let inner_w = area.width.saturating_sub(4) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;

    let mut lines = board_lines(theme, state, inner_w);
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  waiting for the first reply…",
            theme.faint_style(),
        )));
    }

    // Scroll: `offset` hides rows from the bottom, so Up pages toward older
    // messages and the thread stays pinned to the live tail by default.
    let visible = inner_h.saturating_sub(1).max(1);
    let offset = state.offset.min(lines.len().saturating_sub(1));
    let end = lines.len().saturating_sub(offset);
    let start = end.saturating_sub(visible);
    let window: Vec<Line<'static>> = lines.into_iter().skip(start).take(visible).collect();

    // Pinned status row: convening spinner or done mark, then the esc hint.
    let round = state.messages.iter().map(|m| m.round).max().unwrap_or(0);
    let mut status: Vec<Span<'static>> = Vec::new();
    if state.running {
        status.push(Span::styled(
            format!(
                "{} ",
                glyph::SPINNER[app.view.spinner % glyph::SPINNER.len()]
            ),
            Style::default().fg(theme.accent()),
        ));
        status.push(Span::styled(
            format!("convening… round {round}"),
            theme.dim(),
        ));
        status.push(Span::styled(" · esc cancel", theme.faint_style()));
    } else {
        status.push(Span::styled(
            format!("{} convened", glyph::CHECK),
            Style::default().fg(theme.success()),
        ));
        status.push(Span::styled(" · esc close", theme.faint_style()));
    }
    if state.in_memory {
        status.push(Span::styled(" · in-memory store", theme.faint_style()));
    }

    let esc_hint = if state.running {
        "esc cancel"
    } else {
        "esc close"
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus()))
        .title(
            Line::from(Span::styled(
                format!(
                    " panel · {} ",
                    text::truncate(&state.topic, inner_w.saturating_sub(12).max(8))
                ),
                Style::default().fg(theme.accent()),
            ))
            .centered(),
        )
        .title_bottom(
            Line::from(Span::styled(
                format!(
                    " {}{} scroll {} {esc_hint} ",
                    glyph::ARROW_UP,
                    glyph::ARROW_DOWN,
                    glyph::DOT
                ),
                theme.faint_style(),
            ))
            .right_aligned(),
        )
        .style(Style::default().bg(theme.panel_bg()));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(window), inner);
    let status_rect = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    frame.render_widget(Paragraph::new(Line::from(status)), status_rect);
}

/// `/memory` record view: read-only body of one stored memory, opened from the
/// picker with `enter`. Metadata first, then the wrapped content.
pub fn memory(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let Some(Overlay::Memory(ref view)) = app.overlay else {
        return;
    };
    let record = &view.record;

    let inner_w = area.width.saturating_sub(4) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;

    let mut meta: Vec<Span<'static>> = vec![
        Span::styled(
            format!(" {} ", glyph::PROMPT),
            Style::default().fg(theme.accent()),
        ),
        Span::styled(
            text::truncate(&record.title, inner_w.saturating_sub(4).max(8)),
            theme.strong(),
        ),
    ];
    meta.push(Span::styled(" · ", theme.faint_style()));
    meta.push(Span::styled(record.category.clone(), theme.accent_soft()));
    if let Some(project) = &record.project {
        meta.push(Span::styled(format!(" · {project}"), theme.dim()));
    }
    for tag in &record.tags {
        meta.push(Span::styled(format!(" #{tag}"), theme.faint_style()));
    }
    meta.push(Span::styled(
        format!(" · {}", record.age_label()),
        theme.faint_style(),
    ));
    meta.push(Span::styled(
        format!(" · {}", record.id),
        theme.faint_style(),
    ));

    let mut lines = vec![Line::from(meta), Line::default()];
    let body: Vec<Span<'static>> = vec![Span::styled(record.content.clone(), theme.base())];
    lines.extend(text::wrap_spans(body, inner_w.max(8)));
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!(
            "{} unread \u{b7} stored memory is untrusted data",
            record.content.len()
        ),
        theme.faint_style(),
    )));

    // `offset` hides rows from the top, so Up moves toward older body text.
    let offset = view.offset.min(lines.len().saturating_sub(1));
    let window: Vec<Line<'static>> = lines.into_iter().skip(offset).take(inner_h).collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus()))
        .title(
            Line::from(Span::styled(
                " memory ",
                Style::default().fg(theme.accent()),
            ))
            .centered(),
        )
        .title_bottom(
            Line::from(Span::styled(
                format!(
                    " {}{} scroll {} esc close ",
                    glyph::ARROW_UP,
                    glyph::ARROW_DOWN,
                    glyph::DOT
                ),
                theme.faint_style(),
            ))
            .right_aligned(),
        )
        .style(Style::default().bg(theme.panel_bg()));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(window), inner);
}

// ---------------------------------------------------------------------------
// /improve — self-edit proposals
// ---------------------------------------------------------------------------

/// Rows the `/improve` sheet wants: summary, one line per proposal, a status
/// line. Drives overlay sizing.
pub fn improve_row_count(app: &App) -> u16 {
    let Some(Overlay::Improve(ref state)) = app.overlay else {
        return 16;
    };
    // In review the diff owns the height; the list is bounded and short.
    if state.review.is_some() {
        return 30;
    }
    ((state.rows.len() as u16) + 3).clamp(6, 24)
}

/// `/improve`: the pending self-edit proposals, or the review view of one of
/// them. Nothing here applies anything - `a`/`r` only exist in the review view
/// and the footer says so.
pub fn improve(frame: &mut Frame, area: Rect, app: &App) {
    let Some(Overlay::Improve(ref state)) = app.overlay else {
        return;
    };
    match &state.review {
        Some(review) => improve_review(frame, area, app, state, review),
        None => improve_list(frame, area, app, state),
    }
}

/// The list half: id, target, path, size, rationale and timestamp per row, with
/// the cursor on the row `enter` would open.
fn improve_list(frame: &mut Frame, area: Rect, app: &App, state: &ImproveState) {
    let theme = &app.theme;
    let inner_w = area.width.saturating_sub(4) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();
    if state.loading {
        lines.push(Line::from(Span::styled(
            "  loading proposals…",
            theme.faint_style(),
        )));
    } else if state.rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no pending proposals - ask the agent to propose a self-edit",
            theme.faint_style(),
        )));
    }
    let rationale_w = inner_w.saturating_sub(34).max(12);
    for (i, row) in state.rows.iter().enumerate() {
        let selected = i == state.cursor;
        let cursor_marker = if selected { glyph::GUTTER } else { " " };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {cursor_marker} "),
                Style::default().fg(theme.accent()),
            ),
            Span::styled(
                format!("{:<8}", row.target),
                if selected {
                    Style::default().fg(theme.accent())
                } else {
                    theme.dim()
                },
            ),
            Span::styled(
                text::truncate_head(&row.path, inner_w.saturating_sub(12).max(16)),
                if selected {
                    Style::default().fg(theme.strong())
                } else {
                    theme.dim()
                },
            ),
            Span::styled(
                format!("  +{} −{}", row.added, row.removed),
                theme.faint_style(),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("       ", theme.faint_style()),
            Span::styled(
                format!("{} ", text::truncate(&row.rationale, rationale_w)),
                theme.faint_style(),
            ),
            Span::styled(format!("· {}", row.created_at), theme.faint_style()),
        ]));
    }

    let visible = inner_h.saturating_sub(status_rows(state) as usize).max(1);
    let window: Vec<Line<'static>> = lines.into_iter().take(visible).collect();
    improve_block(
        frame,
        area,
        theme,
        " improve ",
        " ↑↓ move · enter review · esc close ",
        window,
        status_line(state, theme, false),
    );
}

/// The review half: metadata header plus the proposal's unified diff, styled by
/// [`crate::ui::diff::DiffView`] so `+`/`-`/`@@` read exactly as elsewhere.
fn improve_review(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    state: &ImproveState,
    review: &ImproveReview,
) {
    let theme = &app.theme;
    let inner_w = area.width.saturating_sub(4) as usize;

    let diff_view = DiffView::with_theme(*theme);
    let mut lines: Vec<Line<'static>> = vec![Line::from(vec![
        Span::styled(
            format!(" {} ", glyph::PROMPT),
            Style::default().fg(theme.accent()),
        ),
        Span::styled(
            text::truncate(&review.path, inner_w.saturating_sub(24).max(16)),
            theme.strong(),
        ),
        Span::styled(format!(" · {} ", review.target), theme.accent_soft()),
        Span::styled(
            format!("· +{} −{} · {}", review.added, review.removed, review.id),
            theme.faint_style(),
        ),
    ])];
    lines.push(Line::from(Span::styled(
        format!("  {}", text::truncate(&review.rationale, inner_w.max(12))),
        theme.dim(),
    )));
    lines.push(Line::default());
    // The diff is styled by `DiffView` itself, so `+`/`-`/`@@` read exactly as
    // they do in a transcript; the text is owned because the view is dropped.
    lines.extend(
        review
            .lines
            .iter()
            .map(|line| Line::from(Span::styled(line.clone(), diff_view.style_for(line)))),
    );

    // `offset` hides rows from the top, so Up moves down the diff.
    let offset = state.offset.min(lines.len().saturating_sub(1));
    let visible = (area.height.saturating_sub(2 + status_rows(state)).max(1)) as usize;
    let window: Vec<Line<'static>> = lines.into_iter().skip(offset).take(visible).collect();
    improve_block(
        frame,
        area,
        theme,
        &format!(" review · {} ", text::truncate(&review.path, 40)),
        " a approve · r reject · ↑↓ scroll · esc back ",
        window,
        status_line(state, theme, true),
    );
}

/// One bottom line: the in-flight verify note, or the outcome of the last
/// decision, colored by severity.
fn status_line<'a>(state: &ImproveState, theme: &Theme, reviewing: bool) -> Option<Line<'a>> {
    let Some(status) = &state.status else {
        return None;
    };
    let style = match status.level {
        ImproveLevel::Info => theme.dim(),
        ImproveLevel::Success => Style::default().fg(theme.success()),
        ImproveLevel::Warn => Style::default().fg(theme.warn()),
        ImproveLevel::Danger => Style::default().fg(theme.danger()),
    };
    let marker = if state.applying.is_some() && reviewing {
        format!("{} ", glyph::SPINNER[0])
    } else {
        match status.level {
            ImproveLevel::Success => format!("{} ", glyph::CHECK),
            ImproveLevel::Danger => format!("{} ", glyph::CROSS),
            _ => format!("{} ", glyph::DOT),
        }
    };
    Some(Line::from(vec![
        Span::styled(marker, style),
        Span::styled(status.message.clone(), style),
    ]))
}

/// Lines the status note occupies at the bottom of the sheet.
fn status_rows(state: &ImproveState) -> u16 {
    let Some(status) = &state.status else {
        return 0;
    };
    (status.message.lines().count() as u16).clamp(1, IMPROVE_DETAIL_LINES as u16)
}

/// Shared chrome for both halves of the overlay.
fn improve_block(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: &str,
    keys: &str,
    window: Vec<Line<'static>>,
    status: Option<Line<'static>>,
) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus()))
        .title(
            Line::from(Span::styled(
                title.to_string(),
                Style::default().fg(theme.accent()),
            ))
            .centered(),
        )
        .title_bottom(
            Line::from(Span::styled(format!(" {keys} "), theme.faint_style())).right_aligned(),
        )
        .style(Style::default().bg(theme.panel_bg()));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let body_h = inner.height.saturating_sub(match &status {
        Some(_) => 1,
        None => 0,
    });
    frame.render_widget(
        Paragraph::new(window),
        Rect {
            height: body_h,
            ..inner
        },
    );
    if let Some(status) = status {
        frame.render_widget(
            Paragraph::new(status),
            Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(1),
                width: inner.width,
                height: 1,
            },
        );
    }
}

pub fn overlay_rect(area: Rect, kind: OverlayKind, content_rows: u16) -> Rect {
    let (w_pct, max_h) = match kind {
        OverlayKind::Picker => (72, 18),
        OverlayKind::Providers => (72, 22),
        OverlayKind::Connect => (70, 10),
        OverlayKind::Help => (76, 40),
        OverlayKind::Tools => (76, 48),
        OverlayKind::Board => (84, 40),
        OverlayKind::Memory => (76, 40),
        OverlayKind::Improve => (80, 34),
        OverlayKind::SudoPassword => (60, 12),
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

// ---------------------------------------------------------------------------
// /sudo — sudo password input overlay
// ---------------------------------------------------------------------------

/// Rows for the sudo password overlay.
pub fn sudo_password_row_count(_app: &App) -> u16 {
    10
}

/// Modal for entering the sudo password when a command requires elevation.
/// Styled exactly like the other overlays: Clear backdrop, rounded border in
/// `border_focus`, centered accent title, key hints bottom-right, `panel_bg`.
pub fn sudo_password(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let Some(Overlay::SudoPassword(ref state)) = app.overlay else {
        return;
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus()))
        .title(
            Line::from(Span::styled(
                " sudo password required ",
                Style::default()
                    .fg(theme.accent())
                    .add_modifier(Modifier::BOLD),
            ))
            .centered(),
        )
        .title_bottom(
            Line::from(Span::styled(
                " enter submit \u{b7} esc cancel ",
                theme.faint_style(),
            ))
            .right_aligned(),
        )
        .style(Style::default().bg(theme.panel_bg()));

    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let label_w = 9; // "password " is the widest label
    let mut lines: Vec<Line> = Vec::with_capacity(6);

    // The command that needs elevation - bounded to the panel width.
    let cmd_w = (inner.width as usize).saturating_sub(label_w + 3).max(8);
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<label_w$}", "command"),
            Style::default().fg(theme.faint()),
        ),
        Span::styled(
            text::truncate(&state.command, cmd_w),
            Style::default().fg(theme.strong()),
        ),
    ]));
    lines.push(Line::from(""));

    // Masked input with the theme's block cursor glyph at the typing position.
    let mut pw_spans = vec![Span::styled(
        format!("  {:<label_w$}", "password"),
        Style::default().fg(theme.faint()),
    )];
    let shown = state.password.chars().count();
    if state.submitting {
        pw_spans.push(Span::styled(
            glyph::SPINNER[app.view.spinner % glyph::SPINNER.len()],
            Style::default().fg(theme.accent()),
        ));
        pw_spans.push(Span::styled(
            " authenticating...",
            Style::default().fg(theme.faint()),
        ));
    } else {
        pw_spans.push(Span::styled(
            glyph::BULLET.repeat(shown),
            Style::default().fg(theme.accent()),
        ));
        pw_spans.push(Span::styled(
            glyph::CURSOR,
            Style::default().fg(theme.accent()),
        ));
    }
    lines.push(Line::from(pw_spans));
    lines.push(Line::from(""));

    // Status row: hint, error, or in-flight note.
    if let Some(err) = &state.error {
        lines.push(Line::from(Span::styled(
            format!("  \u{2717} {err}"),
            Style::default().fg(theme.danger()),
        )));
    } else if state.submitting {
        lines.push(Line::from(Span::styled(
            "  validating password with sudo...",
            Style::default().fg(theme.warn()),
        )));
    } else if shown == 0 {
        lines.push(Line::from(Span::styled(
            "  type your sudo password - it never leaves this machine",
            Style::default().fg(theme.faint()),
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);

    // Terminal cursor at the password typing position (after the bullets).
    let password_x = inner.x + 2 + label_w as u16 + shown.min(60) as u16;
    let password_y = inner.y + 2;
    if !state.submitting {
        frame.set_cursor_position((password_x, password_y));
    }
}
