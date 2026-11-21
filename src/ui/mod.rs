pub mod chat;
pub mod composer;
pub mod diff;
pub mod panels;
use crate::ui::panels::draw_thinking_panel;
#[cfg(test)]
mod tests;
pub mod text;
pub mod theme;

use crate::app::{App, LeftPanel, Overlay, OverlayKind};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::Frame;
use theme::Theme;

/// Nominal height of the `/memory` record overlay; the rect clamps to the area.
const MEMORY_OVERLAY_ROWS: u16 = 28;

/// `NANOMITE_REDUCED_MOTION=1` freezes the spinner on frame zero. Everything
/// else keeps working; only the animation stops. Captured once so a frame
/// never probes the environment.
fn reduced_motion() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("NANOMITE_REDUCED_MOTION").is_some())
}

/// Cached frame geometry for the three-tile hacker layout (flipped T1).
/// Left column (30%): info panel (top) + chat input (bottom).
/// Right column (70%): main transcript + response.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rects {
    pub header: Rect,
    pub left_info: Rect,
    /// Rows between the info panel and the composer: prompt queue then
    /// follow-up suggestions. Zero height when neither is showing.
    pub left_aux: Rect,
    pub left_input: Rect,
    pub main_chat: Rect,
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
    pub layout: Option<(Rect, u16, u16, Rects)>,
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
        }
    }

    pub fn mark_dirty(&mut self) {
        self.offsets_dirty = true;
    }

    pub fn cached_chat_height(&self) -> Option<u16> {
        self.layout.map(|(_, _, _, r)| r.main_chat.height)
    }

    /// Get the cached chat area rect for mouse hit testing
    pub fn cached_chat_rect(&self) -> Option<Rect> {
        self.layout.map(|(_, _, _, r)| r.main_chat)
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

/// Rows needed by the two strips above the composer: queued prompts (header
/// plus up to three previews, with a `+N more` row) and follow-up suggestions
/// (header plus up to three rows). Kept in one place so the layout split and
/// the renderers agree on geometry.
fn aux_rows(app: &App) -> (u16, u16) {
    const MAX_SHOWN: usize = 3;
    let queued = app.prompt_queue_len();
    let queue_rows = if queued == 0 {
        0
    } else {
        1 + queued.min(MAX_SHOWN) + usize::from(queued > MAX_SHOWN)
    };
    let suggestion_rows = app
        .suggestions
        .as_ref()
        .map(|strip| 1 + strip.items.len().min(MAX_SHOWN))
        .unwrap_or(0);
    (queue_rows as u16, suggestion_rows as u16)
}

/// Draws the whole frame.
pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let theme = app.theme;
    if area.width < 24 || area.height < 8 {
        if area.width >= 20 && area.height >= 1 {
            let hint = format!("terminal too small - {}x{} needed", 24, 8);
            frame.render_widget(
                ratatui::widgets::Paragraph::new(ratatui::text::Line::from(
                    ratatui::text::Span::styled(hint, Style::default().fg(theme.warn())),
                )),
                Rect {
                    x: area.x,
                    y: area.y + (area.height - 1) / 2,
                    width: area.width,
                    height: 1,
                },
            );
        }
        return;
    }

    // The left input tile hugs the composer: measure the wrapped prompt at
    // the tile's inner text width (borders + `❯ ` gutter), give the composer
    // a comfortable 5-row base (3 text rows + borders, grows to 10 for long
    // input) and hand the rest of the column to the info panel.
    let left_w = left_tile_width(area) as usize;
    let wrap_w = left_w.saturating_sub(4).max(8);
    app.composer_layout_width = wrap_w;
    let prompt_layout = app
        .composer
        .layout(wrap_w, Style::default().fg(theme.user()));
    let composer_rows = if app.compact {
        composer::rows_compact(&prompt_layout)
    } else {
        composer::rows(&prompt_layout)
    };
    let min_input = if app.compact { 1 } else { 5 };
    let input_h = composer_rows.max(min_input);
    let (queue_rows, suggestion_rows) = aux_rows(app);
    // A pending approval / toast / search gets its own reserved bottom row so
    // the full-width status bar never paints over the composer's bottom border.
    let has_status_bar = composer::has_priority_status(app) && area.height >= 2;
    let layout_area = if has_status_bar {
        Rect {
            height: area.height - 1,
            ..area
        }
    } else {
        area
    };
    let rects = app
        .view
        .layout(layout_area, input_h, queue_rows + suggestion_rows);

    panels::header(frame, rects.header, app);
    // Left info column: the `/activity` cycle selects one of four live
    // panels. Thinking still respects its own visibility flag.
    match app.left_panel {
        LeftPanel::Activity => panels::activity_timeline(frame, rects.left_info, app),
        LeftPanel::LiveDiff => panels::live_diff_viewer(frame, rects.left_info, app),
        LeftPanel::ToolMonitor => panels::tool_monitor(frame, rects.left_info, app),
        LeftPanel::Thinking if app.show_thinking => {
            draw_thinking_panel(frame, rects.left_info, app, &theme);
        }
        LeftPanel::Thinking => {}
    }

    // Aux strips ride between the info panel and the composer. Both renderers
    // are no-ops when their state is empty, so the split is cheap.
    if rects.left_aux.height > 0 {
        let mut y = rects.left_aux.y;
        if queue_rows > 0 {
            let h = queue_rows.min(rects.left_aux.height);
            chat::render_prompt_queue(
                frame,
                Rect {
                    y,
                    height: h,
                    ..rects.left_aux
                },
                app,
            );
            y += h;
        }
        if suggestion_rows > 0 && y < rects.left_aux.y + rects.left_aux.height {
            let h = (rects.left_aux.y + rects.left_aux.height - y).min(suggestion_rows);
            panels::suggestions(
                frame,
                Rect {
                    y,
                    height: h,
                    ..rects.left_aux
                },
                app,
            );
        }
    }

    chat::render(frame, rects.main_chat, app);
    let cursor = composer::render(frame, rects.left_input, app, &prompt_layout, app.compact);

    // Transient action bar: when a tool approval, toast or transcript search
    // needs attention, it draws full-width over the bottom row in both
    // densities (the composer's own border line is too narrow for approval
    // prompts). The old permanent footer row is gone; this row exists only
    // while something actionable is pending.
    if has_status_bar {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(composer::status_line(app, area.width as usize)),
            Rect {
                x: area.x,
                y: area.y + area.height - 1,
                width: area.width,
                height: 1,
            },
        );
    }

    // Overlays render full-width on top of everything; they don't need to
    // respect the left info column since they are modal and painted last.
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
        Some(Overlay::Settings(_)) => {
            let rect =
                panels::overlay_rect(area, OverlayKind::Settings, panels::settings_row_count(app));
            panels::settings(frame, rect, app);
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
        Some(Overlay::Hacker(_)) => {
            let rect =
                panels::overlay_rect(area, OverlayKind::Hacker, panels::hacker_row_count(app));
            panels::hacker(frame, rect, app);
        }
        None => {}
    }

    // Approval is the one truly modal surface: it paints above overlays and
    // carries the full command plus the y/a/n/d legend, not a truncated line.
    panels::confirm_modal(frame, area, app);

    if app.is_busy() && !reduced_motion() {
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

/// Width of the left info/input column: 30% of the terminal, clamped so both
/// columns stay usable. Shared by `View::layout` and the composer measurement
/// so the wrapped row count always matches the painted width.
pub(crate) fn left_tile_width(area: Rect) -> u16 {
    let w = (area.width as f32 * 0.30).round() as u16;
    w.max(24).min(area.width.saturating_sub(24)).max(1)
}

impl View {
    fn layout(&mut self, area: Rect, input_h: u16, aux_h: u16) -> Rects {
        // Two-tile layout: header | (left: info+aux+input) | (right: main).
        // The old full-width footer row is gone (owner decision 2026-11-11):
        // mode and working state live in the header, transient status (tool
        // approval, toasts, search) rides the composer's bottom border.
        if let Some((cached_area, cached_input, cached_aux, rects)) = self.layout {
            if cached_area == area && cached_input == input_h && cached_aux == aux_h {
                return rects;
            }
        }

        let header_h = 1;
        let main_h = area.height.saturating_sub(header_h);

        // Header
        let header = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: header_h,
        };

        // Main area under the header
        let main_area = Rect {
            x: area.x,
            y: area.y + header_h,
            width: area.width,
            height: main_h,
        };

        // Horizontal split: left 30%, right 70%
        let left_w = left_tile_width(area);

        let left = Rect {
            x: main_area.x,
            y: main_area.y,
            width: left_w,
            height: main_area.height,
        };

        let right = Rect {
            x: main_area.x + left_w,
            y: main_area.y,
            width: main_area.width.saturating_sub(left_w),
            height: main_area.height,
        };

        // Left column vertical split: the composer hugs its content, the aux
        // strips ride above it, and the info panel occupies the rest. Always
        // leave the panel a few rows.
        let input_h = input_h.max(1).min(left.height.saturating_sub(3)).max(1);
        let aux_h = aux_h.min(left.height.saturating_sub(input_h + 3));
        let info_h = left.height.saturating_sub(input_h + aux_h);

        let left_info = Rect {
            x: left.x,
            y: left.y,
            width: left.width,
            height: info_h,
        };

        let left_aux = Rect {
            x: left.x,
            y: left.y + info_h,
            width: left.width,
            height: aux_h,
        };

        let left_input = Rect {
            x: left.x,
            y: left.y + info_h + aux_h,
            width: left.width,
            height: input_h,
        };

        let rects = Rects {
            header,
            left_info,
            left_aux,
            left_input,
            main_chat: right,
        };
        self.layout = Some((area, input_h, aux_h, rects));
        rects
    }
}
