pub mod chat;
use crate::ui::chat::render_prompt_queue;
pub mod composer;
pub mod diff;
pub mod panels;
#[cfg(test)]
mod tests;
pub mod text;
pub mod theme;

use crate::app::{App, Overlay, OverlayKind};
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::Frame;
use theme::Theme;

/// Nominal height of the `/memory` record overlay; the rect clamps to the area.
const MEMORY_OVERLAY_ROWS: u16 = 28;

/// Cached frame geometry: recomputed only when the terminal or composer resizes.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rects {
    pub header: Rect,
    pub chat: Rect,
    pub queue: Option<Rect>,
    pub suggestions: Option<Rect>,
    pub footer: Rect,
    pub composer: Rect,
}

/// Viewport + animation state. Everything here is redraw-cheap to read.
pub struct View {
    pub spinner: usize,
    pub scroll_from_bottom: u16,
    pub follow: bool,
    pub content_height: u16,
    /// Content rows that arrived while the user was scrolled away from the
    /// tail; drives the floating "new lines" pill.
    pub new_since_scroll: u16,
    /// Content height at the moment the user scrolled away.
    pub(crate) new_baseline: u16,

    pub(crate) offsets: Vec<u16>,
    offsets_dirty: bool,
    offset_width: u16,
    offset_view: u16,
    pub layout: Option<(Rect, u16, u16, u16, Rects)>,
    footer: Option<(String, Line<'static>)>,
}

impl View {
    pub fn new(theme: Theme) -> Self {
        let _ = theme;
        Self {
            spinner: 0,
            scroll_from_bottom: 0,
            follow: false,
            content_height: 0,
            new_since_scroll: 0,
            new_baseline: 0,
            offsets: Vec::new(),
            offsets_dirty: true,
            offset_width: 0,
            offset_view: 0,
            layout: None,
            footer: None,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.offsets_dirty = true;
    }

    pub fn cached_chat_height(&self) -> Option<u16> {
        self.layout.map(|(_, _, _, _, r)| r.chat.height)
    }

    /// Get the cached chat area rect for mouse hit testing
    pub fn cached_chat_rect(&self) -> Option<Rect> {
        self.layout.map(|(_, _, _, _, r)| r.chat)
    }

    /// Get the viewport row count (chat area height)
    pub fn viewport_rows(&self) -> u16 {
        self.cached_chat_height().unwrap_or(10)
    }

    pub fn invalidate_layout(&mut self) {
        self.layout = None;
        self.offsets_dirty = true;
    }

    /// Scroll helpers. All offsets are measured from the bottom so new content stays pinned.
    pub fn scroll_by(&mut self, delta: i32) {
        let was = self.follow;
        let next = self.scroll_from_bottom as i32 + delta;
        self.scroll_from_bottom = next.clamp(0, self.max_scroll() as i32) as u16;
        self.follow = self.scroll_from_bottom == 0;
        self.track_tail_state(was);
    }

    pub fn scroll_to_bottom(&mut self) {
        let was = self.follow;
        self.scroll_from_bottom = 0;
        self.follow = true;
        self.track_tail_state(was);
    }

    pub fn scroll_to_top(&mut self) {
        // Saturate instead of measuring `max_scroll()` here: the cached
        // viewport height can be stale (right after a resize, say), and the
        // next `rebuild_offsets` clamps the sentinel to the true maximum.
        self.scroll_from_bottom = u16::MAX;
        self.follow = false;
    }

    /// Scroll so that entry `entry_index` sits at the top of the viewport.
    /// Uses cached offsets from the last render; no-op if offsets are stale.
    pub fn scroll_to_entry_index(&mut self, entry_index: usize) {
        if entry_index < self.offsets.len().saturating_sub(1) {
            let entry_top = self.offsets[entry_index];
            let max_scroll = self.max_scroll();
            self.scroll_from_bottom = max_scroll.saturating_sub(entry_top);
            self.follow = self.scroll_from_bottom == 0;
        }
    }

    /// Records the baseline when the user scrolls away from the tail and
    /// clears the counter when they return — the input for the floating
    /// "N new lines" pill.
    fn track_tail_state(&mut self, was_following: bool) {
        if self.follow {
            self.new_since_scroll = 0;
            self.new_baseline = self.content_height;
        } else if was_following {
            self.new_baseline = self.content_height;
            self.new_since_scroll = 0;
        }
    }

    fn max_scroll(&self) -> u16 {
        self.content_height.saturating_sub(self.offset_view)
    }

    /// `(top, bottom)` virtual rows currently visible.
    pub fn window(&self) -> (u16, u16) {
        let max = self.max_scroll();
        let from_bottom = self.scroll_from_bottom.min(max);
        let top = max - from_bottom;
        (top, top + self.offset_view)
    }

    /// Cumulative virtual row offsets for every entry plus a trailing sentinel.
    pub fn rebuild_offsets(
        &mut self,
        entries: &[crate::app::Entry],
        width: usize,
        view_h: u16,
        compact: bool,
    ) {
        let view_h_eff = view_h;
        if !self.offsets_dirty
            && self.offset_width == width as u16
            && self.offset_view == view_h_eff
            && self.offsets.len() == entries.len() + 1
        {
            return;
        }
        self.offsets.clear();
        self.offsets.reserve(entries.len() + 1);
        let mut acc = 0u16;
        self.offsets.push(acc);
        for entry in entries {
            acc = acc.saturating_add(entry.slot(compact));
            self.offsets.push(acc);
        }
        self.content_height = acc;
        self.offset_width = width as u16;
        self.offset_view = view_h_eff;
        self.offsets_dirty = false;
        let max = self.max_scroll();
        if self.scroll_from_bottom > max {
            self.scroll_from_bottom = max;
        }
        // New rows streamed in while scrolled away show up in the pill.
        if self.follow {
            self.new_since_scroll = 0;
            self.new_baseline = acc;
        } else {
            self.new_since_scroll = acc.saturating_sub(self.new_baseline);
        }
    }
}

/// Rough line count of the help sheet, used for scrolling and sizing.
pub fn help_row_count() -> usize {
    // 1 commands header + N command rows + 1 blank + 1 keys header
    // + the keys table itself, so the count can never drift from the sheet.
    crate::commands::COMMANDS.len() + 3 + panels::help_keys().len()
}

/// Draws the whole frame.
pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        return;
    }
    let theme = app.theme;

    // The composer metrics are needed to solve the vertical layout, so measure it first.
    let inner_width = area.width.saturating_sub(6).max(8) as usize;
    // Key handling measures wrapping at the painted width; keep the cached
    // value in sync so Up/Down behave identically at any terminal size.
    app.composer_layout_width = inner_width;
    let prompt_layout = app.composer.layout(inner_width, theme.base());
    let composer_rows = if app.compact {
        composer::rows_compact(&prompt_layout)
    } else {
        composer::rows(&prompt_layout)
    };

    let queue_rows = if app.prompt_queue_len() > 0 {
        app.prompt_queue_len() as u16 + 1
    } else {
        0
    };
    // The follow-up strip mounts only while suggestions are on screen:
    // one header row plus one row per suggestion.
    let suggest_rows = match &app.suggestions {
        Some(strip) => strip.items.len() as u16 + 1,
        None => 0,
    };
    let rects = app
        .view
        .layout(area, composer_rows, queue_rows, suggest_rows);

    panels::header(frame, rects.header, app);
    chat::render(frame, rects.chat, app);
    if let Some(queue_rect) = rects.queue {
        render_prompt_queue(frame, queue_rect, app);
    }
    if let Some(suggest_rect) = rects.suggestions {
        panels::suggestions(frame, suggest_rect, app);
    }
    panels::footer(frame, rects.footer, app);

    let cursor = composer::render(frame, rects.composer, app, &prompt_layout, app.compact);

    match app.overlay.clone() {
        Some(Overlay::Picker(picker)) => {
            let rows = (picker.items.len() as u16).saturating_add(2);
            let rect = panels::overlay_rect(area, OverlayKind::Picker, rows);
            panels::picker(frame, rect, app);
        }
        Some(Overlay::Providers(_)) => {
            let rect = panels::overlay_rect(area, OverlayKind::Providers, 22);
            panels::providers(frame, rect, app);
        }
        Some(Overlay::Connect(_)) => {
            let rect = panels::overlay_rect(area, OverlayKind::Connect, 5);
            panels::connect(frame, rect, app);
        }
        Some(Overlay::Help(_)) => {
            let rect = panels::overlay_rect(area, OverlayKind::Help, help_row_count() as u16);
            panels::help(frame, rect, app);
        }
        Some(Overlay::Tools(_)) => {
            let rect = panels::overlay_rect(area, OverlayKind::Tools, panels::tools_row_count());
            panels::tools(frame, rect, app);
        }
        Some(Overlay::Board(_)) => {
            let rect = panels::overlay_rect(area, OverlayKind::Board, panels::board_row_count(app));
            panels::board(frame, rect, app);
        }
        Some(Overlay::Memory(_)) => {
            let rect = panels::overlay_rect(area, OverlayKind::Memory, MEMORY_OVERLAY_ROWS);
            panels::memory(frame, rect, app);
        }
        Some(Overlay::Improve(_)) => {
            let rect =
                panels::overlay_rect(area, OverlayKind::Improve, panels::improve_row_count(app));
            panels::improve(frame, rect, app);
        }
        Some(Overlay::SudoPassword(_)) => {
            let rect = panels::overlay_rect(
                area,
                OverlayKind::SudoPassword,
                panels::sudo_password_row_count(app),
            );
            panels::sudo_password(frame, rect, app);
        }
        None => {}
    }

    if app.is_busy() {
        app.view.spinner = app.view.spinner.wrapping_add(1);
    }
    if let Some((_, x, y)) = cursor {
        if app.overlay.is_none() && app.composer_focused() {
            frame.set_cursor_position((x, y));
        } else {
            frame.set_cursor_position((0, 0));
        }
    }
}

impl View {
    fn layout(
        &mut self,
        area: Rect,
        composer_rows: u16,
        queue_rows: u16,
        suggest_rows: u16,
    ) -> Rects {
        let has_queued = queue_rows > 0;
        let has_suggestions = suggest_rows > 0;

        if let Some((cached_area, cached_rows, cached_queue, cached_suggest, rects)) = self.layout {
            if cached_area == area
                && cached_rows == composer_rows
                && cached_queue == queue_rows
                && cached_suggest == suggest_rows
            {
                return rects;
            }
        }
        let areas = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(3),
            ratatui::layout::Constraint::Length(queue_rows),
            ratatui::layout::Constraint::Length(suggest_rows),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(composer_rows),
        ])
        .horizontal_margin(1)
        .split(area);

        let rects = Rects {
            header: areas[0],
            chat: areas[1],
            queue: if has_queued { Some(areas[2]) } else { None },
            suggestions: if has_suggestions {
                Some(areas[3])
            } else {
                None
            },
            footer: areas[4],
            composer: areas[5],
        };
        self.layout = Some((area, composer_rows, queue_rows, suggest_rows, rects));
        rects
    }
}
