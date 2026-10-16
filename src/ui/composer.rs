use super::text;
use super::theme::glyph;
use crate::app::App;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};
use ratatui::Frame;
use std::collections::VecDeque;

/// Multi-line prompt buffer with cursor, history and wrapped-row math.
#[derive(Clone)]
pub struct Composer {
    text: String,
    /// Cursor position as a char index.
    cursor: usize,
    history: VecDeque<String>,
    history_idx: Option<usize>,
    draft: String,
}

impl std::fmt::Debug for Composer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Composer")
            .field("text", &self.text)
            .field("cursor", &self.cursor)
            .finish()
    }
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

impl Composer {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            history: VecDeque::new(),
            history_idx: None,
            draft: String::new(),
        }
    }

    pub fn value(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Caret position as a char index.
    #[cfg(test)]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Empties the buffer and returns its contents.
    pub fn drain(&mut self) -> String {
        let text = std::mem::take(&mut self.text);
        self.cursor = 0;
        self.history_idx = None;
        text
    }

    /// Pre-filled buffer with the caret at the end (used by the `/connect` form).
    pub fn filled(text: String) -> Self {
        let mut composer = Self::new();
        composer.insert_str(&text);
        composer
    }

    pub fn insert(&mut self, ch: char) {
        let idx = self.byte_index(self.cursor);
        self.text.insert(idx, ch);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        for ch in s.chars() {
            self.insert(ch);
        }
    }

    pub fn newline(&mut self) {
        self.insert('\n');
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let idx = self.byte_index(self.cursor - 1);
            self.text.remove(idx);
            self.cursor -= 1;
        }
    }

    pub fn delete_forward(&mut self) {
        if self.cursor < self.chars_len() {
            let idx = self.byte_index(self.cursor);
            self.text.remove(idx);
        }
    }

    /// Like `ctrl+w`: trailing whitespace first, then the word behind the cursor.
    pub fn delete_word_back(&mut self) {
        let whitespace_first = self.before_cursor().is_some_and(|c| c.is_whitespace());
        while self.cursor > 0 {
            let is_space = self.before_cursor().is_some_and(|c| c.is_whitespace());
            if is_space == whitespace_first {
                self.backspace();
            } else {
                break;
            }
        }
    }

    pub fn delete_to_start(&mut self) {
        let idx = self.byte_index(self.cursor);
        self.text.drain(..idx);
        self.cursor = 0;
    }

    pub fn delete_to_end(&mut self) {
        let idx = self.byte_index(self.cursor);
        self.text.truncate(idx);
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.history_idx = None;
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.chars_len());
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.chars_len();
    }

    pub fn move_word_left(&mut self) {
        let mut idx = self.cursor;
        while idx > 0 {
            let prev = self.char_at(idx - 1);
            if prev.is_some_and(|c| c.is_whitespace()) {
                break;
            }
            idx -= 1;
        }
        while idx > 0 && self.char_at(idx - 1).is_some_and(|c| c.is_whitespace()) {
            idx -= 1;
        }
        self.cursor = idx;
    }

    pub fn move_word_right(&mut self) {
        let len = self.chars_len();
        let mut idx = self.cursor;
        while idx < len && self.char_at(idx).is_some_and(|c| c.is_whitespace()) {
            idx += 1;
        }
        while idx < len && self.char_at(idx).is_some_and(|c| !c.is_whitespace()) {
            idx += 1;
        }
        self.cursor = idx;
    }

    /// Moves the cursor one visual row up/down inside the wrapped layout.
    pub fn move_row(&mut self, layout: &Layout, delta: i32) {
        let Some((row, col)) = layout.cursor_position(self.cursor) else {
            return;
        };
        let Ok(target) = usize::try_from(row as i64 + delta as i64) else {
            return;
        };
        if let Some(idx) = layout.char_index_at(target, col) {
            self.cursor = idx;
        }
    }

    /// Consumes the buffer, remembering it in history.
    pub fn submit(&mut self) -> Option<String> {
        let value = self.text.trim().to_string();
        if value.is_empty() {
            return None;
        }
        self.history.push_back(value.clone());
        while self.history.len() > 200 {
            self.history.pop_front();
        }
        self.clear();
        Some(value)
    }

    pub fn recall_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.history_idx {
            Some(current) => current.saturating_sub(1),
            None => {
                // First recall jumps to the most recent prompt, like a shell.
                self.draft = self.text.clone();
                self.history.len() - 1
            }
        };
        self.history_idx = Some(idx);
        self.set_from_history();
    }

    pub fn recall_next(&mut self) {
        let Some(idx) = self.history_idx else { return };
        let next = idx + 1;
        if next >= self.history.len() {
            self.history_idx = None;
            self.text = std::mem::take(&mut self.draft);
        } else {
            self.history_idx = Some(next);
            self.set_from_history();
        }
        self.cursor = self.chars_len();
    }

    fn set_from_history(&mut self) {
        if let Some(idx) = self.history_idx {
            if let Some(entry) = self.history.get(idx) {
                self.text = entry.clone();
                self.cursor = self.chars_len();
            }
        }
    }

    fn chars_len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte_index(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.text.len())
    }

    fn char_at(&self, idx: usize) -> Option<char> {
        self.text.chars().nth(idx)
    }

    fn before_cursor(&self) -> Option<char> {
        self.cursor.checked_sub(1).and_then(|i| self.char_at(i))
    }

    /// Builds the wrapped visual layout for a given inner text width.
    pub fn layout(&self, inner_width: usize, style: Style) -> Layout {
        Layout::build(&self.text, self.cursor, inner_width.max(1), style)
    }
}

/// Visual layout of the composer buffer for a given width.
#[derive(Debug, Clone)]
pub struct Layout {
    pub lines: Vec<Line<'static>>,
    /// Char index where each visual row starts (plus a final sentinel).
    pub row_starts: Vec<usize>,
    /// Char offset of the cursor within its visual row.
    pub cursor: (usize, usize),
}

impl Layout {
    fn build(text: &str, cursor_idx: usize, width: usize, style: Style) -> Self {
        let logical: Vec<&str> = text.split('\n').collect();
        let mut lines = Vec::new();
        let mut row_starts = Vec::new();
        let mut char_pos = 0usize;

        for l in logical.iter() {
            // `wrap_ranges` never returns an empty vec — an empty logical line
            // still yields one (empty) visual row, so a trailing newline gets
            // its row and interior blanks render as real blank rows.
            for (start, end) in wrap_ranges(l, width) {
                row_starts.push(char_pos + start);
                let content: String = l.chars().skip(start).take(end - start).collect();
                lines.push(Line::from(Span::styled(content, style)));
            }
            char_pos += l.chars().count() + 1;
        }

        if lines.is_empty() {
            lines.push(Line::default());
            row_starts.push(0);
        }

        let mut cursor = (0usize, 0usize);
        for (row, start) in row_starts.iter().enumerate() {
            let len = row_len(&lines[row]);
            let end = start + len;
            if cursor_idx >= *start && cursor_idx < end {
                cursor = (row, cursor_idx - start);
                break;
            }
            if cursor_idx == end {
                let next_start = row_starts.get(row + 1).copied();
                cursor = match next_start {
                    Some(next) if next == cursor_idx => (row + 1, 0),
                    _ => (row, len),
                };
                break;
            }
        }

        let _ = style;
        Self {
            lines,
            row_starts,
            cursor,
        }
    }

    pub fn rows(&self) -> usize {
        self.lines.len()
    }

    /// Visual `(row, col)` of the cursor captured when the layout was built.
    pub fn cursor_position(&self, _cursor: usize) -> Option<(usize, usize)> {
        Some(self.cursor)
    }

    /// Best-effort char index for a visual row/column.
    pub fn char_index_at(&self, row: usize, col: usize) -> Option<usize> {
        let start = self.row_starts.get(row)?;
        Some(start + col.min(row_len(&self.lines[row])))
    }
}

fn row_len(line: &Line<'_>) -> usize {
    line.spans.iter().map(|s| s.content.chars().count()).sum()
}

/// Greedy word wrap expressed as char-index ranges into `line`.
fn wrap_ranges(line: &str, width: usize) -> Vec<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return vec![(0, 0)];
    }
    if text::width(line) <= width {
        return vec![(0, chars.len())];
    }

    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut col = 0usize;
    let mut i = 0usize;
    let mut last_break = None;

    while i < chars.len() {
        let cw = unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(0);
        if chars[i] == ' ' {
            last_break = Some(i);
        }
        if col + cw > width {
            let split = match last_break {
                Some(b) if b > start => b,
                _ => i,
            };
            let split = if split == start { i } else { split };
            ranges.push((start, split));
            start = split + 1;
            col = 0;
            last_break = None;
            i = start;
            continue;
        }
        col += cw;
        i += 1;
    }
    if start < chars.len() {
        ranges.push((start, chars.len()));
    }
    if ranges.is_empty() {
        ranges.push((0, chars.len()));
    }
    ranges
}

// ---------------------------------------------------------------------------
// Widget rendering
// ---------------------------------------------------------------------------

/// Shortest composer body (keeps the chatbox roomy even when empty).
pub const MIN_INNER_ROWS: usize = 3;

/// Longest composer expansion before the viewport starts scrolling internally.
pub const MAX_INNER_ROWS: usize = 8;

/// Total composer height for a measured layout: the text rows plus the
/// rounded border. Hints live on the bottom border, so the inner rows belong
/// entirely to the text.
pub fn rows(layout: &Layout) -> u16 {
    (layout.rows().clamp(MIN_INNER_ROWS, MAX_INNER_ROWS) + 2) as u16
}

/// Returns `(first_visible_row, x, y)` for the text cursor.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    layout: &Layout,
) -> Option<(usize, u16, u16)> {
    if area.width < 8 || area.height < 3 {
        return None;
    }
    let theme = &app.theme;
    let busy = app.is_busy();
    let focused = app.overlay.is_none();

    // Rounded block: the workspace path sits on the top border and the key
    // hints on the bottom one. The border itself separates transcript from
    // input, so every inner row belongs to the prompt text.
    let hint_text = if busy {
        "esc interrupts \u{b7} type to queue the next turn"
    } else {
        "enter send \u{b7} ctrl+j newline \u{b7} / commands"
    };
    let path_w = (area.width as usize).saturating_sub(6).max(8);
    let hint_w = (area.width as usize).saturating_sub(4);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused {
            if busy {
                // While the turn runs the composer reads as "still yours": the
                // soft accent draws the eye without the alarm of border_focus.
                theme.accent_soft()
            } else {
                theme.border_focus()
            }
        } else {
            theme.border()
        }))
        .title(Line::from(Span::styled(
            format!(" {} ", text::display_path(&app.cwd, path_w)),
            theme.faint_style(),
        )))
        .title_bottom(
            Line::from(Span::styled(
                text::truncate(hint_text, hint_w),
                theme.faint_style(),
            ))
            .right_aligned(),
        );

    // The content area inside the block border.
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 4 || inner.height == 0 {
        return None;
    }

    let text_top = inner.y;
    let text_h = inner.height as usize;

    let prompt = Span::styled(
        format!("{} ", glyph::PROMPT),
        Style::default().fg(if focused {
            theme.accent()
        } else {
            theme.muted()
        }),
    );
    frame.render_widget(
        Paragraph::new(prompt),
        Rect {
            x: inner.x,
            y: text_top,
            width: 2,
            height: 1,
        },
    );

    let text_x = inner.x + 2;
    let text_w = inner.width.saturating_sub(2);

    if app.composer.is_empty() {
        let hint = if busy {
            "the agent is working - type to queue the next turn"
        } else {
            "ask anything - / opens the command menu"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                text::truncate(hint, text_w as usize),
                // The placeholder keeps the accent tone at low intensity so the
                // empty composer still reads as "yours" without shouting.
                if busy {
                    theme.faint_style()
                } else {
                    Style::default().fg(theme.accent_soft())
                },
            )),
            Rect {
                x: text_x,
                y: text_top,
                width: text_w,
                height: 1,
            },
        );
        return Some((0, text_x, text_top));
    }

    // Keep the cursor row visible when the buffer exceeds the box.
    let cursor_row = layout.cursor.0;
    let first = cursor_row.saturating_sub(text_h - 1);
    let visible: Vec<Line<'static>> = layout.lines.iter().skip(first).cloned().collect();
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(visible)),
        Rect {
            x: text_x,
            y: text_top,
            width: text_w,
            height: text_h as u16,
        },
    );

    let cursor_y = text_top + (cursor_row - first) as u16;
    let cursor_x = text_x + layout.cursor.1.min(text_w.saturating_sub(1) as usize) as u16;
    Some((first, cursor_x, cursor_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_tracks_cursor() {
        let mut c = Composer::new();
        c.insert_str("hello brave new world");
        let layout = c.layout(10, Style::default());
        assert!(layout.rows() >= 3);
        assert_eq!(layout.cursor.0, layout.rows() - 1);
    }

    #[test]
    fn word_movement_and_deletion() {
        let mut c = Composer::new();
        c.insert_str("alpha beta gamma");
        c.move_word_left();
        assert_eq!(c.cursor(), 10);
        c.delete_word_back();
        assert_eq!(c.value(), "alpha  gamma");
        c.move_end();
        c.delete_word_back();
        assert_eq!(c.value(), "alpha  ");
    }

    #[test]
    fn history_round_trip() {
        let mut c = Composer::new();
        c.insert_str("first");
        assert_eq!(c.submit().as_deref(), Some("first"));
        c.insert_str("second");
        assert_eq!(c.submit().as_deref(), Some("second"));
        c.recall_prev();
        assert_eq!(c.value(), "second");
        c.recall_prev();
        assert_eq!(c.value(), "first");
        c.recall_next();
        assert_eq!(c.value(), "second");
        c.recall_next();
        assert_eq!(c.value(), "");
    }

    #[test]
    fn long_token_is_hard_split() {
        let mut c = Composer::new();
        c.insert_str(&"a".repeat(25));
        let layout = c.layout(10, Style::default());
        assert_eq!(layout.rows(), 3);
        assert_eq!(layout.cursor, (2, 3));
    }

    #[test]
    fn multi_line_layout_has_no_phantom_rows() {
        let mut c = Composer::new();
        c.insert_str("alpha\nbravo\ncharlie");
        let layout = c.layout(20, Style::default());
        let flat: Vec<String> = layout.lines.iter().map(|l| l.to_string()).collect();
        assert_eq!(flat, ["alpha", "bravo", "charlie"]);
        assert_eq!(layout.rows(), 3);
    }

    #[test]
    fn trailing_newline_gets_its_own_row() {
        let mut c = Composer::new();
        c.insert_str("alpha\n");
        let layout = c.layout(20, Style::default());
        assert_eq!(layout.rows(), 2);
        // The caret rests on the empty row after the newline.
        assert_eq!(layout.cursor, (1, 0));
    }
}
