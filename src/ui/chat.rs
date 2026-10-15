use super::text::{self, wrap_spans};
use super::theme::{glyph, Theme};
use crate::app::{Entry, EntryRole, Phase, Rendered};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation};
use ratatui::Frame;

/// Width of the left rail (`▌ `) drawn next to every transcript row.
pub const GUTTER: usize = 2;

/// Transcripts up to this many entries get measured exactly before drawing.
pub const WARM_LIMIT: usize = 400;

pub fn render(frame: &mut Frame, area: Rect, app: &mut crate::app::App) {
    if area.width < 12 || area.height == 0 {
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
    // One column is always reserved on the right so the scrollbar never overlaps text
    // and the wrap width does not change when the bar appears.
    let width = area.width.saturating_sub(1) as usize;
    let view_h = area.height;

    // 1. Offsets come from cached heights; entries outside the window use a cheap estimate.
    view.rebuild_offsets(entries, width, view_h);
    let (mut top, window_bottom) = view.window();

    // Warm the render cache, then rebuild offsets so scrolling uses exact heights.
    // Small transcripts are measured in full; large ones get the window plus a margin so
    // scrolling forward fills in real heights instead of estimates.
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
        let rail = rail_style(&entries[idx], &theme);
        // User prompts and assistant replies render as borderless filled bubbles;
        // the `▌` rail is reserved for system lines.
        let bubbled = entries[idx].role != EntryRole::System;
        let entry = &mut entries[idx];
        let cached = entry.ensure_rendered(&theme, width, show_details);

        let batch_start = row;
        let mut batch: Vec<Line<'static>> = Vec::with_capacity((bottom - row) as usize);
        while row < bottom && row < entry_end {
            let local = (row - entry_start) as usize;
            let line = if local < status_rows {
                status.as_ref().map(|l| {
                    if bubbled {
                        l.clone()
                    } else {
                        gutter_line(l.clone(), rail)
                    }
                })
            } else {
                // Past the body this is the blank separator row between entries.
                let body = local - status_rows;
                match cached.lines.get(body) {
                    Some(l) => Some(if bubbled {
                        l.clone()
                    } else {
                        gutter_line(l.clone(), rail)
                    }),
                    None => Some(Line::default()),
                }
            };
            batch.push(line.unwrap_or_default());
            row += 1;
        }

        let height = batch.len() as u16;
        if height > 0 {
            frame.render_widget(
                Paragraph::new(ratatui::text::Text::from(batch)),
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
            .thumb_symbol("┃")
            .thumb_style(Style::default().fg(theme.border_focus()))
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

/// Centred placeholder shown until the first prompt is sent.
fn render_welcome(frame: &mut Frame, area: Rect, app: &crate::app::App, theme: &Theme) {
    let rows: Vec<(String, String)> = vec![
        ("model".into(), app.model_label()),
        ("provider".into(), app.provider_label()),
        ("tools".into(), "files · shell · ssh · web".into()),
        ("commands".into(), "/ or ctrl+x opens the menu".into()),
    ];

    let card_w = (area.width as usize).saturating_sub(8).clamp(20, 52);
    // Molecule mark: a honeycomb cluster beside the wordmark (Option C).
    let hex = Style::default().fg(theme.accent());
    let logo = vec![
        Line::from(Span::styled("    \u{2b21}\u{2b21}\u{2b21}", hex)),
        Line::from(vec![
            Span::styled("   \u{2b21}\u{2b21}\u{2b21}\u{2b21}\u{2b21}  ", hex),
            Span::styled("nanomite", theme.bold()),
        ]),
        Line::from(Span::styled("    \u{2b21}\u{2b21}\u{2b21}", hex)),
        Line::from(Span::styled("  the microscopic agent", theme.faint_style())),
    ];
    let lines: Vec<Line<'static>> = logo
        .into_iter()
        .chain(std::iter::once(Line::default()))
        .chain(std::iter::once(Line::default()))
        .chain(rows.into_iter().map(|(k, v)| {
            Line::from(vec![
                Span::styled(format!("  {k:<10}"), Style::default().fg(theme.faint())),
                Span::styled(text::truncate(&v, card_w.saturating_sub(12)), theme.dim()),
            ])
        }))
        .chain(std::iter::once(Line::default()))
        .chain(std::iter::once(Line::from(Span::styled(
            format!("{} ask anything to begin", glyph::CHEVRON),
            theme.accent_style(),
        ))))
        .collect();

    let height = lines.len() as u16;
    if area.height <= height {
        return;
    }
    let x = area.x + (area.width.saturating_sub(card_w as u16)) / 2;
    let y = area.y + (area.height - height) / 2;
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(lines)),
        Rect {
            x,
            y,
            width: card_w as u16,
            height,
        },
    );
}

fn rail_style(entry: &Entry, theme: &Theme) -> Style {
    match entry.role {
        EntryRole::User => Style::default().fg(theme.accent()),
        EntryRole::Assistant => match entry.phase {
            Phase::Streaming => Style::default().fg(theme.accent_soft()),
            Phase::Failed => Style::default().fg(theme.danger()),
            _ => Style::default().fg(theme.border_focus()),
        },
        EntryRole::System => Style::default().fg(theme.border()),
    }
}

/// Live one-line status (spinner / error / summary) drawn above the cached body.
fn status_line(entry: &Entry, theme: &Theme, spinner: usize) -> Option<Line<'static>> {
    match entry.phase {
        Phase::Streaming => {
            let elapsed = entry.elapsed.unwrap_or_else(|| entry.started.elapsed());
            Some(Line::from(vec![
                Span::styled(
                    format!("{} ", glyph::SPINNER[spinner % glyph::SPINNER.len()]),
                    Style::default().fg(theme.accent()),
                ),
                Span::styled("thinking", Style::default().fg(theme.muted())),
                Span::styled(
                    format!("  {}", text::format_duration(elapsed)),
                    Style::default().fg(theme.faint()),
                ),
            ]))
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

fn gutter_line(line: Line<'static>, rail: Style) -> Line<'static> {
    if line.spans.is_empty() {
        return Line::default();
    }
    let mut spans = Vec::with_capacity(line.spans.len() + 1);
    spans.push(Span::styled(format!("{} ", glyph::GUTTER), rail));
    spans.extend(line.spans);
    Line::from(spans)
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
            // Prompts: quote marker + 2-col indent, no label row.
            EntryRole::User => {
                let wrap_w = inner.saturating_sub(2).max(8);
                let mut rows = 0usize;
                for line in self.text.lines() {
                    let w = text::width(line) as u16;
                    rows += w.div_ceil(wrap_w as u16).max(1) as usize;
                }
                (self.tools.len() as u16) + rows.max(1) as u16
            }
            _ => {
                let mut h = self.tools.len() as u16;
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
                // opencode-minimal prompt: quote marker, dimmed text, indented.
                // Roles are carried by alignment and tone - no label rows.
                let marker = Style::default().fg(theme.faint());
                let style = Style::default().fg(theme.muted());
                let wrap_w = inner.saturating_sub(2).max(8);
                let mut first = true;
                for raw in self.text.split('\n') {
                    if raw.trim().is_empty() {
                        lines.push(Line::from(Span::raw("  ")));
                        continue;
                    }
                    lines.extend(
                        wrap_spans(vec![Span::raw(raw.to_string())], wrap_w)
                            .into_iter()
                            .map(|mut l| {
                                for span in l.spans.iter_mut() {
                                    *span = Span::styled(span.content.to_string(), style);
                                }
                                let mut spans: Vec<Span> = Vec::with_capacity(l.spans.len() + 2);
                                spans.push(Span::styled(
                                    if first { "\u{203a} " } else { "  " },
                                    marker,
                                ));
                                first = false;
                                spans.extend(l.spans);
                                Line::from(spans)
                            }),
                    );
                }
                if first {
                    lines.push(Line::from(Span::raw("  ")));
                }
            }
            EntryRole::Assistant => {
                // Reply body: tools grouped, then full-width markdown. No label.
                for tool in &self.tools {
                    let (marker, marker_style) = if tool.running {
                        (glyph::HOURGLASS, Style::default().fg(theme.warn()))
                    } else if tool.ok {
                        (glyph::CHECK, Style::default().fg(theme.success()))
                    } else {
                        (glyph::CROSS, Style::default().fg(theme.danger()))
                    };
                    // `/details` off keeps the row to just the tool name.
                    let detail = if details {
                        text::truncate(
                            &tool.detail,
                            inner.saturating_sub(tool.name.chars().count() + 6),
                        )
                    } else {
                        String::new()
                    };
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(format!("{marker} "), marker_style),
                        Span::styled(
                            tool.name.clone(),
                            Style::default()
                                .fg(theme.tool())
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            if detail.is_empty() {
                                String::new()
                            } else {
                                format!("  {detail}")
                            },
                            Style::default().fg(theme.faint()),
                        ),
                    ]));
                    // Live preview of what the tool is writing/running
                    // (OpenCode-style): bounded at construction, rendered under
                    // the row while the tool runs. `/details` off collapses it.
                    if details {
                        if let Some(preview) = &tool.preview {
                            for raw in preview.split('\n') {
                                let style = if raw.starts_with("- ") {
                                    Style::default().fg(theme.danger())
                                } else if raw.starts_with("+ ") {
                                    Style::default().fg(theme.success())
                                } else if raw.starts_with('…') {
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
                            chars.pop();
                            s.content = chars.into_iter().collect();
                        }
                        last.spans.push(Span::styled(
                            "\u{258d}",
                            Style::default().fg(theme.accent()),
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
