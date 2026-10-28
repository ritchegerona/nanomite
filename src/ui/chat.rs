use super::diff::DiffView;
use super::text::{self, wrap_spans};
use super::theme::{glyph, Theme};
use crate::app::{Entry, EntryRole, Phase, Rendered};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation,
};
use ratatui::Frame;

/// Width of the left gutter reserved for role badges.
pub const GUTTER: usize = 4;

/// Transcripts up to this many entries get measured exactly before drawing.
pub const WARM_LIMIT: usize = 400;

/// Maximum columns for the tool detail when `/details` is off (compact mode).
pub const COMPACT_TOOL_DETAIL_WIDTH: usize = 40;

/// Glyphs for compact tool status display.
pub mod tool_glyph {
    pub const RUNNING: &str = "•";
    pub const SUCCESS: &str = "✓";
    pub const FAILURE: &str = "✗";
    pub const DIFF_ADD: &str = "+";
    pub const DIFF_DEL: &str = "−";
}

/// opencode-style glyph for one tool's inline row: `$` runs commands, `→`
/// reads and lists, `←` writes, `✱` searches, `%` fetches a URL, `◈` searches
/// the web, `⚙` anything without a dedicated mark.
fn tool_icon(name: &str) -> &'static str {
    match name {
        "run_command" | "ssh_run" => "$",
        "read_file" | "read_document" | "list_files" => "→",
        "write_file" | "edit_file" => "←",
        "search" => "✱",
        "web_fetch" => "%",
        "web_search" => "◈",
        _ => "⚙",
    }
}

/// The active tool mode as `(colour, title-cased label)`. opencode keys the
/// user-message bar and the assistant footer to the agent colour; nanomite's
/// equivalent of an agent is the tool mode.
pub(crate) fn mode_parts(
    policy: crate::agent::policy::ToolPolicy,
    theme: &Theme,
) -> (Color, &'static str) {
    use crate::agent::policy::ToolPolicy;
    let color = match policy {
        ToolPolicy::Plan => theme.mode_plan(),
        ToolPolicy::Coder => theme.mode_coder(),
        ToolPolicy::Build => theme.mode_build(),
        ToolPolicy::Hacker => theme.mode_hacker(),
    };
    let label = match policy {
        ToolPolicy::Plan => "Plan",
        ToolPolicy::Coder => "Coder",
        ToolPolicy::Build => "Build",
        ToolPolicy::Hacker => "Hacker",
    };
    (color, label)
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
    let mode = mode_parts(app.cfg.tool_policy, &theme);

    if !app.entries.iter().any(|e| e.role != EntryRole::System) {
        app.view.content_height = 0;
        app.view.scroll_from_bottom = 0;
        render_welcome(frame, area, app, &theme);
        return;
    }

    // Focus mode (f2, /focus): render only the latest exchange. The viewport
    // offsets are rebuilt relative to this slice, so scrolling, search jumps
    // and alt-arrow navigation all operate inside the window.
    let focus_base = if app.focus {
        app.entries
            .iter()
            .rposition(|e| e.role == EntryRole::User)
            .unwrap_or(0)
    } else {
        0
    };
    app.focus_base = focus_base;

    let busy = app.is_busy();
    let crate::app::App { entries, view, .. } = app;
    let entries = &mut entries[focus_base..];
    // Reserve space for scrollbar
    let width = area.width.saturating_sub(1) as usize;
    let view_h = area.height;

    // Transcript search (ctrl+f): the query is overlaid at render time so the
    // per-entry cache never churns while the user types.
    let search_query = app
        .search
        .as_ref()
        .map(|s| s.query.clone())
        .filter(|q| !q.trim().is_empty());

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
            entries[idx].ensure_rendered(&theme, width, show_details, mode);
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
        let cached = entry.ensure_rendered(&theme, width, show_details, mode);

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

            // Apply search highlighting first, then selection highlighting
            // (the selection wins visually where both cover a span).
            if let Some(mut line) = line {
                if let Some(q) = &search_query {
                    line = apply_search_highlight(line, q, &theme);
                }
                if let Some(ref sel) = selection {
                    line = apply_selection_highlight(
                        line,
                        idx,
                        local.saturating_sub(status_rows),
                        status_rows,
                        sel,
                        &theme,
                    );
                }
                batch.push(line);
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

    // Floating "new content" pill, opencode-style: while the live tail is
    // streaming in and the user is scrolled away, count the rows that arrived
    // and offer the jump back. Painted over the transcript's bottom-right
    // corner, clear of the scrollbar column.
    if busy && !view.follow && view.new_since_scroll > 0 && view.content_height > view_h {
        let label = format!(
            "{} {} new lines · end to follow",
            glyph::ARROW_DOWN,
            view.new_since_scroll
        );
        let width = (label.chars().count() as u16 + 2).min(area.width.saturating_sub(2));
        if width >= 8 {
            let rect = Rect {
                x: area.x + area.width.saturating_sub(width + 1),
                y: area.y + view_h.saturating_sub(1),
                width,
                height: 1,
            };
            frame.render_widget(Clear, rect);
            let pill = Paragraph::new(Line::from(Span::styled(
                format!(" {label} "),
                Style::default().fg(theme.panel_bg()).bg(theme.accent()),
            )));
            frame.render_widget(pill, rect);
        }
    }
}

/// Overlay the search-match style on every case-insensitive occurrence of
/// `query` in a rendered line, preserving each span's original style. Lines
/// without a hit are returned untouched, so most rows cost one `contains`.
fn apply_search_highlight(line: Line<'static>, query: &str, theme: &Theme) -> Line<'static> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return line;
    }
    let flat: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    if !flat.to_lowercase().contains(&needle) {
        return line;
    }
    // Browser-style highlight: the warn tone fills the match, panel text on top.
    let style = Style::default().fg(theme.panel_bg()).bg(theme.warn());
    Line::from(text::highlight_spans(line.spans, &needle, style))
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

    // Char range covered on this row: the anchor columns are CHAR indices
    // (the mouse mapping snaps clicks to grapheme-cluster starts, so wide
    // glyphs and emoji select whole). Rows between anchor and end are
    // selected whole.
    let char_start = if entry_idx == start_entry && body_row == start_row {
        start_col
    } else {
        0
    };
    let char_end = if entry_idx == end_entry && body_row == end_row {
        end_col
    } else {
        usize::MAX
    };

    // Walk the row's characters with one global counter and highlight the
    // ones inside [char_start, char_end). Every selection unit in the app
    // is a char index - the pointer mapping, the keyboard visual mode and
    // the text extraction - so the painted cells land exactly under the
    // pointer; ratatui carries the style across a wide glyph's continuation
    // cell, keeping CJK and emoji blocks fully covered.
    let highlight = Style::default().bg(theme.accent()).fg(theme.panel_bg());
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut plain = String::new();
    let mut marked = String::new();
    let mut idx = 0usize;

    for span in line.spans {
        for ch in span.content.chars() {
            if idx >= char_start && idx < char_end {
                if !plain.is_empty() {
                    out.push(Span::styled(std::mem::take(&mut plain), span.style));
                }
                marked.push(ch);
            } else {
                if !marked.is_empty() {
                    out.push(Span::styled(
                        std::mem::take(&mut marked),
                        span.style.patch(highlight),
                    ));
                }
                plain.push(ch);
            }
            idx += 1;
        }
        // Flush at span boundaries so the original per-span styles survive.
        if !plain.is_empty() {
            out.push(Span::styled(std::mem::take(&mut plain), span.style));
        }
        if !marked.is_empty() {
            out.push(Span::styled(
                std::mem::take(&mut marked),
                span.style.patch(highlight),
            ));
        }
    }

    Line::from(out)
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

    let mut rows: Vec<Line> = vec![
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
    // The resume card: what memory says the last work session was about.
    if let Some(hint) = &app.resume_hint {
        rows.push(Line::from(vec![
            Span::styled("  resume    ", theme.faint_style()),
            Span::styled(text::truncate(hint, card_w - 12), theme.dim()),
        ]));
    }

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

/// Live one-line status above the cached body while the turn streams, at the
/// same three-column indent as the rest of the assistant content. Terminal
/// states render in the body itself: failures as `Error: …`, interrupts on
/// the message footer.
fn status_line(entry: &Entry, theme: &Theme, spinner: usize) -> Option<Line<'static>> {
    if entry.phase != Phase::Streaming {
        return None;
    }
    let elapsed = entry.elapsed.unwrap_or_else(|| entry.started.elapsed());
    let mut spans = vec![
        Span::raw("   ".to_string()),
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
                let mut rows = 2usize; // raised-block padding above and below
                for line in self.text.lines() {
                    let w = text::width(line) as u16;
                    rows += w.div_ceil(wrap_w as u16).max(1) as usize;
                }
                (self.tools.len() as u16) + rows.max(1) as u16
            }
            _ => {
                let text_w = inner.saturating_sub(3).max(8);
                let mut h = self.tools.len() as u16 + 1; // message footer
                if !self.tools.is_empty() {
                    h += 1; // gap between tool rows and the body
                }
                if self.text.is_empty() {
                    h += 1;
                } else {
                    for line in self.text.lines() {
                        let w = text::width(line) as u16;
                        h += w.div_ceil(text_w as u16).max(1);
                    }
                }
                if self.error.is_some() {
                    h += 2;
                }
                h
            }
        }
    }

    /// True when this entry paints a live status line above its body. Only
    /// the streaming phase does: failures render as `Error: …` in the body,
    /// interrupts on the message footer.
    pub fn status_rows(&self) -> u16 {
        u16::from(self.phase == Phase::Streaming)
    }

    /// Rebuilds cached lines only when the entry or the viewport changed.
    pub fn ensure_rendered(
        &mut self,
        theme: &Theme,
        width: usize,
        details: bool,
        mode: (Color, &'static str),
    ) -> &Rendered {
        let stale = self.cache.as_ref().is_none_or(|c| {
            c.rev != self.rev
                || c.width as usize != width
                || c.details != details
                || c.mode_color != mode.0
                || c.mode_label != mode.1
        });
        if stale {
            let (lines, code_blocks) = self.build(theme, width, details, mode);
            let height = lines.len() as u16;
            self.cache = Some(Rendered {
                rev: self.rev,
                width: width as u16,
                details,
                mode_color: mode.0,
                mode_label: mode.1.to_string(),
                code_blocks,
                lines,
                height,
            });
        }
        self.cache.as_ref().expect("cache filled above")
    }

    /// Renders the entry's lines directly. Cached paths go through
    /// `ensure_rendered`; `pub(crate)` for height diagnostics in tests.
    #[allow(clippy::type_complexity)]
    pub(crate) fn build(
        &self,
        theme: &Theme,
        width: usize,
        details: bool,
        mode: (Color, &'static str),
    ) -> (Vec<Line<'static>>, Vec<(u16, u16, String)>) {
        let inner = width.saturating_sub(GUTTER).max(8);
        let mut lines = Vec::new();
        let mut code_blocks = Vec::new();
        let (mode_color, mode_label) = mode;

        match self.role {
            EntryRole::System => {
                lines.push(Line::from(Span::styled(
                    format!("{} {}", glyph::DOT, self.text),
                    Style::default().fg(theme.faint()),
                )));
            }
            EntryRole::User => {
                // opencode-style prompt block: a mode-coloured bar down the
                // left edge of a raised background, plain text inside. No
                // badge, no timestamp.
                let bg = theme.element_bg();
                let bar = Style::default().fg(mode_color);
                let pad = Style::default().bg(bg);
                let text_style = Style::default().fg(theme.strong()).bg(bg);
                let wrap_w = inner.saturating_sub(2).max(8);

                let bar_row = |lines: &mut Vec<Line<'static>>| {
                    let mut spans = vec![Span::styled(glyph::GUTTER.to_string(), bar)];
                    if width > 1 {
                        spans.push(Span::styled(" ".repeat(width - 1), pad));
                    }
                    lines.push(Line::from(spans));
                };
                bar_row(&mut lines);

                for raw in self.text.split('\n') {
                    if raw.trim().is_empty() {
                        // Blank rows stay inside the raised block.
                        bar_row(&mut lines);
                        continue;
                    }
                    for seg in wrap_spans(vec![Span::raw(raw.to_string())], wrap_w) {
                        let content: String =
                            seg.spans.iter().map(|s| s.content.as_ref()).collect();
                        let w = text::width(&content);
                        let mut spans = vec![
                            Span::styled(glyph::GUTTER.to_string(), bar),
                            Span::styled(" ".to_string(), pad),
                            Span::styled(content, text_style),
                        ];
                        if width > 2 + w {
                            spans.push(Span::styled(" ".repeat(width - 2 - w), pad));
                        }
                        lines.push(Line::from(spans));
                    }
                }

                bar_row(&mut lines);
            }
            EntryRole::Assistant => {
                // opencode-style rows: everything indented three columns,
                // tool calls as `icon label` in one colour, no badge. Running
                // rows show a bare `•`; failures go red with `✗`.
                for tool in &self.tools {
                    let (icon, style) = if tool.running {
                        (tool_glyph::RUNNING, Style::default().fg(theme.muted()))
                    } else if tool.ok {
                        (tool_icon(&tool.name), Style::default().fg(theme.muted()))
                    } else {
                        (tool_glyph::FAILURE, Style::default().fg(theme.danger()))
                    };
                    let label = if tool.detail.is_empty() {
                        tool.name.clone()
                    } else {
                        format!("{} {}", tool.name, tool.detail)
                    };
                    let budget = inner.saturating_sub(5).max(8);
                    let mut spans = vec![
                        Span::raw("   ".to_string()),
                        Span::styled(format!("{icon} "), style),
                        Span::styled(text::truncate(&label, budget), style),
                    ];
                    if !tool.running {
                        if let Some(elapsed) = tool.elapsed {
                            spans.push(Span::styled(
                                format!(" · {}", text::format_duration(elapsed)),
                                Style::default().fg(theme.faint()),
                            ));
                        }
                    }
                    lines.push(Line::from(spans));

                    // Completed block bodies: output or patch, shown once the
                    // tool has finished (opencode keeps running rows bare) and
                    // only while `/details` / `ctrl+o` is on.
                    if details && !tool.running {
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
                                    let shown = text::truncate(&content, inner.saturating_sub(5));
                                    if shown.is_empty() {
                                        lines.push(Line::from(Span::raw("     ")));
                                    } else {
                                        lines.push(Line::from(Span::styled(
                                            format!("     {shown}"),
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
                                    let shown = text::truncate(raw, inner.saturating_sub(5));
                                    if shown.is_empty() {
                                        lines.push(Line::from(Span::raw("     ")));
                                    } else {
                                        lines.push(Line::from(Span::styled(
                                            format!("     {shown}"),
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
                    let base = lines.len();
                    let (md_lines, blocks) =
                        text::markdown_with_blocks(&self.text, inner.saturating_sub(3).max(8), &st);
                    for mut line in md_lines {
                        let mut spans = Vec::with_capacity(line.spans.len() + 1);
                        spans.push(Span::raw("   ".to_string()));
                        spans.append(&mut line.spans);
                        lines.push(Line::from(spans));
                    }
                    for (start, end, source) in blocks {
                        code_blocks.push(((base + start) as u16, (base + end) as u16, source));
                    }
                }

                // Live stream cursor at the head of the reply, like opencode.
                // The caret is appended after the last character; a character
                // is only yielded when the final line already fills the
                // viewport, so short lines never flicker and full ones never
                // clip the caret.
                if self.phase == Phase::Streaming {
                    if let Some(last) = lines.last_mut() {
                        let line_width: usize = last
                            .spans
                            .iter()
                            .map(|s| text::width(s.content.as_ref()))
                            .sum();
                        if line_width >= inner {
                            if let Some(s) = last.spans.last_mut() {
                                let mut chars: Vec<char> = s.content.chars().collect();
                                if !chars.is_empty() {
                                    chars.pop();
                                }
                                s.content = chars.into_iter().collect();
                            }
                        }
                        last.spans.push(Span::styled(
                            glyph::CURSOR,
                            Style::default()
                                .fg(theme.accent())
                                .add_modifier(Modifier::SLOW_BLINK),
                        ));
                    }
                }

                // opencode-style message footer: Mode · model · duration ·
                // tok/s, with `· interrupted` appended when cut short.
                if matches!(self.phase, Phase::Done | Phase::Interrupted) {
                    let mut spans = vec![
                        Span::raw("   ".to_string()),
                        Span::styled(mode_label.to_string(), Style::default().fg(mode_color)),
                    ];
                    if let Some(model) = &self.model {
                        spans.push(Span::styled(
                            format!(" · {}", text::truncate_head(model, 32)),
                            Style::default().fg(theme.faint()),
                        ));
                    }
                    if let Some(elapsed) = self.elapsed {
                        spans.push(Span::styled(
                            format!(" · {}", text::format_duration(elapsed)),
                            Style::default().fg(theme.faint()),
                        ));
                        if let Some(usage) = &self.usage {
                            let secs = elapsed.as_secs_f64();
                            if secs >= 1.0 && usage.completion_tokens > 0 {
                                let rate = f64::from(usage.completion_tokens) / secs;
                                spans.push(Span::styled(
                                    format!(" · {rate:.1} tok/s"),
                                    Style::default().fg(theme.faint()),
                                ));
                            }
                        }
                    }
                    if self.phase == Phase::Interrupted {
                        spans.push(Span::styled(
                            " · interrupted",
                            Style::default().fg(theme.muted()),
                        ));
                    }
                    lines.push(Line::from(spans));
                }
            }
        }

        if let Some(err) = &self.error {
            lines.push(Line::default());
            let wrapped = text::wrap_prefixed(
                err,
                "   Error: ",
                Style::default().fg(theme.danger()),
                inner,
                "          ",
            );
            if self.error_expanded {
                lines.extend(wrapped);
                lines.push(Line::from(Span::styled(
                    "   … click to collapse",
                    Style::default().fg(theme.faint()),
                )));
            } else {
                // Collapsed: the first couple of lines plus a hint, the full
                // text one plain click away (opencode's error expansion).
                let mut iter = wrapped.into_iter();
                for line in iter.by_ref().take(2) {
                    lines.push(line);
                }
                let remaining = iter.count();
                if remaining > 0 {
                    lines.push(Line::from(Span::styled(
                        format!(
                            "   … {} · click to expand",
                            text::plural(remaining, "more line")
                        ),
                        Style::default().fg(theme.faint()),
                    )));
                }
            }
        }

        (lines, code_blocks)
    }
}
