use crate::ui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

/// Read-only view over a unified diff produced by a tool call.
///
/// The view owns nothing but the diff text: it is a pure projection, so it is
/// safe to hand it model- or tool-supplied strings. Nothing here ever executes
/// or evaluates the diff contents.
pub struct DiffView {
    entries: Vec<String>,
    theme: Theme,
}

impl DiffView {
    pub fn new() -> Self {
        Self::with_theme(Theme::detect())
    }

    pub fn with_theme(theme: Theme) -> Self {
        Self {
            entries: Vec::new(),
            theme,
        }
    }

    /// Append one diff line. The string is kept verbatim: a leading `+` marks an
    /// addition, `-` a removal, `@@` a hunk header and `---`/`+++` a file header.
    pub fn push_diff(&mut self, diff: impl Into<String>) {
        let diff = diff.into();
        if !diff.is_empty() {
            self.entries.push(diff);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Style for one diff line, following unified-diff precedence: file headers
    /// (`---`/`+++`) must be matched before the bare `-`/`+` content markers.
    /// A leading `…` is the bounded-preview elision marker ("… +N more lines")
    /// rather than diff data, so it renders faint like the chatter it is.
    pub fn style_for(&self, entry: &str) -> Style {
        if entry.starts_with("+++") || entry.starts_with("---") {
            Style::default()
                .fg(self.theme.accent_soft())
                .add_modifier(Modifier::BOLD)
        } else if entry.starts_with("@@") {
            Style::default().fg(self.theme.accent())
        } else if entry.starts_with('+') {
            Style::default().fg(self.theme.diff_add())
        } else if entry.starts_with('-') {
            Style::default().fg(self.theme.diff_delete())
        } else if entry.starts_with('…') {
            Style::default().fg(self.theme.faint())
        } else {
            self.theme.base()
        }
    }

    /// Styled lines for embedding the diff inside another cached text (e.g. a
    /// transcript entry) instead of painting a standalone widget.
    pub fn styled_lines(&self) -> Vec<Line<'_>> {
        self.entries
            .iter()
            .map(|entry| Line::from(Span::styled(entry.as_str(), self.style_for(entry))))
            .collect()
    }

    /// Draws the diff. Context and hunk lines are rendered in the base style,
    /// additions green, removals red, file and hunk headers accented.
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let lines = self.styled_lines();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border()))
            .style(Style::default().bg(self.theme.panel_bg()));
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }

    // -- side-by-side (split) rendering ------------------------------------

    /// Parses the unified-diff entries into paired rows. Line numbers are
    /// tracked from `@@ -a,b +c,d @@` hunk headers exactly like `git diff`
    /// does; context advances both counters, `-` only old, `+` only new.
    fn split_rows(&self) -> Vec<SplitRow> {
        use SplitRow as Row;
        let mut rows = Vec::new();
        let mut old_line = 0usize;
        let mut new_line = 0usize;
        for entry in &self.entries {
            if entry.starts_with("+++") || entry.starts_with("---") {
                // File headers become the panel titles, not body rows.
                continue;
            }
            if entry.starts_with("@@") {
                if let Some((old_start, new_start)) = parse_hunk_heads(entry) {
                    old_line = old_start;
                    new_line = new_start;
                }
                rows.push(Row {
                    left_num: None,
                    left_text: String::new(),
                    left_del: false,
                    right_num: None,
                    right_text: String::new(),
                    right_add: false,
                    full: Some(entry.clone()),
                });
                continue;
            }
            if entry.starts_with('…') {
                rows.push(Row {
                    left_num: None,
                    left_text: String::new(),
                    left_del: false,
                    right_num: None,
                    right_text: String::new(),
                    right_add: false,
                    full: Some(entry.clone()),
                });
                continue;
            }
            if let Some(rest) = entry.strip_prefix('-') {
                rows.push(Row {
                    left_num: Some(old_line),
                    left_text: format!("-{rest}"),
                    left_del: true,
                    right_num: None,
                    right_text: String::new(),
                    right_add: false,
                    full: None,
                });
                old_line += 1;
            } else if let Some(rest) = entry.strip_prefix('+') {
                rows.push(Row {
                    left_num: None,
                    left_text: String::new(),
                    left_del: false,
                    right_num: Some(new_line),
                    right_text: format!("+{rest}"),
                    right_add: true,
                    full: None,
                });
                new_line += 1;
            } else {
                let rest = entry.strip_prefix(' ').unwrap_or(entry);
                rows.push(Row {
                    left_num: Some(old_line),
                    left_text: rest.to_string(),
                    left_del: false,
                    right_num: Some(new_line),
                    right_text: rest.to_string(),
                    right_add: false,
                    full: None,
                });
                old_line += 1;
                new_line += 1;
            }
        }
        rows
    }

    /// Side-by-side lines: old on the left, new on the right, line numbers on
    /// both, one output row per diff row. Rows never wrap — long lines are
    /// truncated to their column — so the two panels stay aligned. `width` is
    /// the total available width; below `SPLIT_MIN_WIDTH` callers should fall
    /// back to [`Self::styled_lines`].
    pub fn styled_split_lines(&self, width: usize) -> Vec<Line<'static>> {
        const GAP: usize = 3; // " │ " separator between the panels
        if width < SPLIT_MIN_WIDTH {
            return self.styled_lines().into_iter().map(owned_line).collect();
        }
        let col_w = (width - GAP) / 2;
        let gap_style = Style::default().fg(self.theme.border());
        let num_style = Style::default().fg(self.theme.faint());
        let mut out = Vec::new();
        for row in self.split_rows() {
            if let Some(text) = row.full {
                let style = if text.starts_with("@@") {
                    Style::default().fg(self.theme.accent())
                } else {
                    Style::default().fg(self.theme.faint())
                };
                let shown = crate::ui::text::truncate(&text, width);
                out.push(Line::from(Span::styled(shown.to_string(), style)));
                continue;
            }
            let left_style = if row.left_del {
                Style::default().fg(self.theme.diff_delete())
            } else {
                self.theme.base()
            };
            let right_style = if row.right_add {
                Style::default().fg(self.theme.diff_add())
            } else {
                self.theme.base()
            };
            let left_cell = cell(row.left_num, &row.left_text, col_w);
            let right_cell = cell(row.right_num, &row.right_text, col_w);
            let left_num_span = Span::styled(left_cell.0, num_style);
            let left_text_span = Span::styled(left_cell.1, left_style);
            let right_num_span = Span::styled(right_cell.0, num_style);
            let right_text_span = Span::styled(right_cell.1, right_style);
            out.push(Line::from(vec![
                left_num_span,
                left_text_span,
                Span::styled(" │ ".to_string(), gap_style),
                right_num_span,
                right_text_span,
            ]));
        }
        out
    }

    /// Draws the diff side-by-side (old left / new right). Falls back to the
    /// unified view when the area is too narrow for two readable columns.
    pub fn render_split(&self, frame: &mut Frame, area: Rect) {
        let inner_w = area.width.saturating_sub(2) as usize; // borders
        let lines = self.styled_split_lines(inner_w);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border()))
            .style(Style::default().bg(self.theme.panel_bg()));
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }
}

/// Minimum total width at which the split view is still readable; below it
/// `styled_split_lines` degrades to the unified rendering.
const SPLIT_MIN_WIDTH: usize = 40;

/// One paired row of a side-by-side diff: old text on the left, new text
/// on the right, each with its source line number (`None` = blank side).
/// `full` carries a full-width row (hunk header or elision marker).
struct SplitRow {
    left_num: Option<usize>,
    left_text: String,
    left_del: bool,
    right_num: Option<usize>,
    right_text: String,
    right_add: bool,
    full: Option<String>,
}

/// `@@ -12,3 +12,4 @@` → `(12, 12)`. Missing counts are fine; garbage is not
/// a hunk header and returns `None`.
fn parse_hunk_heads(header: &str) -> Option<(usize, usize)> {
    let mut old = None;
    let mut new = None;
    for part in header.split_whitespace() {
        if let Some(rest) = part.strip_prefix('-') {
            old = rest.split(',').next()?.parse().ok();
        } else if let Some(rest) = part.strip_prefix('+') {
            new = rest.split(',').next()?.parse().ok();
        }
    }
    match (old, new) {
        (Some(o), Some(n)) => Some((o, n)),
        _ => None,
    }
}

/// One side of a split row: right-aligned line number (blank when `None`)
/// plus the text truncated to the remaining column width. Returns the number
/// cell and the text cell separately so they can carry different styles.
fn cell(num: Option<usize>, text: &str, col_w: usize) -> (String, String) {
    let num_w = 4usize; // fits 4-digit line numbers, matches the screenshot
    let text_w = col_w.saturating_sub(num_w + 1);
    let num_cell = match num {
        Some(n) => format!("{:>num_w$} ", n),
        None => format!("{:>num_w$} ", ""),
    };
    (
        num_cell,
        crate::ui::text::truncate(text, text_w.max(1)).to_string(),
    )
}

/// Converts a borrowed line into an owned one (spans copied).
fn owned_line(line: Line<'_>) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|s| Span::styled(s.content.to_string(), s.style))
            .collect::<Vec<_>>(),
    )
}

impl Default for DiffView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn view() -> DiffView {
        let mut v = DiffView::with_theme(Theme::with_id(crate::ui::theme::ThemeId::Umbrella));
        v.push_diff("--- a/src/lib.rs");
        v.push_diff("+++ b/src/lib.rs");
        v.push_diff("@@ -1,3 +1,3 @@");
        v.push_diff(" fn main() {}");
        v.push_diff("-let x = 1;");
        v.push_diff("+let x = 2;");
        v
    }

    fn render_to_string(view: &DiffView, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view.render(f, f.area())).unwrap();
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

    #[test]
    fn file_headers_beat_content_markers() {
        let v = view();
        // `---` is a file header, not a removal, so it must not render as a deletion.
        assert_ne!(
            v.style_for("--- a/src/lib.rs"),
            Style::default().fg(v.theme.diff_delete())
        );
        assert_eq!(
            v.style_for("-let x = 1;"),
            Style::default().fg(v.theme.danger())
        );
    }

    #[test]
    fn hunk_and_content_lines_are_distinguished() {
        let v = view();
        assert_eq!(
            v.style_for("@@ -1,3 +1,3 @@"),
            Style::default().fg(v.theme.accent())
        );
        assert_eq!(
            v.style_for("+let x = 2;"),
            Style::default().fg(v.theme.diff_add())
        );
        assert_eq!(v.style_for(" fn main() {}"), v.theme.base());
    }

    #[test]
    fn elision_marker_renders_faint_not_as_content() {
        let v = view();
        // `… +N more lines` is preview chatter, not diff data.
        assert_eq!(
            v.style_for("… +3 more lines"),
            Style::default().fg(v.theme.faint())
        );
    }

    #[test]
    fn styled_lines_match_the_render_styles() {
        let v = view();
        let lines = v.styled_lines();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0].spans[0].style, v.style_for("--- a/src/lib.rs"));
        assert_eq!(lines[4].spans[0].style, v.style_for("-let x = 1;"));
        assert_eq!(lines[4].spans[0].content, "-let x = 1;");
        assert_eq!(lines[5].spans[0].style, v.style_for("+let x = 2;"));
    }

    #[test]
    fn empty_entries_are_dropped() {
        let mut v = DiffView::with_theme(Theme::detect());
        v.push_diff("");
        assert!(v.is_empty());
        assert_eq!(v.len(), 0);
        v.push_diff("+a");
        assert_eq!(v.len(), 1);
        v.clear();
        assert!(v.is_empty());
    }

    #[test]
    fn renders_all_diff_lines() {
        let text = render_to_string(&view(), 40, 10);
        assert!(text.contains("--- a/src/lib.rs"), "{text}");
        assert!(text.contains("+let x = 2;"), "{text}");
    }

    #[test]
    fn renders_empty_view_without_panicking() {
        let v = DiffView::with_theme(Theme::detect());
        let text = render_to_string(&v, 20, 5);
        // An empty diff still paints its border and nothing else.
        assert_eq!(text.lines().filter(|l| !l.is_empty()).count(), 5, "{text}");
        assert!(!text.contains("let x"), "{text}");
    }

    #[test]
    fn tiny_area_does_not_panic() {
        let _ = render_to_string(&view(), 2, 1);
    }

    // -- side-by-side (split) ------------------------------------------------

    #[test]
    fn split_rows_pair_old_and_new_with_line_numbers() {
        let v = view();
        let lines = v.styled_split_lines(80);
        // hunk header + context + removal + addition = 4 rows.
        assert_eq!(lines.len(), 4, "{lines:?}");
        // Line 0: hunk header, full width, no line numbers.
        let hunk: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(hunk.starts_with("@@ -1,3 +1,3 @@"), "{hunk}");
        // Line 1: context `fn main() {}` on BOTH sides, numbered 1 and 1.
        let ctx: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(ctx.contains("   1 fn main() {}"), "{ctx}");
        assert_eq!(ctx.matches("fn main() {}").count(), 2, "{ctx}");
        // Line 2: removal only on the left, blank right, old line 2.
        let del: String = lines[2].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(del.contains("-let x = 1;"), "{del}");
        // Line 3: addition only on the right, blank left, new line 2.
        let add: String = lines[3].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(add.contains("+let x = 2;"), "{add}");
    }

    #[test]
    fn split_lines_carry_column_styles() {
        let v = view();
        let lines = v.styled_split_lines(80);
        // Removal row: first (number) span faint, second (text) span red.
        assert_eq!(
            lines[2].spans[0].style,
            Style::default().fg(v.theme.faint())
        );
        assert_eq!(
            lines[2].spans[1].style,
            Style::default().fg(v.theme.diff_delete())
        );
        // Addition row: right text span green.
        assert_eq!(
            lines[3].spans[4].style,
            Style::default().fg(v.theme.diff_add())
        );
    }

    #[test]
    fn split_falls_back_to_unified_when_narrow() {
        let v = view();
        let narrow = v.styled_split_lines(30);
        let unified: Vec<String> = v
            .styled_lines()
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        let narrow_text: Vec<String> = narrow
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert_eq!(narrow_text, unified);
    }

    #[test]
    fn split_render_shows_both_panels_side_by_side() {
        let v = view();
        let text = render_to_string_split(&v, 80, 10);
        // Both the removal (left panel) and the addition (right panel) appear
        // on separate output lines in the same frame.
        assert!(text.contains("-let x = 1;"), "{text}");
        assert!(text.contains("+let x = 2;"), "{text}");
        // The gap column separator is painted between the panels.
        assert!(text.contains("│"), "{text}");
    }

    #[test]
    fn split_render_narrow_area_does_not_panic() {
        let v = view();
        let _ = render_to_string_split(&v, 10, 4);
        let _ = render_to_string_split(&v, 2, 1);
    }

    fn render_to_string_split(view: &DiffView, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view.render_split(f, f.area())).unwrap();
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
}
