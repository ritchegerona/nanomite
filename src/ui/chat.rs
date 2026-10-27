use super::diff::DiffView;
use super::text::{self, wrap_spans};
use super::theme::{glyph, Theme};
use crate::app::{Entry, EntryRole, Phase, Rendered};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Scrollbar, ScrollbarOrientation};
use ratatui::Frame;

/// Width of the left gutter reserved for role badges.
pub const GUTTER: usize = 4;

/// Transcripts up to this many entries get measured exactly before drawing.
pub const WARM_LIMIT: usize = 400;

/// Maximum columns for the tool detail when `/details` is off (compact mode).
pub const COMPACT_TOOL_DETAIL_WIDTH: usize = 40;

/// Glyphs for compact tool status display.
pub mod tool_glyph {
    pub const RUNNING: &str = "⟳";
    pub const SUCCESS: &str = "✓";
    pub const FAILURE: &str = "✗";
    pub const DIFF_ADD: &str = "+";
    pub const DIFF_DEL: &str = "−";
}

pub fn render_prompt_queue(frame: &mut Frame, area: Rect, app: &crate::app::App) {
    let queue_len = app.prompt_queue_len();
    if queue_len == 0 {
        return;
    }
    let theme = app.theme;

    // Background-style strip, deliberately quiet: no borders, no panel, no
    // bright colours. It reads like a margin note listing what is waiting,
    // so the user knows the messages are held and will run after the current
    // turn - without competing with the transcript for attention.
    let mut lines = vec![Line::from(Span::styled(
        format!("  {} queued for after this turn", glyph::HOURGLASS),
        theme.faint_style(),
    ))];

    // The waiting messages themselves, one line each, collapsed to their
    // first line and bounded so a tall queue can never flood the layout.
    const MAX_SHOWN: usize = 3;
    for preview in app.prompt_queue_items(MAX_SHOWN) {
        lines.push(Line::from(Span::styled(
            format!(
                "    {} {}",
                glyph::CHEVRON,
                text::truncate(&preview, area.width as usize - 10)
            ),
            Style::default().fg(theme.muted()),
        )));
    }
    if queue_len > MAX_SHOWN {
        lines.push(Line::from(Span::styled(
            format!("    {} +{} more waiting", glyph::DOT, queue_len - MAX_SHOWN),
            theme.faint_style(),
        )));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut crate::app::App) {
    if area.width < 20 || area.height == 0 {
        return;
    }
    let theme = app.theme;
    let show_thinking = app.show_thinking;
    let show_details = app.show_tool_details;

    if !app.entries.iter().any(|e| e.role != EntryRole::System) {
        app.view.content_height = 0;
        app.view.scroll_from_bottom = 0;
        render_welcome(frame, area, app, &theme);
        return;
    }

    let crate::app::App { entries, view, .. } = app;
    // Reserve space for scrollbar
    let width = area.width.saturating_sub(1) as usize;
    let view_h = area.height;

    // 1. Offsets come from cached heights; entries outside the window use a cheap estimate.
    view.rebuild_offsets(entries, width, view_h);
    let (mut top, window_bottom) = view.window();

    // Warm the render cache, then rebuild offsets so scrolling uses exact heights.
    let margin = 16usize;
    let first = entries.len().saturating_sub(WARM_LIMIT);
    let pad = (margin as u16) * 8;
    for idx in first..entries.len() {
        let (start, end) = (view.offsets[idx], view.offsets[idx + 1]);
        if entries.len() <= WARM_LIMIT || (end > top + pad && start < window_bottom + pad) {
            entries[idx].ensure_rendered(&theme, width, show_details);
        }
    }
    view.mark_dirty();
    view.rebuild_offsets(entries, width, view_h);
    (top, _) = view.window();
    let bottom = top + view_h;

    // Check if we have an active selection
    let selection = app.selection.clone();

    // 2. Walk the virtual rows that intersect the viewport.
    let mut row = top;
    let mut idx = 0usize;
    while idx < entries.len() && row < bottom {
        let entry_start = view.offsets[idx];
        let entry_end = view.offsets[idx + 1];
        if entry_end <= row {
            idx += 1;
            continue;
        }
        if entry_start >= bottom {
            break;
        }

        let status_rows = entries[idx].status_rows() as usize;
        let status = status_line(&entries[idx], &theme, view.spinner).filter(|_| show_thinking);
        let entry = &mut entries[idx];
        let cached = entry.ensure_rendered(&theme, width, show_details);

        let batch_start = row;
        let mut batch: Vec<Line<'static>> = Vec::with_capacity((bottom - row) as usize);
        while row < bottom && row < entry_end {
            let local = (row - entry_start) as usize;
            let line = if local < status_rows {
                status.as_ref().cloned()
            } else {
                let body = local - status_rows;
                cached.lines.get(body).cloned()
            };

            // Apply selection highlighting if this line is within the selection
            if let Some(line) = line {
                if let Some(ref sel) = selection {
                    batch.push(apply_selection_highlight(
                        line,
                        idx,
                        local.saturating_sub(status_rows),
                        status_rows,
                        sel,
                        &theme,
                    ));
                } else {
                    batch.push(line);
                }
            } else {
                batch.push(Line::default());
            }
            row += 1;
        }

        let height = batch.len() as u16;
        if height > 0 {
            frame.render_widget(
                Paragraph::new(Text::from(batch)),
                Rect {
                    x: area.x,
                    y: area.y + (batch_start - top),
                    width: width as u16,
                    height,
                },
            );
        }
        idx += 1;
    }

    if view.content_height > view_h {
        let mut state = ratatui::widgets::ScrollbarState::new(view.content_height as usize)
            .position(top as usize)
            .viewport_content_length(view_h as usize);
        let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("█")
            .thumb_style(Style::default().fg(theme.accent()))
            .track_style(Style::default().fg(theme.border()));
        frame.render_stateful_widget(
            sb,
            Rect {
                x: area.x + area.width.saturating_sub(1),
                y: area.y,
                width: 1,
                height: view_h,
            },
            &mut state,
        );
    }
}

/// Apply selection highlighting to a line if it falls within the selection range
fn apply_selection_highlight(
    line: Line<'static>,
    entry_idx: usize,
    body_row: usize,
    _status_rows: usize,
    sel: &crate::app::Selection,
    theme: &Theme,
) -> Line<'static> {
    let (start_entry, start_row, start_col) = sel.start;
    let (end_entry, end_row, end_col) = sel.end;

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

    // Check if this entry is within selection range
    if entry_idx < start_entry || entry_idx > end_entry {
        return line;
    }

    // For body rows, check if this row is within selection
    let is_in_range = if entry_idx == start_entry && entry_idx == end_entry {
        body_row >= start_row && body_row <= end_row
    } else if entry_idx == start_entry {
        body_row >= start_row
    } else if entry_idx == end_entry {
        body_row <= end_row
    } else {
        true // Middle entry - fully selected
    };

    if !is_in_range {
        return line;
    }

    // Apply highlighting to spans within the column range
    let mut new_spans = Vec::new();
    let mut current_col = 0usize;

    for span in line.spans {
        let span_len = span.content.chars().count();
        let span_end = current_col + span_len;

        let sel_start = if entry_idx == start_entry && body_row == start_row {
            start_col
        } else {
            0
        };
        let sel_end = if entry_idx == end_entry && body_row == end_row {
            end_col
        } else {
            usize::MAX
        };

        // Check if this span overlaps with selection
        if span_end > sel_start && current_col < sel_end {
            // Split into up to 3 parts: before, selected, after
            let chars: Vec<char> = span.content.chars().collect();

            // Before selection
            if current_col < sel_start {
                let before_end = (sel_start - current_col).min(chars.len());
                if before_end > 0 {
                    let before: String = chars[0..before_end].iter().collect();
                    new_spans.push(Span::styled(before, span.style));
                }
            }

            // Selected portion
            let sel_begin = sel_start.saturating_sub(current_col);
            let sel_finish = if span_end > sel_end {
                sel_end - current_col
            } else {
                span_len
            };
            let sel_finish = sel_finish.min(chars.len());

            if sel_begin < sel_finish && sel_begin < chars.len() {
                let selected: String = chars[sel_begin..sel_finish].iter().collect();
                let highlight_style = span.style.bg(theme.accent()).fg(theme.panel_bg());
                new_spans.push(Span::styled(selected, highlight_style));
            }

            // After selection
            if span_end > sel_end && sel_end < usize::MAX && sel_end > current_col {
                let after_start = sel_end - current_col;
                if after_start < chars.len() {
                    let after: String = chars[after_start..].iter().collect();
                    new_spans.push(Span::styled(after, span.style));
                }
            }

            // If no overlap, keep as-is
            if span_end <= sel_start || current_col >= sel_end {
                new_spans.push(span);
            }
        } else {
            new_spans.push(span);
        }

        current_col = span_end;
    }

    Line::from(new_spans)
}

/// Centred placeholder shown until the first prompt is sent.
fn render_welcome(frame: &mut Frame, area: Rect, app: &crate::app::App, theme: &Theme) {
    let card_w = (area.width as usize).saturating_sub(8).clamp(40, 60);

    let logo = vec![
        Line::from(vec![
            Span::styled("    ▄▄▄▄▄▄▄ ", Style::default().fg(theme.accent())),
            Span::styled("nanomite", theme.bold().fg(theme.strong())),
        ]),
        Line::from(vec![
            Span::styled("   █ ▄▄▄ █ ", Style::default().fg(theme.accent())),
            Span::styled("the microscopic agent", theme.faint_style()),
        ]),
        Line::from(Span::styled(
            "    ▀▀▀▀▀▀▀ ",
            Style::default().fg(theme.accent()),
        )),
    ];

    let rows: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("  model     ", theme.faint_style()),
            Span::styled(text::truncate(&app.model_label(), card_w - 12), theme.dim()),
        ]),
        Line::from(vec![
            Span::styled("  provider  ", theme.faint_style()),
            Span::styled(
                text::truncate(&app.provider_label(), card_w - 12),
                theme.dim(),
            ),
        ]),
        Line::from(vec![
            Span::styled("  workspace ", theme.faint_style()),
            Span::styled(text::display_path(&app.cwd, card_w - 12), theme.dim()),
        ]),
        Line::from(vec![
            Span::styled("  tools     ", theme.faint_style()),
            Span::styled("files · shell · ssh · web · memory", theme.dim()),
        ]),
    ];

    let lines: Vec<Line> = logo
        .into_iter()
        .chain(std::iter::once(Line::default()))
        .chain(rows)
        .chain(std::iter::once(Line::default()))
        .chain(std::iter::once(Line::from(vec![Span::styled(
            "  ❯ ask anything to begin  ",
            theme.accent_style(),
        )])))
        .chain(std::iter::once(Line::from(vec![Span::styled(
            "  /  commands  ·  ctrl+x  palette  ·  esc  interrupt",
            theme.faint_style(),
        )])))
        .collect();

    let height = lines.len() as u16;
    if area.height <= height {
        return;
    }
    let x = area.x + (area.width.saturating_sub(card_w as u16)) / 2;
    let y = area.y + (area.height - height) / 2;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focus()))
        .style(Style::default().bg(theme.panel_bg()));

    frame.render_widget(
        block,
        Rect {
            x,
            y,
            width: card_w as u16,
            height,
        },
    );
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::default().bg(theme.panel_bg())),
        Rect {
            x,
            y,
            width: card_w as u16,
            height,
        },
    );
}

/// Format timestamp as HH:MM
fn format_timestamp(ts: u64) -> String {
    let dt = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(ts);
    let local = chrono::DateTime::<chrono::Local>::from(dt);
    local.format("%H:%M").to_string()
}

/// Live one-line status (spinner / error / summary) drawn above the cached body.
fn status_line(entry: &Entry, theme: &Theme, spinner: usize) -> Option<Line<'static>> {
    match entry.phase {
        Phase::Streaming => {
            let elapsed = entry.elapsed.unwrap_or_else(|| entry.started.elapsed());
            let mut spans = vec![
                Span::styled(
                    format!("{} ", glyph::SPINNER[spinner % glyph::SPINNER.len()]),
                    Style::default().fg(theme.accent()),
                ),
                Span::styled("thinking", Style::default().fg(theme.muted())),
                Span::styled(
                    format!("  {}", text::format_duration(elapsed)),
                    Style::default().fg(theme.faint()),
                ),
            ];
            // Steps · tools finished so far
            let done_tools = entry.tools.iter().filter(|t| !t.running).count();
            if entry.steps > 0 || done_tools > 0 {
                spans.push(Span::styled(
                    format!("  · {}", text::plural(entry.steps, "step")),
                    Style::default().fg(theme.faint()),
                ));
                if done_tools > 0 {
                    spans.push(Span::styled(
                        format!(" · {}", text::plural(done_tools, "tool")),
                        Style::default().fg(theme.faint()),
                    ));
                }
            }
            // Streaming token counter
            if let Some(usage) = &entry.usage {
                let tokens = usage.completion_tokens.saturating_add(usage.prompt_tokens);
                if tokens > 0 {
                    spans.push(Span::styled(
                        format!("  ▋ {} tok", text::format_tokens(tokens as u64)),
                        Style::default().fg(theme.accent_soft()),
                    ));
                }
            }
            Some(Line::from(spans))
        }
        Phase::Failed => {
            let msg = entry.error.clone().unwrap_or_else(|| "failed".into());
            Some(Line::from(vec![
                Span::styled(
                    format!("{} ", glyph::CROSS),
                    Style::default().fg(theme.danger()),
                ),
                Span::styled(
                    text::truncate(&msg, 96),
                    Style::default().fg(theme.danger()),
                ),
            ]))
        }
        Phase::Interrupted => Some(Line::from(vec![
            Span::styled(
                format!("{} ", glyph::DOT),
                Style::default().fg(theme.warn()),
            ),
            Span::styled("interrupted", Style::default().fg(theme.muted())),
            Span::styled(
                format!(
                    "  {}",
                    text::format_duration(entry.elapsed.unwrap_or_default())
                ),
                Style::default().fg(theme.faint()),
            ),
        ])),
        Phase::Done => None,
    }
}

impl Entry {
    /// Height of the cached body (excludes the dynamic status line and trailing gap).
    pub fn height(&self) -> u16 {
        self.cache
            .as_ref()
            .map(|c| c.height)
            .unwrap_or_else(|| self.estimated_height(80))
    }

    /// Cheap pre-render height so offscreen entries never touch markdown.
    pub fn estimated_height(&self, width: usize) -> u16 {
        let inner = width.saturating_sub(GUTTER).max(8);
        match self.role {
            EntryRole::User => {
                let wrap_w = inner.saturating_sub(2).max(8);
                let mut rows = 3usize; // role badge + empty line + content start
                for line in self.text.lines() {
                    let w = text::width(line) as u16;
                    rows += w.div_ceil(wrap_w as u16).max(1) as usize;
                }
                (self.tools.len() as u16) + rows.max(1) as u16
            }
            _ => {
                let mut h = self.tools.len() as u16 + 4; // role badge + empty line + tool section gap
                if self.text.is_empty() {
                    h += 1;
                } else {
                    for line in self.text.lines() {
                        let w = text::width(line) as u16;
                        h += w.div_ceil(inner as u16).max(1);
                        h += 1;
                    }
                }
                h
            }
        }
    }

    /// True when this entry paints a live status line above its body.
    pub fn status_rows(&self) -> u16 {
        u16::from(self.phase != Phase::Done)
    }

    /// Rebuilds cached lines only when the entry or the viewport changed.
    pub fn ensure_rendered(&mut self, theme: &Theme, width: usize, details: bool) -> &Rendered {
        let stale = self
            .cache
            .as_ref()
            .is_none_or(|c| c.rev != self.rev || c.width as usize != width || c.details != details);
        if stale {
            let lines = self.build(theme, width, details);
            let height = lines.len() as u16;
            self.cache = Some(Rendered {
                rev: self.rev,
                width: width as u16,
                details,
                lines,
                height,
            });
        }
        self.cache.as_ref().expect("cache filled above")
    }

    fn build(&self, theme: &Theme, width: usize, details: bool) -> Vec<Line<'static>> {
        let inner = width.saturating_sub(GUTTER).max(8);
        let mut lines = Vec::new();

        match self.role {
            EntryRole::System => {
                lines.push(Line::from(Span::styled(
                    format!("{} {}", glyph::DOT, self.text),
                    Style::default().fg(theme.faint()),
                )));
            }
            EntryRole::User => {
                let style = Style::default()
                    .fg(theme.strong())
                    .add_modifier(Modifier::BOLD);
                let wrap_w = inner.saturating_sub(4).max(8);
                let mut first = true;
                let ts = format_timestamp(self.started.elapsed().as_secs());

                // User badge with timestamp
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        " YOU ",
                        Style::default()
                            .fg(theme.panel_bg())
                            .bg(theme.accent())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(ts, Style::default().fg(theme.faint())),
                ]));
                lines.push(Line::default());

                for raw in self.text.split('\n') {
                    if raw.trim().is_empty() {
                        lines.push(Line::from(vec![Span::styled("    ", Style::default())]));
                        continue;
                    }
                    lines.extend(
                        wrap_spans(vec![Span::raw(raw.to_string())], wrap_w)
                            .into_iter()
                            .map(|mut l| {
                                for span in l.spans.iter_mut() {
                                    *span = Span::styled(span.content.to_string(), style);
                                }
                                let mut spans = Vec::with_capacity(l.spans.len() + 2);
                                spans.push(Span::styled(
                                    if first {
                                        format!("{} ", glyph::PROMPT)
                                    } else {
                                        "  ".to_string()
                                    },
                                    Style::default().fg(theme.accent()),
                                ));
                                first = false;
                                spans.extend(l.spans);
                                Line::from(spans)
                            }),
                    );
                }
                if first {
                    lines.push(Line::from(Span::raw("    ")));
                }
            }
            EntryRole::Assistant => {
                let _wrap_w = inner;
                let ts = format_timestamp(self.started.elapsed().as_secs());

                // Assistant badge with timestamp
                let (badge_style, _badge_text) = match self.phase {
                    Phase::Streaming => (
                        Style::default()
                            .fg(theme.panel_bg())
                            .bg(theme.accent_soft()),
                        " ◐  ",
                    ),
                    Phase::Failed => (
                        Style::default().fg(theme.panel_bg()).bg(theme.danger()),
                        " ✗  ",
                    ),
                    Phase::Interrupted => (
                        Style::default().fg(theme.panel_bg()).bg(theme.warn()),
                        " ●  ",
                    ),
                    _ => (
                        Style::default().fg(theme.panel_bg()).bg(theme.success()),
                        " ✓  ",
                    ),
                };

                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(" AI ", badge_style.add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                    Span::styled(ts, Style::default().fg(theme.faint())),
                ]));
                lines.push(Line::default());

                // Tool calls as pills
                for tool in &self.tools {
                    let (marker, marker_style, status_text) = if tool.running {
                        (
                            glyph::SPINNER[0],
                            Style::default().fg(theme.warn()),
                            "running",
                        )
                    } else if tool.ok {
                        (
                            tool_glyph::SUCCESS,
                            Style::default().fg(theme.success()),
                            "ok",
                        )
                    } else {
                        (
                            tool_glyph::FAILURE,
                            Style::default().fg(theme.danger()),
                            "failed",
                        )
                    };

                    let mut pill_spans = vec![
                        Span::raw("  "),
                        Span::styled(" ", Style::default()),
                        Span::styled(format!("{} ", marker), marker_style),
                        Span::styled(
                            tool.name.clone(),
                            Style::default()
                                .fg(theme.tool())
                                .add_modifier(Modifier::BOLD),
                        ),
                    ];

                    // Mini-diff chip for edit_file
                    if tool.name == "edit_file" {
                        if let Some(_r) = tool.edit_replacements {
                            pill_spans.push(Span::raw(" "));
                            pill_spans.push(Span::styled(
                                format!("{} {}", tool_glyph::DIFF_ADD, tool_glyph::DIFF_DEL),
                                Style::default().fg(theme.diff_add()),
                            ));
                        }
                    }

                    // Status pill
                    let status_color = if tool.running {
                        theme.warn()
                    } else if tool.ok {
                        theme.success()
                    } else {
                        theme.danger()
                    };
                    pill_spans.push(Span::raw(" "));
                    pill_spans.push(Span::styled(
                        format!(" {} ", status_text),
                        Style::default().fg(theme.panel_bg()).bg(status_color),
                    ));

                    // Duration
                    if let Some(elapsed) = self.elapsed {
                        pill_spans.push(Span::raw(" "));
                        pill_spans.push(Span::styled(
                            format!(" {} ", text::format_duration(elapsed)),
                            Style::default().fg(theme.faint()),
                        ));
                    }

                    // Detail in compact mode
                    if !details {
                        let detail = text::truncate(
                            &tool.detail,
                            inner.saturating_sub(tool.name.chars().count() + 20),
                        );
                        if !detail.is_empty() {
                            pill_spans.push(Span::raw(" "));
                            pill_spans
                                .push(Span::styled(detail, Style::default().fg(theme.muted())));
                        }
                    }

                    lines.push(Line::from(pill_spans));

                    // Live preview of what the tool is writing/running
                    if details {
                        if let Some(preview) = &tool.preview {
                            if tool.name == "edit_file" {
                                let mut diff = DiffView::with_theme(*theme);
                                for raw in preview.split('\n') {
                                    diff.push_diff(raw.to_string());
                                }
                                for line in diff.styled_lines() {
                                    let content: String =
                                        line.spans.iter().map(|s| s.content.as_ref()).collect();
                                    let style =
                                        line.spans.first().map(|s| s.style).unwrap_or_default();
                                    let shown = text::truncate(&content, inner.saturating_sub(4));
                                    if shown.is_empty() {
                                        lines.push(Line::from(Span::raw("    ")));
                                    } else {
                                        lines.push(Line::from(Span::styled(
                                            format!("    {shown}"),
                                            style,
                                        )));
                                    }
                                }
                            } else {
                                for raw in preview.split('\n') {
                                    let style = if raw.starts_with('…') {
                                        Style::default().fg(theme.faint())
                                    } else {
                                        Style::default().fg(theme.muted())
                                    };
                                    let shown = text::truncate(raw, inner.saturating_sub(4));
                                    if shown.is_empty() {
                                        lines.push(Line::from(Span::raw("    ")));
                                    } else {
                                        lines.push(Line::from(Span::styled(
                                            format!("    {shown}"),
                                            style,
                                        )));
                                    }
                                }
                            }
                        }
                    }
                }

                if !self.tools.is_empty() {
                    lines.push(Line::default());
                }

                let st = text::MarkdownStyles::new(theme);
                if self.text.trim().is_empty() {
                    lines.push(Line::default());
                } else {
                    lines.extend(text::markdown(&self.text, inner, &st));
                }

                // Live stream cursor at the head of the reply, like opencode.
                if self.phase == Phase::Streaming {
                    if let Some(last) = lines.last_mut() {
                        if let Some(s) = last.spans.last_mut() {
                            let mut chars: Vec<char> = s.content.chars().collect();
                            if !chars.is_empty() {
                                chars.pop();
                            }
                            s.content = chars.into_iter().collect();
                        }
                        last.spans.push(Span::styled(
                            glyph::CURSOR,
                            Style::default()
                                .fg(theme.accent())
                                .add_modifier(Modifier::SLOW_BLINK),
                        ));
                    }
                }
            }
        }

        if let Some(err) = &self.error {
            lines.push(Line::default());
            lines.extend(text::wrap_prefixed(
                err,
                &format!("{} ", glyph::CROSS),
                Style::default().fg(theme.danger()),
                inner,
                "  ",
            ));
        }

        lines
    }
}
