use super::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncates to `max` display columns, appending an ellipsis when it has to cut.
pub fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max.saturating_sub(1) {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

/// Keeps the tail of a path visible (`src/…/runtime.rs`) when trimming the head.
pub fn truncate_head(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1);
    let mut out: Vec<char> = Vec::new();
    let mut w = 0;
    for ch in s.chars().rev() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > keep {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.reverse();
    format!("…{}", out.into_iter().collect::<String>())
}

/// `~/dev/nanomite` - home collapsed, middle trimmed only if still too long.
pub fn display_path(path: &Path, max: usize) -> String {
    let mut text = path.to_string_lossy().replace('\\', "/");
    if let Some(home) = dirs::home_dir() {
        let home = home.to_string_lossy().replace('\\', "/");
        if !home.is_empty() && text.starts_with(&home) {
            text = format!("~{}", &text[home.len()..]);
        }
    }
    truncate_head(&text, max)
}

pub fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        format!("{}m{:02}s", d.as_secs() / 60, d.as_secs() % 60)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MarkdownStyles {
    pub link: Style,
    pub base: Style,
    pub heading: Style,
    pub dim: Style,
    pub code: Style,
    pub fence: Style,
    pub bullet: Style,
    pub quote: Style,
}

impl MarkdownStyles {
    pub fn new(theme: &Theme) -> Self {
        Self {
            link: Style::default()
                .fg(theme.accent())
                .add_modifier(Modifier::UNDERLINED),
            base: theme.base(),
            heading: Style::default()
                .fg(theme.accent())
                .add_modifier(Modifier::BOLD),
            dim: Style::default().fg(theme.muted()),
            code: Style::default().fg(theme.code()),
            fence: Style::default().fg(theme.faint()),
            bullet: Style::default().fg(theme.accent_soft()),
            quote: Style::default()
                .fg(theme.quote())
                .add_modifier(Modifier::ITALIC),
        }
    }
}

/// Renders a pragmatic subset of markdown into wrapped, styled lines.
///
/// Tables render one aligned line per row; over-wide columns are truncated to
/// fit the viewport instead of being re-wrapped into prose.
pub fn markdown(src: &str, max_width: usize, st: &MarkdownStyles) -> Vec<Line<'static>> {
    let max_width = max_width.max(8);
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut in_fence = false;
    let mut in_table = false;
    let mut has_header = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_col_widths: Vec<usize> = Vec::new(); // Max content width per column
    let mut paragraph: Vec<Span<'static>> = Vec::new();

    // Headings hug the text that follows them.
    let mut after_heading = false;
    let flush = |paragraph: &mut Vec<Span<'static>>, out: &mut Vec<Line<'static>>| {
        if !paragraph.is_empty() {
            out.extend(wrap_spans(std::mem::take(paragraph), max_width));
        }
    };

    for raw in src.replace('\r', "").split('\n') {
        let trimmed = raw.trim_end();
        let is_table_row = trimmed.trim_start().starts_with('|');

        // Any non-table line closes a table in progress.
        if in_table && !is_table_row {
            flush_table(
                &table_rows,
                &table_col_widths,
                has_header,
                max_width,
                st,
                &mut out,
            );
            in_table = false;
            has_header = false;
            table_rows.clear();
            table_col_widths.clear();
        }

        // Fenced code blocks.
        if let Some(lang) = fence_marker(trimmed) {
            if in_fence {
                in_fence = false;
                out.push(Line::from(Span::styled("▏", st.fence)));
            } else {
                flush(&mut paragraph, &mut out);
                in_fence = true;
                let label = if lang.is_empty() {
                    String::new()
                } else {
                    format!(" {lang}")
                };
                out.push(Line::from(Span::styled(format!("▏{label}"), st.fence)));
            }
            continue;
        }
        if in_fence {
            out.push(Line::from(vec![
                Span::styled("▏ ", st.fence),
                Span::styled(trimmed.to_string(), st.code),
            ]));
            continue;
        }

        let line = trimmed.trim_start();

        // Table rows: collect cells, track column widths, skip the divider row.
        if is_table_row {
            if !in_table {
                flush(&mut paragraph, &mut out);
                in_table = true;
                has_header = false;
            }
            if is_table_separator(line) {
                // A divider right after the first row marks it as the header.
                if table_rows.len() == 1 {
                    has_header = true;
                }
            } else {
                let cells = parse_table_row(line);
                for (i, cell) in cells.iter().enumerate() {
                    if table_col_widths.len() <= i {
                        table_col_widths.push(0);
                    }
                    table_col_widths[i] = table_col_widths[i].max(width(cell));
                }
                table_rows.push(cells);
            }
            continue;
        }

        if line.is_empty() {
            if std::mem::take(&mut after_heading) {
                continue;
            }
            flush(&mut paragraph, &mut out);
            if !out.is_empty() {
                out.push(Line::default());
            }
            continue;
        }

        after_heading = false;

        // Horizontal rule.
        if matches!(line, "---" | "***" | "___" | "- - -") {
            flush(&mut paragraph, &mut out);
            out.push(Line::from(Span::styled(
                super::theme::glyph::RULE.repeat(max_width.min(60)),
                st.fence,
            )));
            continue;
        }

        // Headings.
        if let Some(level) = heading_level(line) {
            flush(&mut paragraph, &mut out);
            let mut spans = vec![Span::styled(
                line[level..].trim().to_string(),
                if level <= 2 {
                    st.heading
                } else {
                    st.base.add_modifier(Modifier::BOLD)
                },
            )];
            if level == 1 {
                spans.insert(0, Span::styled("", st.heading));
            }
            out.push(Line::from(spans));
            after_heading = level <= 2;
            continue;
        }

        // Blockquote.
        if let Some(rest) = line.strip_prefix("> ") {
            flush(&mut paragraph, &mut out);
            let spans = inline(rest, st);
            let mut lines = wrap_spans(spans, max_width.saturating_sub(2));
            for line in &mut lines {
                line.spans.insert(0, Span::styled("▏ ", st.quote));
            }
            out.extend(lines);
            continue;
        }

        // Bulleted / ordered lists.
        if let Some((marker, indent, rest)) = list_item(line) {
            flush(&mut paragraph, &mut out);
            let pad = " ".repeat(indent);
            let spans = inline(rest, st);
            let mut lines = wrap_spans(
                spans,
                max_width.saturating_sub(pad.len() + marker.len() + 1),
            );
            if let Some(first) = lines.first_mut() {
                first.spans.splice(
                    0..0,
                    [
                        Span::raw(pad),
                        Span::styled(format!("{marker} "), st.bullet),
                    ],
                );
            }
            let cont = " ".repeat(indent + marker.len() + 1);
            for line in lines.iter_mut().skip(1) {
                line.spans.insert(0, Span::raw(cont.clone()));
            }
            out.extend(lines);
            continue;
        }

        paragraph.extend(inline(line, st));
    }

    if in_table {
        flush_table(
            &table_rows,
            &table_col_widths,
            has_header,
            max_width,
            st,
            &mut out,
        );
    }
    flush(&mut paragraph, &mut out);
    while out.last().is_some_and(|l| l.spans.is_empty()) {
        out.pop();
    }
    out
}

/// True for the `| --- | :---: |` alignment divider row.
fn is_table_separator(line: &str) -> bool {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    !inner.is_empty()
        && inner.split('|').all(|cell| {
            let c = cell.trim();
            let c = c.strip_prefix(':').unwrap_or(c);
            let c = c.strip_suffix(':').unwrap_or(c);
            !c.is_empty() && c.chars().all(|ch| ch == '-')
        })
}

/// Splits `| a | b |` into trimmed cell contents, unescaping `\|`.
fn parse_table_row(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner
        .split('|')
        .map(|cell| cell.trim().replace("\\|", "|"))
        .collect()
}

/// Renders accumulated rows as aligned columns: header, rule, then body rows.
fn flush_table(
    rows: &[Vec<String>],
    natural: &[usize],
    has_header: bool,
    max_width: usize,
    st: &MarkdownStyles,
    out: &mut Vec<Line<'static>>,
) {
    if rows.is_empty() {
        return;
    }
    let ncols = natural.len();
    if ncols == 0 {
        return;
    }
    // " │ " separates columns, 3 columns each.
    let widths = fit_table_widths(natural, max_width.saturating_sub(3 * (ncols - 1)));
    let head = st.base.add_modifier(Modifier::BOLD);
    for (r, row) in rows.iter().enumerate() {
        if has_header && r == 0 {
            out.push(table_line(row, &widths, head, st.dim));
            let mut rule = String::new();
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    rule.push_str("─┼─");
                }
                rule.push_str(&"─".repeat(*w));
            }
            out.push(Line::from(Span::styled(rule, st.dim)));
        } else {
            out.push(table_line(row, &widths, st.base, st.dim));
        }
    }
}

/// One aligned table row as a styled line.
fn table_line(row: &[String], widths: &[usize], style: Style, sep: Style) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let last = widths.len().saturating_sub(1);
    for (i, col) in widths.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", sep));
        }
        let cell = row.get(i).map(String::as_str).unwrap_or("");
        let text = if width(cell) > *col {
            pad_end(&truncate(cell, *col), *col)
        } else if i == last {
            cell.to_string()
        } else {
            pad_end(cell, *col)
        };
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

/// Shrinks natural column widths to fit `available`, shaving the widest
/// column first so narrow columns keep their content. Never shrinks below 3.
fn fit_table_widths(natural: &[usize], available: usize) -> Vec<usize> {
    let mut widths = natural.to_vec();
    if widths.is_empty() || widths.iter().sum::<usize>() <= available {
        return widths;
    }
    while widths.iter().sum::<usize>() > available {
        let Some((idx, _)) = widths
            .iter()
            .enumerate()
            .filter(|(_, w)| **w > 3)
            .max_by_key(|(_, w)| *w)
        else {
            break;
        };
        widths[idx] -= 1;
    }
    // Degenerate viewport: even split rather than letting one column eat it all.
    if widths.iter().sum::<usize>() > available {
        let each = (available / widths.len()).max(1);
        widths.iter_mut().for_each(|w| *w = each);
    }
    widths
}

/// Pads `s` with trailing spaces up to `w` display columns.
fn pad_end(s: &str, w: usize) -> String {
    let mut out = s.to_string();
    for _ in width(s)..w {
        out.push(' ');
    }
    out
}

fn fence_marker(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("```")
}

fn heading_level(line: &str) -> Option<usize> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    if line[hashes..].starts_with(' ') {
        Some(hashes)
    } else {
        None
    }
}

/// Returns `(marker, indent, content)` for list items.
fn list_item(line: &str) -> Option<(String, usize, &str)> {
    let indent = line.len() - line.trim_start().len();
    let rest = line.trim_start();
    if let Some(tail) = rest.strip_prefix("- ").or_else(|| rest.strip_prefix("* ")) {
        return Some(("•".into(), indent, tail));
    }
    if let Some(dot) = rest.find(". ") {
        if dot > 0 && rest[..dot].chars().all(|c| c.is_ascii_digit()) {
            return Some((format!("{}.", &rest[..dot]), indent, &rest[dot + 2..]));
        }
    }
    None
}

/// Inline emphasis, inline code, links and autolinks.
///
/// Single pass over the char buffer: matched spans are emitted whole, so this stays
/// linear even for long paragraphs.
fn inline(text: &str, st: &MarkdownStyles) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut i = 0usize;

    while i < chars.len() {
        match match_inline(&chars, i, st) {
            Some((len, kind)) => {
                if !buf.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut buf), st.base));
                }
                let raw: String = chars[i..i + len].iter().collect();
                match kind {
                    InlineKind::Styled(style) => {
                        spans.push(Span::styled(strip_markers(&raw), style))
                    }
                    InlineKind::Link => {
                        let (label, target) = split_link(&raw);
                        spans.push(Span::styled(label, st.link));
                        spans.push(Span::styled(format!(" ({target})"), st.dim));
                    }
                }
                i += len;
            }
            None => {
                buf.push(chars[i]);
                i += 1;
            }
        }
    }
    if !buf.is_empty() {
        spans.push(Span::styled(buf, st.base));
    }
    spans
}

/// What a matched inline construct renders as.
enum InlineKind {
    Styled(Style),
    Link,
}

/// Returns `(consumed_chars, kind)` when an inline construct starts at `i`.
fn match_inline(chars: &[char], i: usize, st: &MarkdownStyles) -> Option<(usize, InlineKind)> {
    for (marker, style) in [
        ("**", st.base.add_modifier(Modifier::BOLD)),
        ("~~", st.dim.add_modifier(Modifier::CROSSED_OUT)),
        ("`", st.code),
    ] {
        if starts_with(chars, i, marker) {
            let from = i + marker.chars().count();
            let close = find(chars, from, marker)?;
            return Some((
                close + marker.chars().count() - i,
                InlineKind::Styled(style),
            ));
        }
    }

    for marker in ['*', '_'] {
        if chars[i] == marker {
            if let Some(close) = find_char(chars, i + 1, marker) {
                if close > i + 1 {
                    return Some((
                        close + 1 - i,
                        InlineKind::Styled(st.base.add_modifier(Modifier::ITALIC)),
                    ));
                }
            }
        }
    }

    if chars[i] == '[' {
        if let Some(label_end) = find_char(chars, i + 1, ']') {
            if chars.get(label_end + 1) == Some(&'(') {
                if let Some(paren_end) = find_char(chars, label_end + 2, ')') {
                    return Some((paren_end + 1 - i, InlineKind::Link));
                }
            }
        }
    }

    if starts_with(chars, i, "http://") || starts_with(chars, i, "https://") {
        let mut end = i;
        while end < chars.len()
            && !chars[end].is_whitespace()
            && chars[end] != ')'
            && chars[end] != ','
        {
            end += 1;
        }
        if end > i + 8 {
            return Some((end - i, InlineKind::Styled(st.link)));
        }
    }

    None
}

fn starts_with(chars: &[char], i: usize, pattern: &str) -> bool {
    (i..)
        .zip(pattern.chars())
        .all(|(idx, c)| chars.get(idx) == Some(&c))
}

/// Splits `[label](target)` into its two display parts.
fn split_link(raw: &str) -> (String, String) {
    let inner = raw.trim_start_matches('[');
    match inner.find("](") {
        Some(i) => (
            inner[..i].to_string(),
            inner[i + 2..].trim_end_matches(')').to_string(),
        ),
        None => (inner.to_string(), String::new()),
    }
}

fn find(chars: &[char], from: usize, pattern: &str) -> Option<usize> {
    (from..chars.len()).find(|&i| starts_with(chars, i, pattern))
}

fn find_char(chars: &[char], from: usize, needle: char) -> Option<usize> {
    (from..chars.len()).find(|&i| chars[i] == needle)
}

fn strip_markers(s: &str) -> String {
    let t = s.trim();
    for pair in ["**", "~~", "```"] {
        if let Some(stripped) = t.strip_prefix(pair).and_then(|r| r.strip_suffix(pair)) {
            return stripped.to_string();
        }
    }
    if let Some(inner) = t.strip_prefix('`').and_then(|r| r.strip_suffix('`')) {
        return inner.to_string();
    }
    if let Some(inner) = t
        .strip_prefix('*')
        .filter(|_| t.ends_with('*') && t.len() > 1)
    {
        return inner.to_string();
    }
    t.to_string()
}

/// Greedy word wrap that also hard-splits tokens wider than the viewport.
pub fn wrap_spans(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;

    fn push_line(lines: &mut Vec<Line<'static>>, spans: Vec<Span<'static>>) {
        if !spans.is_empty() {
            lines.push(Line::from(spans));
        }
    }

    for span in spans {
        let style = span.style;
        let mut word = String::new();
        for ch in span.content.chars() {
            if ch == ' ' {
                let w = width(&word);
                if w > 0 {
                    if used + w > max_width && used > 0 {
                        push_line(&mut lines, std::mem::take(&mut current));
                        used = 0;
                    }
                    current.push(Span::styled(std::mem::take(&mut word), style));
                    used += w;
                }
                if used < max_width {
                    current.push(Span::styled(" ", style));
                    used += 1;
                } else {
                    push_line(&mut lines, std::mem::take(&mut current));
                    used = 0;
                }
                continue;
            }
            word.push(ch);
            if width(&word) >= max_width {
                let chunk = std::mem::take(&mut word);
                if !current.is_empty() {
                    push_line(&mut lines, std::mem::take(&mut current));
                }
                // Oversized tokens get their own row so nothing is hidden.
                push_line(&mut lines, vec![Span::styled(chunk, style)]);
                used = 0;
            }
        }
        if !word.is_empty() {
            let w = width(&word);
            if used + w > max_width && used > 0 {
                push_line(&mut lines, std::mem::take(&mut current));
                used = 0;
            }
            current.push(Span::styled(word, style));
            used += w;
        }
    }
    push_line(&mut lines, current);
    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

/// Wraps plain text with a fixed prefix on the first visual line and an indent after it.
pub fn wrap_prefixed(
    text: &str,
    prefix: &str,
    style: Style,
    max_width: usize,
    cont: &str,
) -> Vec<Line<'static>> {
    let mut lines = wrap_spans(vec![Span::raw(text.to_string())], max_width);
    if let Some(first) = lines.first_mut() {
        first
            .spans
            .insert(0, Span::styled(prefix.to_string(), style));
    }
    for line in lines.iter_mut().skip(1) {
        line.spans.insert(0, Span::raw(cont.to_string()));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_display_width() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello w…");
        assert_eq!(truncate("ありがとう", 5), "あり…");
        assert_eq!(width(&truncate("ありがとう", 4)), 3);
    }

    #[test]
    fn wraps_on_word_boundaries() {
        let lines = wrap_spans(vec![Span::raw("the quick brown fox jumps")], 10);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].to_string().trim_end(), "the quick");
        assert_eq!(lines[2].to_string().trim_end(), "jumps");
    }

    #[test]
    fn hard_splits_long_tokens() {
        let long = "x".repeat(25);
        let lines = wrap_spans(vec![Span::raw(long)], 10);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn markdown_renders_headings_and_lists() {
        let theme = Theme::detect();
        let st = MarkdownStyles::new(&theme);
        let out = markdown("# Title\n\n- one\n- two\n\n`code`", 40, &st);
        let joined: String = out
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Title"));
        assert!(joined.contains("one"));
    }

    #[test]
    fn formats_tokens_and_durations() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_500), "1.5k");
        assert_eq!(format_duration(Duration::from_millis(250)), "250ms");
        assert_eq!(format_duration(Duration::from_millis(1_250)), "1.2s");
    }

    #[test]
    fn markdown_renders_tables() {
        let theme = Theme::detect();
        let st = MarkdownStyles::new(&theme);
        let out = markdown(
            "| Name | Value |\n|------|-------|\n| Foo | 123 |\n| Bar | 456 |",
            40,
            &st,
        );
        let flat: Vec<String> = out.iter().map(|l| l.to_string()).collect();
        // One line per row: header, rule, two body rows — never a merged blob.
        assert_eq!(flat.len(), 4, "expected 4 table lines, got: {flat:?}");
        assert!(flat[0].contains("Name") && flat[0].contains("Value"));
        assert!(flat[1].starts_with('─'), "rule under header: {flat:?}");
        assert!(flat[2].contains("Foo") && flat[2].contains("123"));
        assert!(flat[3].contains("Bar") && flat[3].contains("456"));
        // Columns line up: both values start at the same offset.
        assert_eq!(
            flat[2].find("123"),
            flat[3].find("456"),
            "values not column-aligned: {flat:?}"
        );
        assert!(
            !flat[0].contains('|'),
            "raw pipes must not survive: {flat:?}"
        );
    }

    #[test]
    fn markdown_table_closes_and_prose_resumes() {
        let theme = Theme::detect();
        let st = MarkdownStyles::new(&theme);
        let out = markdown(
            "| a | b |\n|---|---|\n| 1 | 2 |\n\nAfter the table.",
            40,
            &st,
        );
        let flat: Vec<String> = out.iter().map(|l| l.to_string()).collect();
        let last = flat.last().expect("non-empty");
        assert!(last.contains("After the table."), "got: {flat:?}");
        assert!(!last.contains('│'), "table leaked into prose: {last:?}");
    }

    #[test]
    fn markdown_table_truncates_to_viewport() {
        let theme = Theme::detect();
        let st = MarkdownStyles::new(&theme);
        let wide = "x".repeat(60);
        let out = markdown(&format!("| A | B |\n|---|---|\n| {wide} | y |"), 30, &st);
        assert!(!out.is_empty());
        for line in &out {
            assert!(
                width(&line.to_string()) <= 30,
                "line exceeds viewport: {line:?}"
            );
        }
    }

    #[test]
    fn markdown_table_without_divider_is_all_body_rows() {
        let theme = Theme::detect();
        let st = MarkdownStyles::new(&theme);
        let out = markdown("| a | b |\n| 1 | 2 |", 40, &st);
        assert_eq!(out.len(), 2, "no rule without a divider row");
        assert!(!out[0].to_string().starts_with('─'));
    }
}
