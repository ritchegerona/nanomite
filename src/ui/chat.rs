use super::diff::DiffView;
use super::text::{self, wrap_spans};
use super::theme::{glyph, Theme};
use crate::app::{Entry, EntryRole, Phase, Rendered, ToolDisplay};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

/// Width of the left gutter reserved for role badges.
pub const GUTTER: usize = 4;

/// Assistant turns, tool rows and code fences share one plain four-column
/// indent — the old vertical rail glyph was dropped for a flatter, quieter
/// transcript. The widths are load-bearing: every body row must stay exactly
/// `GUTTER` cells wide so all text shares one column.
const RAIL: &str = "    ";
/// One tool call under the turn, same width as the body indent.
const BRANCH: &str = "    ";
/// Fence header/footer: same indent, minus the rule the fence draws itself.
const FENCE_HEAD: &str = "   ";
const FENCE_FOOT: &str = "   ";

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
        "web_fetch" | "browse" | "browse_screenshot" => "%",
        "web_search" => "◈",
        _ => "⚙",
    }
}

/// The active tool mode as `(colour, title-cased label)`. opencode keys the
/// user-turn node to the agent colour; nanomite's equivalent of an agent is
/// the tool mode. Assistant responses carry a green node instead.
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
    let each = app
        .avg_turn_duration()
        .map(|d| {
            if d.as_secs() < 60 {
                format!("~{}s each", d.as_secs())
            } else {
                format!("~{}m each", d.as_secs() / 60)
            }
        })
        .unwrap_or_default();
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "  {} queued{}",
            queue_len,
            if each.is_empty() {
                String::new()
            } else {
                format!(" · {each}")
            }
        ),
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
    let tool_display = app.tool_display;
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
    let turn_eta = app.turn_eta();
    let compact = app.compact;
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
    view.rebuild_offsets(entries, width, view_h, compact);
    let (mut top, window_bottom) = view.window();

    // Warm the render cache, then rebuild offsets so scrolling uses exact heights.
    let margin = 16usize;
    let first = entries.len().saturating_sub(WARM_LIMIT);
    let pad = (margin as u16) * 8;
    for idx in first..entries.len() {
        let (start, end) = (view.offsets[idx], view.offsets[idx + 1]);
        if entries.len() <= WARM_LIMIT || (end > top + pad && start < window_bottom + pad) {
            entries[idx].ensure_rendered(&theme, width, tool_display, compact, mode);
        }
    }
    view.mark_dirty();
    view.rebuild_offsets(entries, width, view_h, compact);
    (top, _) = view.window();
    let bottom = top + view_h;

    // Check if we have an active selection
    let selection = app.selection.clone();

    // 2. Walk the virtual rows that intersect the viewport. Offsets are
    //    cumulative and sorted, so binary-search the first entry that can
    //    intersect instead of walking the whole transcript from row zero -
    //    this keeps frame time flat as the session grows to thousands of
    //    entries.
    let mut row = top;
    let mut idx = view
        .offsets
        .partition_point(|&off| off <= top)
        .saturating_sub(1)
        .min(entries.len().saturating_sub(1));
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
        let status = status_line(&entries[idx], &theme, view.spinner, turn_eta, !compact)
            .filter(|_| show_thinking);
        let entry = &mut entries[idx];
        let cached = entry.ensure_rendered(&theme, width, tool_display, compact, mode);

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
        // Custom proportional scrollbar. ratatui's `Scrollbar` positions the
        // thumb as `position / (content - 1 + viewport)`, so even fully
        // scrolled (`top == content - viewport`) the thumb stops well short of
        // the track's bottom. Draw the geometry directly with the same
        // full-track mapping `App::scrollbar_jump` assumes: the thumb travels
        // 0..=track_len - thumb_len over 0..=max_scroll, flush at both ends.
        let track_len = view_h as usize;
        let content = view.content_height as usize;
        let viewport = view_h as usize;
        let max_scroll = (content - viewport).max(1);
        let thumb_len = ((viewport * track_len + content / 2) / content).clamp(1, track_len);
        let thumb_start = ((top as usize * (track_len - thumb_len) + max_scroll / 2) / max_scroll)
            .min(track_len - thumb_len);
        let lines: Vec<Line> = (0..track_len)
            .map(|row| {
                let in_thumb = row >= thumb_start && row < thumb_start + thumb_len;
                let (sym, fg) = if in_thumb {
                    ("█", theme.accent())
                } else {
                    ("│", theme.border())
                };
                Line::from(Span::styled(sym, Style::default().fg(fg)))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(Text::from(lines)),
            Rect {
                x: area.x + area.width.saturating_sub(1),
                y: area.y,
                width: 1,
                height: view_h,
            },
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
/// Finalizes an early-returned build path: wraps lines and blocks into the
/// tuple the entry cache stores.
fn finish_build(
    lines: Vec<Line<'static>>,
    code_blocks: Vec<(u16, u16, String)>,
) -> (Vec<Line<'static>>, Vec<(u16, u16, String)>) {
    (lines, code_blocks)
}

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

/// Centred welcome screen: a clean title, the session facts, and the
/// prompt hint. No box, no logo art — just typography.
fn render_welcome(frame: &mut Frame, area: Rect, app: &crate::app::App, theme: &Theme) {
    let width = (area.width as usize).saturating_sub(4).clamp(36, 56);

    let mut lines: Vec<Line> = Vec::new();

    // Title: name + tagline on one line.
    lines.push(Line::from(vec![
        Span::styled("nanomite", theme.bold().fg(theme.accent())),
        Span::styled("  Intelligence Without Boundaries", theme.faint_style()),
    ]));
    lines.push(Line::default());

    // Session facts.
    let fact = |label: &str, value: String, lines: &mut Vec<Line>| {
        lines.push(Line::from(vec![
            Span::styled(format!("  {label:<11}"), theme.faint_style()),
            Span::styled(
                text::truncate(&value, width.saturating_sub(13)),
                theme.dim(),
            ),
        ]));
    };
    fact("model", app.model_label(), &mut lines);
    fact("provider", app.provider_label(), &mut lines);
    fact(
        "workspace",
        text::display_path(&app.cwd, width.saturating_sub(13)),
        &mut lines,
    );
    fact(
        "tools",
        "files · shell · ssh · web · browser · memory".into(),
        &mut lines,
    );
    if let Some(hint) = &app.resume_hint {
        fact("resume", hint.clone(), &mut lines);
    }

    lines.push(Line::default());

    // Prompt hint.
    lines.push(Line::from(Span::styled(
        "  ❯ ask anything to begin",
        theme.accent_style(),
    )));
    lines.push(Line::from(Span::styled(
        "    / commands  ·  ctrl+x palette  ·  esc interrupt",
        theme.faint_style(),
    )));

    let height = lines.len() as u16;
    if area.height <= height + 2 || area.width < width as u16 {
        return;
    }
    let x = area.x + (area.width.saturating_sub(width as u16)) / 2;
    let y = area.y + (area.height - height) / 2;

    frame.render_widget(
        Paragraph::new(Text::from(lines)),
        Rect {
            x,
            y,
            width: width as u16,
            height,
        },
    );
}

/// Live one-line status above the cached body while the turn streams, at the
/// same three-column indent as the rest of the assistant content. Terminal
/// states render in the body itself: failures as `Error: …`; interrupts
/// simply end the turn.
fn status_line(
    entry: &Entry,
    theme: &Theme,
    spinner: usize,
    eta: Option<std::time::Duration>,
    rail: bool,
) -> Option<Line<'static>> {
    if entry.phase != Phase::Streaming {
        return None;
    }
    let elapsed = entry.elapsed.unwrap_or_else(|| entry.started.elapsed());
    let running_tools = entry.tools.iter().filter(|t| t.running).count();
    let label = if running_tools > 0 {
        format!("running {}", text::plural(running_tools, "tool"))
    } else {
        "thinking".to_string()
    };
    let mut spans = vec![
        Span::raw(if rail { RAIL } else { "   " }.to_string()),
        Span::styled(
            format!("{} ", glyph::SPINNER[spinner % glyph::SPINNER.len()]),
            Style::default().fg(theme.accent()),
        ),
        Span::styled(label, Style::default().fg(theme.muted())),
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
    // ETA from the model's recent history, once it has enough turns.
    if let Some(eta) = eta {
        let secs = eta.as_secs();
        let label = if secs < 60 {
            format!("~{secs}s")
        } else {
            format!("~{}m", secs / 60)
        };
        spans.push(Span::styled(
            format!("  · {label} left"),
            Style::default().fg(theme.accent_soft()),
        ));
    }
    Some(Line::from(spans))
}

impl Entry {
    /// Height of the cached body (excludes the dynamic status line and trailing gap).
    pub fn height(&self, compact: bool) -> u16 {
        self.cache
            .as_ref()
            .map(|c| c.height)
            .unwrap_or_else(|| self.estimated_height(80, compact))
    }

    /// Total virtual rows this entry owns: body + live status + the trailing
    /// separator gap (which compact density folds away).
    pub fn slot(&self, compact: bool) -> u16 {
        self.status_rows() + self.height(compact) + u16::from(!compact)
    }

    /// Cheap pre-render height so offscreen entries never touch markdown.
    pub fn estimated_height(&self, width: usize, compact: bool) -> u16 {
        let inner = width.saturating_sub(GUTTER).max(8);
        match self.role {
            EntryRole::User => {
                if compact {
                    // One `❯ text` row plus wrapped continuations.
                    let wrap_w = inner.saturating_sub(2).max(8);
                    let mut rows = 1usize;
                    for line in self.text.lines() {
                        let w = text::width(line) as u16;
                        rows += w.div_ceil(wrap_w as u16).max(1) as usize;
                    }
                    return rows.max(1) as u16;
                }
                let wrap_w = inner.saturating_sub(4).max(8);
                let mut rows = 0usize;
                for line in self.text.lines() {
                    let w = text::width(line) as u16;
                    rows += w.div_ceil(wrap_w as u16).max(1) as usize;
                }
                (self.tools.len() as u16) + rows.max(1) as u16
            }
            _ => {
                let indent = if compact { 3 } else { 4 };
                let text_w = inner.saturating_sub(indent).max(8);
                let mut h = self.tools.len() as u16;
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
        display: ToolDisplay,
        compact: bool,
        mode: (Color, &'static str),
    ) -> &Rendered {
        let stale = self.cache.as_ref().is_none_or(|c| {
            c.rev != self.rev
                || c.width as usize != width
                || c.tool_display != display
                || c.compact != compact
                || c.mode_color != mode.0
                || c.mode_label != mode.1
        });
        if stale {
            let (lines, code_blocks) = self.build(theme, width, display, compact, mode);
            // Link regions are computed in `lines` index space; the cache
            // stores u16s like every other row reference.
            let links = crate::ui::text::link_regions(&lines)
                .into_iter()
                .map(|(r, s, e, u)| (r as u16, s as u16, e as u16, u))
                .collect();
            let height = lines.len() as u16;
            self.cache = Some(Rendered {
                rev: self.rev,
                width: width as u16,
                tool_display: display,
                compact,
                mode_color: mode.0,
                mode_label: mode.1.to_string(),
                code_blocks,
                links,
                lines,
                height,
            });
        }
        self.cache.as_ref().expect("cache filled above")
    }

    /// Renders one tool call as an opencode-style row (`icon label · 2ms`)
    /// plus its preview body when `with_previews` is on and the tool has
    /// finished. Appends to `lines`.
    fn tool_row(
        &self,
        tool: &crate::app::ToolRow,
        theme: &Theme,
        inner: usize,
        rail: bool,
        with_previews: bool,
        lines: &mut Vec<Line<'static>>,
    ) {
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
        let prefix = if rail { BRANCH } else { "   " };
        // Duration is right-aligned into a calm metric column; a failed row
        // carries a short reason instead, so it explains itself in place.
        let duration = tool
            .elapsed
            .filter(|_| !tool.running)
            .map(text::format_duration);
        let reason = tool
            .error
            .as_deref()
            .map(|e| format!("failed: {}", text::truncate(e, 48)));
        let tail = duration.as_ref().map(|d| text::width(d) + 2).unwrap_or(0)
            + reason.as_ref().map(|r| text::width(r) + 3).unwrap_or(0);
        let budget = inner.saturating_sub(text::width(prefix) + 2 + tail).max(8);
        let mut spans = vec![
            Span::raw(prefix.to_string()),
            Span::styled(format!("{icon} "), style),
            Span::styled(text::truncate(&label, budget), style),
        ];
        if let Some(reason) = reason {
            spans.push(Span::styled(
                format!("  · {reason}"),
                Style::default().fg(theme.danger()),
            ));
        }
        if let Some(duration) = duration {
            let used: usize = spans.iter().map(|s| text::width(s.content.as_ref())).sum();
            let pad = inner.saturating_sub(used + text::width(&duration));
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            spans.push(Span::styled(duration, Style::default().fg(theme.faint())));
        }
        lines.push(Line::from(spans));

        // Bodies under the row: a command's live output while it runs (and
        // its tail after it finishes), otherwise the bounded args preview -
        // the code being written or edited, visible as it happens.
        let preview_prefix = if rail { "      " } else { "     " };
        let preview_w = inner.saturating_sub(text::width(preview_prefix));
        // For run_command the row label already carries the command line, so
        // the body shows the output instead; the tail is capped like a
        // preview so a chatty command cannot flood the transcript.
        const OUTPUT_MAX_LINES: usize = 12;
        let body: Option<String> = if tool.name == "run_command" {
            tool.output
                .as_deref()
                .map(|out| {
                    let lines: Vec<&str> = out.lines().collect();
                    let start = lines.len().saturating_sub(OUTPUT_MAX_LINES);
                    lines[start..].join("\n")
                })
                .filter(|out| !out.trim().is_empty())
        } else {
            tool.preview.clone()
        };
        if with_previews {
            if let Some(preview) = &body {
                if tool.name == "edit_file" {
                    let mut diff = DiffView::with_theme(*theme);
                    for raw in preview.split('\n') {
                        diff.push_diff(raw.to_string());
                    }
                    // Side-by-side: old left / new right with line numbers;
                    // degrades to unified inside `styled_split_lines` when
                    // the column is too narrow for two readable panels.
                    for line in diff.styled_split_lines(preview_w) {
                        let mut spans: Vec<Span> = vec![Span::raw(preview_prefix.to_string())];
                        spans.extend(
                            line.spans
                                .into_iter()
                                .map(|s| Span::styled(s.content.to_string(), s.style)),
                        );
                        lines.push(Line::from(spans));
                    }
                } else if tool.name == "write_file" {
                    // The code being written, colored like a fenced block.
                    // The extension drives the tokenizer, the same one the
                    // transcript's fences use.
                    let ext = tool
                        .detail
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or(&tool.detail)
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_ascii_lowercase())
                        .unwrap_or_default();
                    let st = text::MarkdownStyles::new(theme);
                    for line in text::highlighted_lines(preview, &ext, &st) {
                        for seg in text::wrap_spans(line.spans, preview_w) {
                            let mut spans: Vec<Span> = vec![Span::raw(preview_prefix.to_string())];
                            spans.extend(seg.spans);
                            lines.push(Line::from(spans));
                        }
                    }
                } else {
                    for raw in preview.split('\n') {
                        let style = if raw.starts_with('…') {
                            Style::default().fg(theme.faint())
                        } else {
                            Style::default().fg(theme.muted())
                        };
                        let shown = text::truncate(raw, preview_w);
                        if shown.is_empty() {
                            lines.push(Line::from(Span::raw(preview_prefix.to_string())));
                        } else {
                            lines.push(Line::from(Span::styled(
                                format!("{preview_prefix}{shown}"),
                                style,
                            )));
                        }
                    }
                }
            }
        }
    }

    /// Renders the entry's lines directly. Cached paths go through
    /// `ensure_rendered`; `pub(crate)` for height diagnostics in tests.
    #[allow(clippy::type_complexity)]
    pub(crate) fn build(
        &self,
        theme: &Theme,
        width: usize,
        display: ToolDisplay,
        compact: bool,
        mode: (Color, &'static str),
    ) -> (Vec<Line<'static>>, Vec<(u16, u16, String)>) {
        let inner = width.saturating_sub(GUTTER).max(8);
        let mut lines = Vec::new();
        let mut code_blocks = Vec::new();
        let (mode_color, _) = mode;
        let rail = !compact && self.role == EntryRole::Assistant;

        match self.role {
            EntryRole::System => {
                lines.push(Line::from(Span::styled(
                    format!("{} {}", glyph::DOT, self.text),
                    Style::default().fg(theme.faint()),
                )));
            }
            EntryRole::User => {
                if compact {
                    // Compact density: a single `❯ text` row, continuations
                    // indented - no raised block, no padding rows.
                    let wrap_w = inner.saturating_sub(2).max(8);
                    let text_style = Style::default().fg(theme.strong());
                    let mut first = true;
                    for raw in self.text.split('\n') {
                        if raw.trim().is_empty() {
                            lines.push(Line::default());
                            continue;
                        }
                        for seg in wrap_spans(vec![Span::raw(raw.to_string())], wrap_w) {
                            let content: String =
                                seg.spans.iter().map(|s| s.content.as_ref()).collect();
                            let marker = if first {
                                format!("{} ", glyph::PROMPT)
                            } else {
                                "  ".to_string()
                            };
                            first = false;
                            lines.push(Line::from(vec![
                                Span::styled(marker, Style::default().fg(mode_color)),
                                Span::styled(content, text_style),
                            ]));
                        }
                    }
                    if first {
                        lines.push(Line::default());
                    }
                    return finish_build(lines, code_blocks);
                }
                // Rail-node prompt: a mode-coloured `◆` marks the user turn,
                // echoing the assistant indent. No fill, no badge;
                // wrapped lines hang under the text column.
                let node = Style::default().fg(mode_color);
                let text_style = Style::default().fg(theme.strong());
                let stamp = text::clock(self.at);
                let wrap_w = inner.saturating_sub(4).max(8);
                let mut first = true;
                for raw in self.text.split('\n') {
                    if raw.trim().is_empty() {
                        lines.push(Line::default());
                        continue;
                    }
                    for seg in wrap_spans(vec![Span::raw(raw.to_string())], wrap_w) {
                        let content: String =
                            seg.spans.iter().map(|s| s.content.as_ref()).collect();
                        let mut spans = if first {
                            vec![
                                Span::raw("  ".to_string()),
                                Span::styled(format!("{} ", glyph::NODE), node),
                                Span::styled(content, text_style),
                            ]
                        } else {
                            vec![
                                Span::raw("    ".to_string()),
                                Span::styled(content, text_style),
                            ]
                        };
                        if first {
                            // Right-align the wall-clock stamp when it fits.
                            let used: usize =
                                spans.iter().map(|s| text::width(s.content.as_ref())).sum();
                            let stamp_w = text::width(&stamp);
                            if used + 2 + stamp_w <= inner {
                                spans.push(Span::raw(" ".repeat(inner - used - stamp_w)));
                                spans.push(Span::styled(
                                    stamp.clone(),
                                    Style::default().fg(theme.faint()),
                                ));
                            }
                            first = false;
                        }
                        lines.push(Line::from(spans));
                    }
                }
                if first {
                    lines.push(Line::default());
                }
            }
            EntryRole::Assistant => {
                // Cozy density indents the turn one gutter; compact stays a
                // flat three-column log.
                let lead = if rail { RAIL } else { "   " };
                let mut st = text::MarkdownStyles::new(theme);
                st.rail = rail;
                // The model's reasoning lives in the left thinking panel; the
                // transcript carries only the answer and the tool activity.

                // opencode-style rows: everything indented three columns.
                // Tool activity renders per the display level: hidden (just
                // the response), rows only, or rows with preview bodies.
                if display != ToolDisplay::Hidden {
                    let with_previews = display == ToolDisplay::Full;
                    for tool in &self.tools {
                        self.tool_row(tool, theme, inner, rail, with_previews, &mut lines);
                    }
                    if !self.tools.is_empty() {
                        lines.push(Line::default());
                    }
                }

                if self.text.trim().is_empty() {
                    lines.push(Line::default());
                } else {
                    let base = lines.len();
                    let display_text = text::professionalize(&self.text);
                    let md_w = inner.saturating_sub(text::width(lead)).max(8);
                    let (md_lines, blocks) = text::markdown_with_blocks(&display_text, md_w, &st);
                    let opens: Vec<usize> = blocks.iter().map(|(s, _, _)| *s).collect();
                    let closes: Vec<usize> = blocks.iter().map(|(_, e, _)| *e).collect();
                    // A green node marks the start of the response, echoing
                    // the user turn's diamond; it occupies the same gutter
                    // cells as the plain indent, so wrapping never shifts.
                    let mut first_text = true;
                    for (idx, mut line) in md_lines.into_iter().enumerate() {
                        // Code blocks stay inside the turn's indent rather
                        // than rendering as a separate box; the fence's own
                        // rules mark the block's extent.
                        let mut spans = Vec::with_capacity(line.spans.len() + 2);
                        if rail && first_text {
                            spans.push(Span::raw("  ".to_string()));
                            spans.push(Span::styled(
                                format!("{} ", glyph::NODE),
                                Style::default().fg(theme.success()),
                            ));
                            first_text = false;
                        } else {
                            let prefix: &str = if !rail {
                                "   "
                            } else if opens.contains(&idx) {
                                FENCE_HEAD
                            } else if closes.contains(&idx) {
                                FENCE_FOOT
                            } else {
                                RAIL
                            };
                            spans.push(Span::raw(prefix.to_string()));
                        }
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
            }
        }

        if let Some(err) = &self.error {
            lines.push(Line::default());
            let (err_prefix, err_cont) = if rail {
                ("    Error: ", "           ")
            } else {
                ("   Error: ", "          ")
            };
            let hint_prefix = if rail { "    " } else { "   " };
            let wrapped = text::wrap_prefixed(
                err,
                err_prefix,
                Style::default().fg(theme.danger()),
                inner,
                err_cont,
            );
            if self.error_expanded {
                lines.extend(wrapped);
                lines.push(Line::from(Span::styled(
                    format!("{hint_prefix}… click to collapse"),
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
                            "{hint_prefix}… {} · click to expand",
                            text::plural(remaining, "more line")
                        ),
                        Style::default().fg(theme.faint()),
                    )));
                }
            }
        }

        finish_build(lines, code_blocks)
    }
}
