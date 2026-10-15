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
    fn style_for(&self, entry: &str) -> Style {
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
        } else {
            self.theme.base()
        }
    }

    /// Draws the diff. Context and hunk lines are rendered in the base style,
    /// additions green, removals red, file and hunk headers accented.
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let lines: Vec<Line<'_>> = self
            .entries
            .iter()
            .map(|entry| Line::from(Span::styled(entry.as_str(), self.style_for(entry))))
            .collect();

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
}
