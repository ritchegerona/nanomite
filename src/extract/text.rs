//! Plain-text, Markdown and HTML extractors.
//!
//! All three are dependency-free byte parsers: they decode as UTF-8 (lossily)
//! and build sections without a DOM, so hostile input costs at most the capped
//! read size.

use super::{ExtractError, Extractor, Format, Input, Section, SectionKind, Sink};

/// Pushes paragraph sections into `sink`, keeping one blank line between them so
/// `Document::text` still reads like the source, and marking the document
/// truncated when the section cap cut the input short.
fn push_paragraphs(sink: &mut Sink, text: &str) -> Result<(), ExtractError> {
    let max = sink.limits().max_sections;
    // Ask for one more than the cap so hitting the cap is observable.
    let sections = super::paragraph_sections(text, max.saturating_add(1));
    let hit_cap = sections.len() > max;
    for (i, section) in sections.into_iter().take(max).enumerate() {
        sink.tick()?;
        if i > 0 {
            sink.push_text("\n");
        }
        if !sink.push_section(section) {
            return Ok(());
        }
    }
    if hit_cap {
        sink.mark_truncated("section cap reached");
    }
    Ok(())
}

/// Unlabelled prose: paragraphs split on blank lines, locators = line ranges.
pub struct TextExtractor;

impl Extractor for TextExtractor {
    fn name(&self) -> &'static str {
        "text"
    }

    fn supports(&self, format: Format) -> bool {
        matches!(format, Format::Text)
    }

    fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let text = decode(input.bytes);
        push_paragraphs(sink, &text)
    }
}

/// Minimal Markdown structure: ATX headings become heading sections, fenced
/// code blocks become code sections, everything else stays prose. Inline
/// markup is preserved verbatim so retrieval matches the source text.
pub struct MarkdownExtractor;

impl Extractor for MarkdownExtractor {
    fn name(&self) -> &'static str {
        "markdown"
    }

    fn supports(&self, format: Format) -> bool {
        matches!(format, Format::Markdown)
    }

    fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let text = decode(input.bytes);
        let mut fence_marker: Option<char> = None;
        let mut heading: Option<(u8, String)> = None;
        let mut prose = String::new();
        let mut prose_start = 1usize;
        let mut fence = String::new();
        let mut fence_start = 1usize;
        let mut line_no = 0usize;

        macro_rules! flush_prose {
            ($sink:expr, $heading:expr, $prose:expr, $start:expr, $end:expr) => {
                if !$prose.trim().is_empty() {
                    let text = $prose.trim_end().to_string();
                    let (level, title) = $heading.take().unwrap_or((0, String::new()));
                    let section = if title.is_empty() {
                        Section {
                            kind: SectionKind::Paragraph,
                            title: None,
                            level: None,
                            locator: super::Locator::Lines {
                                start: $start,
                                end: $end.max($start),
                            },
                            text,
                        }
                    } else {
                        Section {
                            kind: SectionKind::Heading,
                            title: Some(title),
                            level: Some(level),
                            locator: super::Locator::Lines {
                                start: $start,
                                end: $end.max($start),
                            },
                            text,
                        }
                    };
                    let _ = $sink.push_section(section);
                    $prose.clear();
                }
            };
        }

        for line in text.split_inclusive('\n') {
            line_no += 1;
            sink.tick()?;
            let trimmed = line.trim();

            // Fenced code blocks: capture verbatim, never parse inside.
            if let Some(marker) = fence_marker {
                if trimmed.starts_with(marker) && trimmed.chars().all(|c| c == marker) {
                    let text = fence.trim_end().to_string();
                    fence_marker = None;
                    fence.clear();
                    sink.push_section(Section {
                        kind: SectionKind::CodeBlock,
                        title: None,
                        level: None,
                        locator: super::Locator::Lines {
                            start: fence_start,
                            end: line_no,
                        },
                        text,
                    });
                    // The closing delimiter belongs to the flat text (so a
                    // search for "```rust" hits) but not to the code section,
                    // which holds only the code.
                    sink.push_line(trimmed);
                    continue;
                }
                fence.push_str(line);
                if fence.len() > 64 * 1024 {
                    fence.truncate(64 * 1024);
                    sink.mark_truncated("fenced code block cut at 64 KiB");
                    fence_marker = None;
                }
                continue;
            }
            if let Some(first) = trimmed.chars().next() {
                if (first == '`' || first == '~')
                    && trimmed.len() >= 3
                    && trimmed.chars().take_while(|c| *c == first).count() >= 3
                {
                    flush_prose!(sink, heading, prose, prose_start, line_no.saturating_sub(1));
                    fence_marker = Some(first);
                    fence_start = line_no + 1;
                    // The fence is captured verbatim and never parsed, so its
                    // delimiters stay in the flat retrieval text.
                    fence.clear();
                    sink.push_line(trimmed);
                    continue;
                }
            }

            if let Some((level, title)) = heading_line(trimmed) {
                flush_prose!(sink, heading, prose, prose_start, line_no.saturating_sub(1));
                // Emit the previous heading's own section boundary; the heading
                // text itself rides on the following block (or its own section).
                heading = Some((level, title));
                prose_start = line_no;
                // Give a heading that is immediately followed by another
                // heading its own empty section so titles still index.
                continue;
            }

            if prose.is_empty() {
                prose_start = line_no;
            }
            prose.push_str(line);
        }
        flush_prose!(sink, heading, prose, prose_start, line_no);

        // A document that is nothing but headings still needs its titles.
        if sink.sections.is_empty() {
            for (level, title) in all_headings(&text) {
                if !sink.push_section(Section {
                    kind: SectionKind::Heading,
                    title: Some(title),
                    level: Some(level),
                    locator: super::Locator::None,
                    text: String::new(),
                }) {
                    break;
                }
            }
        }
        if let Some(first) = first_title(&text) {
            sink.sections.insert(
                0,
                Section {
                    kind: SectionKind::Title,
                    title: Some(first),
                    level: Some(1),
                    locator: super::Locator::Lines { start: 1, end: 1 },
                    text: String::new(),
                },
            );
        }
        Ok(())
    }
}

fn heading_line(trimmed: &str) -> Option<(u8, String)> {
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 || !trimmed[hashes..].starts_with(' ') {
        return None;
    }
    let title = trimmed[hashes..]
        .trim()
        .trim_end_matches('#')
        .trim()
        .to_string();
    if title.is_empty() {
        None
    } else {
        Some((hashes as u8, title))
    }
}

fn all_headings(text: &str) -> Vec<(u8, String)> {
    text.lines()
        .filter_map(|l| heading_line(l.trim()))
        .collect()
}

fn first_title(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| heading_line(l.trim()).map(|(_, t)| t))
}

/// Byte-level HTML stripper: tag-aware state machine, script/style dropped
/// wholesale (their text is never extracted), `<h1>`-`<h6>` open new sections,
/// `<li>`/`<p>`/`<td>` provide block breaks.
pub struct HtmlExtractor;

impl Extractor for HtmlExtractor {
    fn name(&self) -> &'static str {
        "html"
    }

    fn supports(&self, format: Format) -> bool {
        matches!(format, Format::Html)
    }

    fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let raw = decode(input.bytes);
        let html = strip_html(&raw, sink)?;
        // Title first, then body sections from the cleaned text.
        if let Some(title) = html_title(&raw) {
            sink.push_section(Section {
                kind: SectionKind::Title,
                title: Some(title),
                level: Some(1),
                locator: super::Locator::None,
                text: String::new(),
            });
        }
        push_paragraphs(sink, &html)
    }
}

fn html_title(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open = lower[start..].find('>')? + start + 1;
    let end = lower[open..].find("</title>")? + open;
    let title = raw[open..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

/// Removes markup while tracking block structure. Entity decoding is limited
/// to the named basics plus numeric references; unknown entities stay literal.
pub(crate) fn strip_html(raw: &str, sink: &mut Sink) -> Result<String, ExtractError> {
    let mut out = String::with_capacity(raw.len() / 2);
    let bytes = raw.as_bytes();
    let mut i = 0usize;
    let mut heading_level: Option<u8> = None;
    let mut in_heading = false;

    while i < bytes.len() {
        if i.is_multiple_of(4096) {
            sink.tick()?;
        }
        if bytes[i] == b'<' {
            let Some(end) = raw[i..].find('>') else {
                // Truncated tag at EOF: keep the literal tail.
                out.push_str(&decode_entities(&raw[i..]));
                break;
            };
            let tag = raw[i + 1..i + end].trim();
            i += end + 1;
            let lower = tag.to_ascii_lowercase();
            let name: String = lower
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '/')
                .collect();
            match name.as_str() {
                "script" | "style" | "noscript" | "template" | "svg" | "iframe" | "object"
                | "embed" | "canvas" | "head" => {
                    // Skip to the matching close tag; contents are never text.
                    if let Some(close) = find_close(&raw[i..], &name) {
                        i += close;
                    } else {
                        i = bytes.len();
                    }
                    continue;
                }
                "br" | "hr" | "tr" | "/tr" | "p" | "/p" | "div" | "/div" | "li" | "/li" | "ul"
                | "/ul" | "ol" | "/ol" | "table" | "/table" | "blockquote" | "/blockquote"
                | "section" | "/section" | "article" | "/article" => {
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "/h1" | "/h2" | "/h3" | "/h4" | "/h5"
                | "/h6" => {
                    if name.starts_with('/') {
                        // Close of a heading: put a blank line after it so the
                        // heading paragraph splits out as its own section.
                        if in_heading {
                            out.push_str("\n\n");
                        }
                        in_heading = false;
                        heading_level = None;
                    } else {
                        if !out.ends_with('\n') {
                            out.push('\n');
                        }
                        heading_level = name[1..].parse::<u8>().ok();
                        in_heading = true;
                    }
                }
                "title" | "/title" | "meta" | "link" => {}
                _ => {
                    // Any other tag: attributes dropped, text flows on.
                    let _ = heading_level;
                }
            }
            continue;
        }
        let next = raw[i..].find('<').map(|p| i + p).unwrap_or(bytes.len());
        let chunk = &raw[i..next];
        // An entity cannot straddle a chunk boundary: a chunk ends only at a
        // `<`, and no entity name contains one.
        out.push_str(&decode_entities(chunk));
        i = next;
    }
    Ok(out)
}

fn find_close(rest: &str, name: &str) -> Option<usize> {
    let lower = rest.to_ascii_lowercase();
    let close = format!("</{name}");
    let _idx = lower.find(&close)?;
    // Skip nested same-name tags.
    let mut depth = 1usize;
    let mut scan = 0usize;
    while scan < lower.len() {
        if let Some(open) = lower[scan..].find(&format!("<{name}")) {
            depth += 1;
            scan += open + name.len() + 1;
        } else {
            let cl = lower[scan..].find(&close)?;
            depth -= 1;
            if depth == 0 {
                let mut end = scan + cl + close.len();
                if let Some(gt) = rest[end..].find('>') {
                    end += gt + 1;
                }
                return Some(end);
            }
            scan += cl + close.len();
        }
    }
    None
}

/// Decodes a claimed text source. Invalid bytes are replaced rather than
/// fatal: a single stray byte in a source file must not cost us the rest of
/// the document, and the replacement character marks exactly where it happened.
fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Decodes the five predefined entities plus numeric character references.
/// Deliberately no general entity table: external/DTD entities are ignored so
/// the stripper cannot be tricked into expanding them.
pub(crate) fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let Some(semi) = rest.find(';').filter(|s| *s <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let replacement: Option<String> = match entity {
            "amp" => Some("&".into()),
            "lt" => Some("<".into()),
            "gt" => Some(">".into()),
            "quot" => Some("\"".into()),
            "apos" => Some("'".into()),
            _ => {
                // `&#65;` is decimal, `&#x41;` is hex. The `x` prefix is what
                // distinguishes them, so it must be checked, not assumed.
                let numeric = entity.strip_prefix('#').and_then(|n| {
                    match n.strip_prefix('x').or_else(|| n.strip_prefix('X')) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse::<u32>().ok(),
                    }
                });
                numeric.and_then(char::from_u32).map(|c| c.to_string())
            }
        };
        match replacement {
            Some(r) => out.push_str(&r),
            None => out.push_str(&rest[..=semi]),
        }
        rest = &rest[semi + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{ExtractionLimits, ExtractorRegistry};

    fn scratch(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nanomite-text-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn extract(name: &str, bytes: &[u8]) -> crate::extract::Document {
        let path = scratch(name, bytes);
        let doc = extract_path(&path);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        doc
    }

    fn extract_path(path: &std::path::Path) -> crate::extract::Document {
        ExtractorRegistry::builtin()
            .extract_path(path, &ExtractionLimits::default())
            .expect("extract")
    }

    #[test]
    fn plain_text_splits_into_paragraphs_with_line_locators() {
        let doc = extract("a.txt", b"first para\nstill first\n\nsecond para\n");
        assert_eq!(doc.format, Format::Text);
        assert_eq!(doc.sections.len(), 2);
        assert_eq!(
            doc.sections[0].locator,
            crate::extract::Locator::Lines { start: 1, end: 2 }
        );
        assert_eq!(doc.sections[1].text, "second para");
        assert_eq!(doc.text, "first para\nstill first\n\nsecond para");
    }

    #[test]
    fn markdown_headings_and_fences_become_sections() {
        let md = "# Title\n\nIntro paragraph.\n\n## Sub\n\n```rust\nfn x() {}\n```\n\nEnd.\n";
        let doc = extract("b.md", md.as_bytes());
        let kinds: Vec<_> = doc.sections.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&SectionKind::Heading));
        assert!(kinds.contains(&SectionKind::CodeBlock));
        let code = doc
            .sections
            .iter()
            .find(|s| s.kind == SectionKind::CodeBlock)
            .unwrap();
        assert_eq!(code.text, "fn x() {}");
        let heading = doc
            .sections
            .iter()
            .find(|s| s.kind == SectionKind::Heading)
            .expect("heading section");
        assert_eq!(heading.title.as_deref(), Some("Title"));
        // Markdown remains available as retrieval text: fences are not parsed.
        assert!(doc.text.contains("```rust"));
    }

    #[test]
    fn markdown_title_indexed_even_when_headings_are_the_whole_file() {
        let doc = extract("c.md", b"# Only A Title\n");
        assert!(doc
            .sections
            .iter()
            .any(|s| s.title.as_deref() == Some("Only A Title")));
    }

    #[test]
    fn html_strips_script_style_and_keeps_prose() {
        let html = r#"<!doctype html><html><head><title>Doc Title</title>
            <style>p{color:red}</style></head>
            <body><h1>Heading</h1><p>First &amp; second &lt;ok&gt;.</p>
            <script>var secret = "not extracted";</script><p>After.</p></body></html>"#;
        let doc = extract("d.html", html.as_bytes());
        assert_eq!(doc.sections[0].title.as_deref(), Some("Doc Title"));
        assert!(
            !doc.text.contains("secret"),
            "script text leaked: {}",
            doc.text
        );
        assert!(!doc.text.contains("color:red"), "style text leaked");
        assert!(doc.text.contains("First & second <ok>."));
        assert!(doc.text.contains("After."));
        // Decoded entities may legitimately produce `<` (`&lt;ok&gt;`), so the
        // check is that no *markup* survived the strip.
        for tag in ["<p", "<body", "<h1", "<script", "<style", "</"] {
            assert!(!doc.text.contains(tag), "tags leaked: {}", doc.text);
        }
    }

    #[test]
    fn html_unclosed_tag_does_not_hang() {
        let doc = extract("e.html", b"<p>text <span>more</p tail");
        assert!(doc.text.contains("text"));
    }

    #[test]
    fn html_nested_script_close_handled() {
        let html = "<body><script>if (a<b) { x(); }</script>visible</body>";
        let doc = extract("f.html", html.as_bytes());
        assert!(doc.text.contains("visible"));
        assert!(!doc.text.contains("if (a"));
    }

    #[test]
    fn entity_decoding_covers_basics_and_numbers() {
        assert_eq!(
            decode_entities("a&amp;b&#65;&#x42;&unknown;"),
            "a&bAB&unknown;"
        );
        assert_eq!(decode_entities("plain"), "plain");
    }

    #[test]
    fn non_utf8_text_recovers_lossily() {
        let doc = extract("g.txt", &[b'h', 0xff, b'i']);
        assert!(doc.text.contains('h'));
    }

    #[test]
    fn section_cap_marks_truncated() {
        let mut body = String::new();
        for i in 0..50 {
            body.push_str(&format!("para {i}\n\n"));
        }
        let path = scratch("h.txt", body.as_bytes());
        let limits = ExtractionLimits {
            max_sections: 5,
            ..ExtractionLimits::default()
        };
        let doc = ExtractorRegistry::builtin()
            .extract_path(&path, &limits)
            .unwrap();
        assert_eq!(doc.sections.len(), 5);
        assert!(doc.truncated);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
