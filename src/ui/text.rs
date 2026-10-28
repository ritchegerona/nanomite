use super::theme::{glyph, Theme};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;
use std::time::Duration;

/// Maps model-output emoji to professional monochrome markers and strips the
/// remaining pictographs from the rendered transcript. Display-only: the
/// stored text and copy/export keep the original bytes.
///
/// - ✅ ✔️ ☑️ → `✓` (monochrome, inherits the surrounding style)
/// - ❌ ✖️ ❎ → `✗`
/// - ⚠️ ❗ → `!`
/// - ZWJ families, flags, skin tones and every other pictograph are dropped
pub fn professionalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let mapped: Option<&str> = match ch {
            '\u{2705}' | '\u{2714}' | '\u{2611}' => Some("\u{2713}"),
            '\u{274C}' | '\u{2716}' | '\u{274E}' => Some("\u{2717}"),
            '\u{26A0}' | '\u{2757}' => Some("!"),
            // Selectors, joiners and modifiers: absorbed by the mapping above
            // or dropped with their base glyph.
            '\u{FE0F}' | '\u{200D}' | '\u{20E3}' => Some(""),
            c if ('\u{1F3FB}'..='\u{1F3FF}').contains(&c) => Some(""),
            c if ('\u{1F1E6}'..='\u{1F1FF}').contains(&c) => Some(""),
            c if ('\u{1F000}'..='\u{1FAFF}').contains(&c) => Some(""),
            c if ('\u{2600}'..='\u{26FF}').contains(&c)
                && !matches!(c, '\u{2611}' | '\u{26A0}') =>
            {
                Some("")
            }
            c if ('\u{2700}'..='\u{27BF}').contains(&c)
                && !matches!(c, '\u{2713}' | '\u{2717}' | '\u{2718}') =>
            {
                Some("")
            }
            '\u{2B50}' => Some(""),
            _ => None,
        };
        if let Some(marker) = mapped {
            out.push_str(marker);
        } else {
            out.push(ch);
        }
    }
    out
}

/// Stateful display-width calculator that approximates how terminals render
/// grapheme clusters — the piece plain `unicode-width` misses and the reason
/// mouse selection could drift on emoji-laden lines:
///
/// - `⚠` + VS16 (`⚠️`) renders as **2** cells, not 1
/// - ZWJ families (`👨‍👩‍👧`) collapse into one glyph of 2 cells
/// - regional indicator pairs (`🇵🇭`) render as one flag of 2 cells
/// - skin-tone modifiers (`👍🏽`) add no cells
///
/// Wrapping, truncation and mouse mapping all feed the same state, so the
/// painted columns and the pointer columns can never disagree.
#[derive(Debug, Clone, Copy, Default)]
pub struct WidthState {
    merge_next: bool,
    ri_open: bool,
    last_cluster: usize,
}

/// One character's contribution: how many cells it adds and whether it
/// continues the previous grapheme cluster.
#[derive(Debug, Clone, Copy)]
pub struct WidthStep {
    pub cells: usize,
    pub continues: bool,
}

impl WidthState {
    pub fn feed(&mut self, ch: char) -> WidthStep {
        match ch {
            // ZWJ: the next glyph joins the current cluster.
            '\u{200D}' => {
                self.merge_next = true;
                WidthStep {
                    cells: 0,
                    continues: true,
                }
            }
            // Emoji presentation selector: the cluster renders two cells;
            // top up a narrow base (⚠️ = ⚠ + VS16).
            '\u{FE0F}' => {
                let add = 2 - self.last_cluster.min(2);
                self.last_cluster = 2;
                WidthStep {
                    cells: add,
                    continues: true,
                }
            }
            // Text presentation selector adds nothing.
            '\u{FE0E}' => WidthStep {
                cells: 0,
                continues: true,
            },
            // Skin-tone modifiers render inside the previous glyph.
            c if ('\u{1F3FB}'..='\u{1F3FF}').contains(&c) => WidthStep {
                cells: 0,
                continues: true,
            },
            // Regional indicators pair into one flag glyph; a non-RI char
            // closes an open pair.
            c if ('\u{1F1E6}'..='\u{1F1FF}').contains(&c) => {
                if self.ri_open {
                    self.ri_open = false;
                    WidthStep {
                        cells: 0,
                        continues: true,
                    }
                } else {
                    self.ri_open = true;
                    self.last_cluster = 2;
                    WidthStep {
                        cells: 2,
                        continues: false,
                    }
                }
            }
            c => {
                self.ri_open = false;
                let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                if self.merge_next {
                    self.merge_next = false;
                    WidthStep {
                        cells: 0,
                        continues: true,
                    }
                } else {
                    self.last_cluster = w;
                    WidthStep {
                        cells: w,
                        continues: false,
                    }
                }
            }
        }
    }
}

pub fn width(s: &str) -> usize {
    let mut state = WidthState::default();
    s.chars().map(|ch| state.feed(ch).cells).sum()
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
    let mut state = WidthState::default();
    let mut w = 0;
    for ch in s.chars() {
        let step = state.feed(ch);
        if w + step.cells > max.saturating_sub(1) {
            break;
        }
        out.push(ch);
        w += step.cells;
    }
    out.push('…');
    out
}

/// Highlights all occurrences of `query` in `text` by wrapping matches in a styled span.
/// Case-insensitive, matches whole words or substrings.
pub fn highlight_matches(text: &str, query: &str, highlight_style: Style) -> Vec<Span<'static>> {
    if query.trim().is_empty() {
        return vec![Span::styled(text.to_string(), Style::default())];
    }

    let query = query.to_lowercase();
    let text_lower = text.to_lowercase();
    let mut spans = Vec::new();
    let mut last_end = 0;

    while let Some(pos) = text_lower[last_end..].find(&query) {
        let actual_pos = last_end + pos;
        if actual_pos > last_end {
            spans.push(Span::styled(
                text[last_end..actual_pos].to_string(),
                Style::default(),
            ));
        }
        let end = actual_pos + query.len();
        spans.push(Span::styled(
            text[actual_pos..end].to_string(),
            highlight_style,
        ));
        last_end = end;
    }

    if last_end < text.len() {
        spans.push(Span::styled(text[last_end..].to_string(), Style::default()));
    }

    spans
}

/// Splits already-styled spans on case-insensitive `query` matches, overlaying
/// `match_style` on the matched slices while every other character keeps its
/// original style. `highlight_matches` restyles whole strings; the transcript
/// search needs markdown, badge and selection colours to survive, so it works
/// span-by-span instead. Matching runs over the flattened line, so a hit that
/// straddles a style boundary (bold meeting plain text, say) is still found.
pub fn highlight_spans(
    spans: Vec<Span<'static>>,
    query: &str,
    match_style: Style,
) -> Vec<Span<'static>> {
    let needle: Vec<char> = query.trim().to_lowercase().chars().collect();
    if needle.is_empty() {
        return spans;
    }
    // Flatten the line's characters once; `owner[i]` maps flattened char i
    // back to the span that styled it.
    let mut flat: Vec<char> = Vec::new();
    let mut owner: Vec<usize> = Vec::new();
    for (si, span) in spans.iter().enumerate() {
        for ch in span.content.chars() {
            flat.push(ch);
            owner.push(si);
        }
    }
    let lower: Vec<char> = flat.iter().flat_map(|c| c.to_lowercase()).collect();
    if lower.len() != flat.len() {
        // Case folding changed the character count (e.g. `İ` → two chars);
        // offsets would desync, so pass the line through untouched.
        return spans;
    }
    // Mark every flattened position covered by a match.
    let mut matched = vec![false; flat.len()];
    let mut any = false;
    if needle.len() <= lower.len() {
        for start in 0..=(lower.len() - needle.len()) {
            if lower[start..start + needle.len()].iter().eq(needle.iter()) {
                for m in &mut matched[start..start + needle.len()] {
                    *m = true;
                }
                any = true;
            }
        }
    }
    if !any {
        return spans;
    }
    // Rebuild as one output span per (source span, match-state) run.
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut run: Option<(usize, bool, String)> = None;
    let flush = |run: &mut Option<(usize, bool, String)>, out: &mut Vec<Span<'static>>| {
        if let Some((si, m, text)) = run.take() {
            if !text.is_empty() {
                let style = if m {
                    spans[si].style.patch(match_style)
                } else {
                    spans[si].style
                };
                out.push(Span::styled(text, style));
            }
        }
    };
    for (i, ch) in flat.iter().copied().enumerate() {
        let (si, m) = (owner[i], matched[i]);
        match &mut run {
            Some((rsi, rm, text)) if *rsi == si && *rm == m => text.push(ch),
            _ => {
                flush(&mut run, &mut out);
                run = Some((si, m, ch.to_string()));
            }
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Keeps the tail of a path visible (`src/…/runtime.rs`) when trimming the head.
pub fn truncate_head(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    // Split into grapheme clusters first so reversed trimming never cuts a
    // ZWJ family or an emoji + selector pair in half.
    let mut clusters: Vec<(String, usize)> = Vec::new();
    let mut state = WidthState::default();
    for ch in s.chars() {
        let step = state.feed(ch);
        if step.continues && !clusters.is_empty() {
            let last = clusters.last_mut().expect("checked above");
            last.0.push(ch);
            last.1 += step.cells;
        } else {
            clusters.push((ch.to_string(), step.cells));
        }
    }
    let keep = max.saturating_sub(1);
    let mut out = String::new();
    let mut w = 0;
    for (text, cw) in clusters.into_iter().rev() {
        if w + cw > keep {
            break;
        }
        out.insert_str(0, &text);
        w += cw;
    }
    format!("…{out}")
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

/// `3 steps` / `1 step` — count with a pluralised noun, no format-string drift.
pub fn plural(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
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
    /// Syntax-highlight styles for fenced code blocks, theme-derived.
    pub code_keyword: Style,
    pub code_string: Style,
    pub code_comment: Style,
    pub code_number: Style,
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
            code_keyword: Style::default().fg(theme.accent()),
            code_string: Style::default().fg(theme.success()),
            code_comment: Style::default().fg(theme.faint()),
            code_number: Style::default().fg(theme.warn()),
        }
    }
}

/// Renders a pragmatic subset of markdown into wrapped, styled lines.
pub fn markdown(src: &str, max_width: usize, st: &MarkdownStyles) -> Vec<Line<'static>> {
    markdown_with_blocks(src, max_width, st).0
}

/// Markdown rendering that also reports finished fenced code blocks as
/// `(first row, last row, raw source)` triples — rows index the returned
/// lines — so the transcript can offer click-to-copy on a whole block.
pub fn markdown_with_blocks(
    src: &str,
    max_width: usize,
    st: &MarkdownStyles,
) -> (Vec<Line<'static>>, Vec<(usize, usize, String)>) {
    let max_width = max_width.max(8);
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut blocks: Vec<(usize, usize, String)> = Vec::new();
    let mut in_fence = false;
    let mut fence_start = 0usize;
    let mut fence_lang = String::new();
    let mut fence_lines: Vec<String> = Vec::new();
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

        // Fenced code blocks: buffered so the whole block can render as one
        // rounded box once the closing fence arrives. Mid-stream (unclosed)
        // blocks fall back to the plain `▏` gutter so live code stays visible.
        if let Some(lang) = fence_marker(trimmed) {
            if in_fence {
                in_fence = false;
                let start = fence_start;
                let source = std::mem::take(&mut fence_lines);
                let lang = std::mem::take(&mut fence_lang);
                fence_start = 0;
                // Box interior: the code column, capped to the viewport.
                let inner = max_width.saturating_sub(6).max(8);
                let code_w = source
                    .iter()
                    .map(|l| width(l))
                    .max()
                    .unwrap_or(0)
                    .min(inner);
                let label = if lang.is_empty() {
                    String::new()
                } else {
                    format!(" {}", lang)
                };
                out.push(box_rule("╭", "╮", code_w, &label, st));
                let lang = code_lang(&lang);
                let mut in_block = false;
                for content in &source {
                    let shown = if width(content) > code_w {
                        format!("{}…", truncate(content, code_w.saturating_sub(1)))
                    } else {
                        content.clone()
                    };
                    let mut spans = vec![Span::styled("│ ", st.fence)];
                    let mut hl = highlight_code(&shown, lang, st, &mut in_block);
                    // Keep the right border aligned: pad the visible width.
                    let w: usize = hl.iter().map(|s| width(s.content.as_ref())).sum();
                    if w < code_w {
                        hl.push(Span::styled(" ".repeat(code_w - w), st.code));
                    }
                    spans.append(&mut hl);
                    spans.push(Span::styled(" │", st.fence));
                    out.push(Line::from(spans));
                }
                let end = out.len(); // the bottom border line
                out.push(box_rule("╰", "╯", code_w, "copy", st));
                blocks.push((start, end, source.join("\n")));
            } else {
                flush(&mut paragraph, &mut out);
                in_fence = true;
                fence_start = out.len();
                fence_lang = lang.to_string();
                fence_lines.clear();
            }
            continue;
        }
        if in_fence {
            fence_lines.push(trimmed.to_string());
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
            // Top-level headings carry a rule, glamour-style.
            if level == 1 {
                let title_w = width(line[level..].trim());
                out.push(Line::from(Span::styled(
                    glyph::RULE.repeat(title_w.clamp(3, max_width)),
                    st.fence,
                )));
            }
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

    if in_fence {
        // Mid-stream block: render unboxed so live code stays visible; the
        // box appears once the closing fence lands.
        let label = if fence_lang.is_empty() {
            String::new()
        } else {
            format!(" {}", fence_lang)
        };
        out.push(Line::from(Span::styled(format!("▏{label}"), st.fence)));
        for line in &fence_lines {
            out.push(Line::from(vec![
                Span::styled("▏ ", st.fence),
                Span::styled(line.clone(), st.code),
            ]));
        }
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
    (out, blocks)
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

/// Renders accumulated rows as a box-drawing grid, opencode-style
/// (`tableOptions.style = "grid"`): `┌─┬─┐` top, header row, `├─┼─┤` rule,
/// body rows, `└─┴─┘` bottom. Cells get one space of padding per side; past
/// the viewport the widest columns are shaved and over-wide cells truncate.
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
    // ` content ` per cell (padding 2) plus `ncols + 1` vertical bars.
    let chrome = 3 * ncols + 1;
    let Some(budget) = max_width.checked_sub(chrome) else {
        return;
    };
    let widths = fit_table_widths(natural, budget);
    let bar = st.fence;
    let head = st.base.add_modifier(Modifier::BOLD);

    let rule = |left: &str, mid: &str, right: &str| -> Line<'static> {
        let mut text = String::from(left);
        for (i, w) in widths.iter().enumerate() {
            if i > 0 {
                text.push_str(mid);
            }
            text.push_str(&"─".repeat(w + 2));
        }
        text.push_str(right);
        Line::from(Span::styled(text, bar))
    };

    out.push(rule("┌", "┬", "┐"));
    for (r, row) in rows.iter().enumerate() {
        if has_header && r == 0 {
            out.push(grid_row(row, &widths, head, bar));
            out.push(rule("├", "┼", "┤"));
        } else {
            out.push(grid_row(row, &widths, st.base, bar));
        }
    }
    out.push(rule("└", "┴", "┘"));
}

/// One grid row: `│ cell │ cell │` with padded, truncated cells.
fn grid_row(row: &[String], widths: &[usize], style: Style, bar: Style) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, col) in widths.iter().enumerate() {
        spans.push(Span::styled("│", bar));
        let cell = row.get(i).map(String::as_str).unwrap_or("");
        let text = if width(cell) > *col {
            truncate(cell, *col)
        } else {
            pad_end(cell, *col)
        };
        spans.push(Span::styled(format!(" {text} "), style));
    }
    spans.push(Span::styled("│", bar));
    Line::from(spans)
}

/// A code-block border rule, wide as the box interior, with an optional
/// label or hint tucked into the line when there is room: `╭─ rust ────╮`.
fn box_rule(
    left: &str,
    right: &str,
    code_w: usize,
    label: &str,
    st: &MarkdownStyles,
) -> Line<'static> {
    let interior = code_w + 2;
    let label_w = width(label);
    let mut text = String::from(left);
    if label_w > 0 && label_w + 3 <= interior {
        text.push_str(&format!("─ {label} "));
        text.push_str(&"─".repeat(interior - label_w - 3));
    } else {
        text.push_str(&"─".repeat(interior));
    }
    text.push_str(right);
    Line::from(Span::styled(text, st.fence))
}
/// Comment syntax family for a fenced block's language tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodeLang {
    /// `//` line + `/* */` block comments (rust, c, go, js, …).
    Slash,
    /// `#` line comments (python, bash, yaml, …).
    Hash,
    /// No known comment syntax; strings and numbers still highlight.
    Plain,
}

fn code_lang(lang: &str) -> CodeLang {
    match lang {
        "rust" | "rs" | "c" | "h" | "cpp" | "c++" | "cc" | "hpp" | "go" | "java" | "kt"
        | "kotlin" | "swift" | "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "zig" | "cs"
        | "csharp" | "dart" | "scala" | "php" | "solidity" | "json" | "jsonc" => CodeLang::Slash,
        "python" | "py" | "python3" | "ruby" | "rb" | "bash" | "sh" | "shell" | "zsh" | "fish"
        | "yaml" | "yml" | "toml" | "perl" | "r" | "makefile" | "make" | "dockerfile"
        | "docker" | "ini" | "conf" | "gitignore" | "env" => CodeLang::Hash,
        _ => CodeLang::Plain,
    }
}

/// A merged keyword set across the common languages. Heuristic by design:
/// the odd false positive in a transcript's snippet beats shipping a heavy
/// syntax-tree dependency (syntect/tree-sitter + onig) in a TUI.
fn code_keyword(word: &str) -> bool {
    matches!(
        word,
        "as" | "async"
            | "await"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "crate"
            | "def"
            | "default"
            | "defer"
            | "do"
            | "dyn"
            | "elif"
            | "else"
            | "enum"
            | "Err"
            | "except"
            | "export"
            | "extends"
            | "extern"
            | "false"
            | "final"
            | "finally"
            | "fn"
            | "for"
            | "from"
            | "func"
            | "function"
            | "go"
            | "if"
            | "impl"
            | "import"
            | "in"
            | "instanceof"
            | "interface"
            | "is"
            | "lambda"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "new"
            | "nil"
            | "None"
            | "not"
            | "null"
            | "Ok"
            | "or"
            | "package"
            | "pass"
            | "print"
            | "private"
            | "protected"
            | "pub"
            | "public"
            | "raise"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "Some"
            | "static"
            | "struct"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "trait"
            | "true"
            | "True"
            | "try"
            | "type"
            | "typeof"
            | "use"
            | "unsafe"
            | "var"
            | "where"
            | "while"
            | "with"
            | "yield"
    )
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// True when `quote` closes within `limit` chars — separates a char literal
/// from a rust lifetime like `'a`, which must never swallow the line.
fn closes_within(chars: &[char], quote: char, limit: usize) -> bool {
    let mut k = 0usize;
    while k < chars.len() && k < limit {
        if chars[k] == '\\' {
            k += 2;
            continue;
        }
        if chars[k] == quote {
            return true;
        }
        k += 1;
    }
    false
}

/// Highlights one source line into styled spans: strings, comments, numbers
/// and keywords. `in_block` carries `/* … */` state across the lines of a
/// block. Runs in O(line length) with no allocation beyond the spans.
fn highlight_code(
    line: &str,
    lang: CodeLang,
    st: &MarkdownStyles,
    in_block: &mut bool,
) -> Vec<Span<'static>> {
    fn push_run(spans: &mut Vec<Span<'static>>, run: &mut String, style: Style) {
        if !run.is_empty() {
            spans.push(Span::styled(std::mem::take(run), style));
        }
    }

    let chars: Vec<char> = line.chars().collect();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut style = st.code;
    let mut i = 0usize;

    while i < chars.len() {
        // Continuing a `/* … */` comment from an earlier line.
        if *in_block {
            style = st.code_comment;
            match chars[i..]
                .windows(2)
                .position(|w| w[0] == '*' && w[1] == '/')
            {
                Some(rel) => {
                    run.push_str(&chars[i..i + rel + 2].iter().collect::<String>());
                    i += rel + 2;
                    *in_block = false;
                    push_run(&mut spans, &mut run, style);
                    style = st.code;
                }
                None => {
                    run.push_str(&chars[i..].iter().collect::<String>());
                    i = chars.len();
                }
            }
            continue;
        }

        let two =
            |pat: [char; 2]| i + 2 <= chars.len() && chars[i] == pat[0] && chars[i + 1] == pat[1];

        // Line comments run to the end of the line.
        if (lang == CodeLang::Slash && two(['/', '/']))
            || (lang == CodeLang::Hash && chars[i] == '#')
        {
            push_run(&mut spans, &mut run, style);
            let comment: String = chars[i..].iter().collect();
            spans.push(Span::styled(comment, st.code_comment));
            return spans;
        }

        // Block comments open a run that may span lines.
        if lang == CodeLang::Slash && two(['/', '*']) {
            push_run(&mut spans, &mut run, style);
            style = st.code_comment;
            run.push_str("/*");
            i += 2;
            *in_block = true;
            continue;
        }

        let ch = chars[i];

        // Strings: double quotes and backticks everywhere; single quotes only
        // when they close within a few characters (char literal, not lifetime).
        if ch == '"' || ch == '`' || (ch == '\'' && closes_within(&chars[i + 1..], ch, 6)) {
            push_run(&mut spans, &mut run, style);
            let mut text = String::new();
            text.push(ch);
            i += 1;
            while i < chars.len() {
                text.push(chars[i]);
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    text.push(chars[i]);
                } else if chars[i] == ch {
                    i += 1;
                    break;
                }
                i += 1;
            }
            spans.push(Span::styled(text, st.code_string));
            continue;
        }

        // Numbers: hex/binary/octal prefixes, or digit runs with `.`/`_` and
        // an optional exponent. `foo.bar` never highlights `.bar`.
        if ch.is_ascii_digit() && (i == 0 || !is_ident_char(chars[i - 1])) {
            push_run(&mut spans, &mut run, style);
            let mut text = String::new();
            if ch == '0'
                && i + 1 < chars.len()
                && matches!(chars[i + 1], 'x' | 'X' | 'b' | 'B' | 'o' | 'O')
            {
                text.push(ch);
                text.push(chars[i + 1]);
                i += 2;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    text.push(chars[i]);
                    i += 1;
                }
            } else {
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_')
                {
                    if chars[i] == '.' && (i + 1 >= chars.len() || !chars[i + 1].is_ascii_digit()) {
                        break;
                    }
                    text.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() && matches!(chars[i], 'e' | 'E') {
                    let mut j = i + 1;
                    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    if j < chars.len() && chars[j].is_ascii_digit() {
                        text.push(chars[i]);
                        i += 1;
                        if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                            text.push(chars[i]);
                            i += 1;
                        }
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            text.push(chars[i]);
                            i += 1;
                        }
                    }
                }
            }
            spans.push(Span::styled(text, st.code_number));
            continue;
        }

        // Identifiers: keywords light up, everything else stays default.
        if ch.is_alphabetic() || ch == '_' {
            let mut j = i;
            while j < chars.len() && is_ident_char(chars[j]) {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            if code_keyword(&word) {
                push_run(&mut spans, &mut run, style);
                spans.push(Span::styled(word, st.code_keyword));
            } else {
                run.push_str(&word);
            }
            i = j;
            continue;
        }

        run.push(ch);
        i += 1;
    }

    if *in_block {
        push_run(&mut spans, &mut run, st.code_comment);
    } else {
        push_run(&mut spans, &mut run, style);
    }
    spans
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
        return Some((glyph::BULLET.to_string(), indent, tail));
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
    fn pluralises_counts() {
        assert_eq!(plural(1, "step"), "1 step");
        assert_eq!(plural(3, "step"), "3 steps");
        assert_eq!(plural(0, "tool"), "0 tools");
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
        // Grid frame: top border, header, rule, two body rows, bottom border.
        assert_eq!(flat.len(), 6, "expected 6 grid lines, got: {flat:?}");
        assert!(
            flat[0].starts_with('┌') && flat[0].ends_with('┐'),
            "{flat:?}"
        );
        assert!(flat[1].contains("Name") && flat[1].contains("Value"));
        assert!(
            flat[2].starts_with('├') && flat[2].contains('┼'),
            "{flat:?}"
        );
        assert!(flat[3].contains("Foo") && flat[3].contains("123"));
        assert!(flat[4].contains("Bar") && flat[4].contains("456"));
        assert!(
            flat[5].starts_with('└') && flat[5].ends_with('┘'),
            "{flat:?}"
        );
        // Columns line up: both values start at the same offset.
        assert_eq!(
            flat[3].find("123"),
            flat[4].find("456"),
            "values not column-aligned: {flat:?}"
        );
        assert!(
            flat[1].contains('│') && flat[3].contains('│') && flat[4].contains('│'),
            "data rows carry vertical bars: {flat:?}"
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
        // Box frame only: no `├` header rule without a divider row.
        assert_eq!(out.len(), 4, "top, two rows, bottom: {:?}", out);
        let flat: Vec<String> = out.iter().map(|l| l.to_string()).collect();
        assert!(
            flat.iter().all(|l| !l.contains('├')),
            "no header rule without a divider row: {flat:?}"
        );
        assert!(!flat[0].to_string().starts_with('─'));
    }

    #[test]
    fn highlight_matches_basic() {
        let style = Style::default().fg(ratatui::style::Color::Red);
        let spans = highlight_matches("hello world hello", "hello", style);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "hello");
        assert_eq!(spans[0].style.fg, Some(ratatui::style::Color::Red));
        assert_eq!(spans[1].content, " world ");
        assert_eq!(spans[2].content, "hello");
        assert_eq!(spans[2].style.fg, Some(ratatui::style::Color::Red));
    }

    #[test]
    fn highlight_matches_case_insensitive() {
        let style = Style::default().fg(ratatui::style::Color::Red);
        let spans = highlight_matches("Hello WORLD hello", "hello", style);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "Hello");
        assert_eq!(spans[0].style.fg, Some(ratatui::style::Color::Red));
        assert_eq!(spans[2].content, "hello");
        assert_eq!(spans[2].style.fg, Some(ratatui::style::Color::Red));
    }

    #[test]
    fn highlight_matches_empty_query() {
        let spans = highlight_matches("hello world", "", Style::default());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "hello world");
    }

    #[test]
    fn highlight_matches_no_match() {
        let spans = highlight_matches(
            "hello world",
            "xyz",
            Style::default().fg(ratatui::style::Color::Red),
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "hello world");
    }

    #[test]
    fn highlight_spans_preserves_surrounding_style() {
        let base = Style::default().fg(ratatui::style::Color::Red);
        let match_style = Style::default().bg(ratatui::style::Color::Blue);
        let spans = vec![Span::styled("abc Needle def", base)];
        let out = highlight_spans(spans, "needle", match_style);
        assert_eq!(out.len(), 3, "before + match + after");
        assert_eq!(out[0].content, "abc ");
        assert_eq!(out[0].style.fg, Some(ratatui::style::Color::Red));
        assert_eq!(out[1].content, "Needle");
        assert_eq!(out[1].style.bg, Some(ratatui::style::Color::Blue));
        assert_eq!(out[2].content, " def");
        assert_eq!(out[2].style.fg, Some(ratatui::style::Color::Red));
    }

    #[test]
    fn highlight_spans_across_multiple_spans() {
        let spans = vec![
            Span::styled("ab", Style::default()),
            Span::styled("cd", Style::default()),
        ];
        let out = highlight_spans(
            spans,
            "bc",
            Style::default().bg(ratatui::style::Color::Blue),
        );
        // A match straddling two spans is found within each span separately:
        // "a", "b" (styled), "c" (styled), "d".
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].content, "a");
        assert_eq!(out[1].content, "b");
        assert_eq!(out[3].content, "d");
    }

    #[test]
    fn highlight_spans_empty_query_is_identity() {
        let spans = vec![Span::styled("hello", Style::default())];
        let out = highlight_spans(
            spans,
            "  ",
            Style::default().bg(ratatui::style::Color::Blue),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].content, "hello");
    }

    #[test]
    fn highlight_code_colors_tokens() {
        let theme = Theme::detect();
        let st = MarkdownStyles::new(&theme);
        let mut in_block = false;
        let spans = highlight_code("let x = 1; // note", CodeLang::Slash, &st, &mut in_block);
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "let x = 1; // note", "round trip: {spans:?}");
        assert!(spans
            .iter()
            .any(|s| s.content == "let" && s.style == st.code_keyword));
        assert!(spans
            .iter()
            .any(|s| s.content == "1" && s.style == st.code_number));
        assert!(spans
            .iter()
            .any(|s| s.content.contains("// note") && s.style == st.code_comment));
    }

    #[test]
    fn highlight_code_strings_and_hash_comments() {
        let theme = Theme::detect();
        let st = MarkdownStyles::new(&theme);
        let mut in_block = false;
        let spans = highlight_code("print(\"hi\")  # todo", CodeLang::Hash, &st, &mut in_block);
        assert!(spans
            .iter()
            .any(|s| s.content.contains("\"hi\"") && s.style == st.code_string));
        assert!(spans
            .iter()
            .any(|s| s.content.contains("# todo") && s.style == st.code_comment));
    }

    #[test]
    fn highlight_code_block_comments_span_lines() {
        let theme = Theme::detect();
        let st = MarkdownStyles::new(&theme);
        let mut in_block = false;
        let first = highlight_code("/* start", CodeLang::Slash, &st, &mut in_block);
        assert!(in_block, "block comment left open");
        let second = highlight_code("end */ let", CodeLang::Slash, &st, &mut in_block);
        assert!(!in_block, "block comment closed");
        assert!(first
            .iter()
            .all(|s| s.style == st.code_comment || s.style == st.fence));
        assert!(second
            .iter()
            .any(|s| s.content.contains("end */") && s.style == st.code_comment));
        assert!(second
            .iter()
            .any(|s| s.content == "let" && s.style == st.code_keyword));
    }

    #[test]
    fn highlight_code_lifetimes_stay_plain() {
        let theme = Theme::detect();
        let st = MarkdownStyles::new(&theme);
        let mut in_block = false;
        let spans = highlight_code("fn f<'a>(x: &'a str)", CodeLang::Slash, &st, &mut in_block);
        // The lifetime must not swallow the rest of the line as a string.
        assert!(
            !spans.iter().any(|s| s.style == st.code_string),
            "lifetimes are not strings: {spans:?}"
        );
    }

    #[test]
    fn markdown_reports_code_block_regions() {
        let theme = Theme::detect();
        let st = MarkdownStyles::new(&theme);
        let (out, blocks) =
            markdown_with_blocks("before\n\n```rust\nlet x = 1;\n```\nafter", 60, &st);
        assert_eq!(blocks.len(), 1, "one fenced block: {blocks:?}");
        let (start, end, source) = &blocks[0];
        assert_eq!(source, "let x = 1;");
        assert!(*end >= *start);
        assert!(*end < out.len());
        // The bottom border row carries the copy hint.
        assert!(out[*end].to_string().contains("copy"));
    }

    #[test]
    fn width_matches_terminal_emoji_rendering() {
        // Variation selector: ⚠ renders 1, ⚠️ renders 2.
        assert_eq!(width("\u{26A0}"), 1);
        assert_eq!(width("\u{26A0}\u{FE0F}"), 2);
        // ZWJ families collapse into one glyph.
        assert_eq!(width("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"), 2);
        // Flags: two regional indicators, one 2-cell glyph.
        assert_eq!(width("\u{1F1F5}\u{1F1ED}"), 2);
        // Skin-tone modifiers add nothing.
        assert_eq!(width("\u{1F44D}\u{1F3FD}"), 2);
        // Plain text and CJK are untouched.
        assert_eq!(width("hello"), 5);
        assert_eq!(width("日本語"), 6);
        // A non-RI char closes an open regional pair.
        assert_eq!(width("\u{1F1F5}x\u{1F1ED}"), 5);
    }

    #[test]
    fn professionalize_maps_status_emoji() {
        assert_eq!(professionalize("\u{2705} done"), "\u{2713} done");
        assert_eq!(professionalize("\u{2714}\u{FE0F} ok"), "\u{2713} ok");
        assert_eq!(professionalize("\u{274C} fail"), "\u{2717} fail");
        assert_eq!(professionalize("\u{26A0}\u{FE0F} careful"), "! careful");
    }

    #[test]
    fn professionalize_strips_pictographs() {
        assert_eq!(professionalize("party \u{1F389}"), "party ");
        assert_eq!(professionalize("\u{1F1F5}\u{1F1ED} flag"), " flag");
        assert_eq!(professionalize("thumb \u{1F44D}\u{1F3FD}"), "thumb ");
        assert_eq!(professionalize("star \u{2B50}"), "star ");
        // Plain text and our own markers are untouched.
        assert_eq!(professionalize("✓ ✗ plain"), "✓ ✗ plain");
    }

    #[test]
    fn truncate_never_splits_an_emoji_cluster() {
        // "⚠️ ok" is 5 cells; a cut to 2 cannot fit the 2-cell cluster, so
        // the selector is dropped with the rest and the bare base stays —
        // a valid glyph that still renders 1 cell (no width drift).
        let out = truncate("\u{26A0}\u{FE0F} ok", 2);
        assert_eq!(out, "\u{26A0}…");
        assert_eq!(width(out.trim_end_matches('…')), 1);
    }

    #[test]
    fn highlight_spans_skips_length_changing_lowercasing() {
        // `İ` lowercases to two chars, desyncing byte offsets; the span must
        // pass through untouched rather than panic on a bad slice.
        let spans = vec![Span::raw("\u{0130} needle")];
        let out = highlight_spans(
            spans,
            "needle",
            Style::default().bg(ratatui::style::Color::Blue),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].content, "\u{0130} needle");
    }
}
