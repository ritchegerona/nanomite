//! Document extractors: RTF (always compiled), OOXML containers (docx/odt,
//! behind `format-documents`) and PDF (behind `format-pdf`).
//!
//! Every extractor here follows the layer's rules:
//!
//! - **Read only.** Originals are opened, never rewritten.
//! - **Untrusted input.** Embedded macros, images, OLE objects, fields and
//!   stylesheets are *data*: they are skipped, never interpreted, and no
//!   external reference (DTD, OLE link, remote font) is ever resolved.
//! - **Bounded work.** Archive reads go through [`zipcontainer::Budget`], PDF
//!   pages through the shared sink caps, and every loop ticks the deadline.
//! - **Partial results beat failure.** A corrupt paragraph, run or page becomes
//!   a warning plus whatever text was recoverable.

use super::{
    ExtractError, ExtractionLimits, Extractor, Format, Input, Locator, Section, SectionKind, Sink,
};

// ---------------------------------------------------------------------------
// RTF
// ---------------------------------------------------------------------------

/// Rich Text Format: a dependency-free control-word scanner over the byte
/// stream. Only text-bearing destinations are followed; everything else
/// (`\fonttbl`, `\colortbl`, `\stylesheet`, `\pict`, `\object`, `\*\...`) is
/// skipped without interpretation.
pub struct RtfExtractor;

impl Extractor for RtfExtractor {
    fn name(&self) -> &'static str {
        "rtf"
    }

    fn supports(&self, format: Format) -> bool {
        format == Format::Rtf
    }

    fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let body = String::from_utf8_lossy(input.bytes);
        let text = rtf_to_text(&body, sink)?;
        let paragraphs: Vec<&str> = text.split('\n').filter(|p| !p.trim().is_empty()).collect();
        if paragraphs.is_empty() {
            return Ok(());
        }
        for (i, para) in paragraphs.iter().enumerate() {
            sink.tick()?;
            sink.push_section(Section {
                kind: SectionKind::Paragraph,
                title: None,
                level: None,
                locator: Locator::Lines {
                    start: i + 1,
                    end: i + 1,
                },
                text: para.trim().to_string(),
            });
        }
        Ok(())
    }
}

/// RTF control words whose destination never contributes body text.
const RTF_SKIP_DESTINATIONS: &[&str] = &[
    "fonttbl",
    "colortbl",
    "stylesheet",
    "listtable",
    "listoverridetable",
    "list",
    "listlevel",
    "revtbl",
    "rsidtbl",
    "generator",
    "info",
    "pict",
    "object",
    "objdata",
    "themedata",
    "colorschememapping",
    "latentstyles",
    "datastore",
    "header",
    "headerl",
    "headerr",
    "headerf",
    "footer",
    "footerl",
    "footerr",
    "footerf",
    "footnote",
    "annotation",
    "field",
    "fldinst",
    "xmlnstbl",
    "filetbl",
    "mmathPr",
    "wgrffmtfilter",
    "shppict",
    "nonshppict",
];

/// Converts RTF markup to plain text.
///
/// Handles groups, control words, the `\'hh` hex escape, `\uN?` unicode
/// escapes with their `\ucN` skip count, and the textual escapes. Destinations
/// in [`RTF_SKIP_DESTINATIONS`] and unknown `\*\...` destinations are dropped.
fn rtf_to_text(input: &str, sink: &mut Sink) -> Result<String, ExtractError> {
    let bytes = input.as_bytes();
    let mut out = String::new();
    // (skip_until_group_end, unicode skip count)
    let mut skip_stack: Vec<(bool, i32)> = Vec::new();
    let mut skip = false;
    let mut uc = 1i32;
    let mut i = 0usize;
    let mut steps = 0usize;

    while i < bytes.len() {
        steps += 1;
        if steps.is_multiple_of(4096) {
            sink.tick()?;
        }

        let b = bytes[i];
        match b {
            b'{' => {
                skip_stack.push((skip, uc));
                skip = false;
                i += 1;
            }
            b'}' => {
                let Some((outer_skip, outer_uc)) = skip_stack.pop() else {
                    // Unbalanced `}`: ignore it rather than failing the file.
                    i += 1;
                    continue;
                };
                skip = outer_skip;
                uc = outer_uc;
                i += 1;
            }
            b'\\' => {
                i += 1;
                if i >= bytes.len() {
                    break;
                }
                let c = bytes[i];
                match c {
                    // Escaped literal braces and backslash.
                    b'{' | b'}' | b'\\' => {
                        if !skip {
                            out.push(c as char);
                        }
                        i += 1;
                    }
                    // `\'hh`: one byte in the current codepage.
                    b'\'' => {
                        i += 1;
                        let hi = hex_val(bytes.get(i).copied().unwrap_or(0));
                        let lo = hex_val(bytes.get(i + 1).copied().unwrap_or(0));
                        i += 2;
                        if !skip {
                            // Latin-1 is the safe superset: RTF without
                            // `\'` escapes is already treated as Unicode.
                            push_byte(&mut out, (hi << 4) | lo);
                        }
                    }
                    // `\*`: marks an ignorable destination.
                    b'*' => {
                        skip = true;
                        i += 1;
                    }
                    // Escaped newline means `\par`, not a line break.
                    b'\n' | b'\r' => {
                        if !skip {
                            out.push('\n');
                        }
                        i += 1;
                    }
                    _ => {
                        // Control word: letters, optional signed number, one
                        // optional delimiter space.
                        let start = i;
                        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        let word = &input[start..i];
                        let num_start = i;
                        if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
                            i += 1;
                        }
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                        let param: Option<i64> = if i > num_start {
                            input[num_start..i].parse::<i64>().ok()
                        } else {
                            None
                        };
                        // One delimiter space is consumed with the control word.
                        if i < bytes.len() && bytes[i] == b' ' {
                            i += 1;
                        }

                        match word {
                            "par" | "line" => {
                                if !skip {
                                    out.push('\n');
                                }
                            }
                            "tab" => {
                                if !skip {
                                    out.push('\t');
                                }
                            }
                            "cell" | "row" => {
                                if !skip {
                                    out.push('\t');
                                }
                            }
                            "cellx" => {}
                            "~" => {
                                if !skip {
                                    out.push(' ');
                                }
                            }
                            "-" | "_" => {
                                if !skip {
                                    out.push('-');
                                }
                            }
                            "u" => {
                                if let Some(code) = param {
                                    if !skip {
                                        match u32::try_from(code).ok().and_then(char::from_u32) {
                                            Some(ch) => out.push(ch),
                                            None => out.push('\u{fffd}'),
                                        }
                                    }
                                    // Drop the `\ucN` fallback characters.
                                    i += skip_unicode_fallback(input, i, uc as usize);
                                }
                            }
                            "uc" => {
                                uc = param.unwrap_or(1).clamp(0, 10) as i32;
                            }
                            _ => {
                                if !skip && RTF_SKIP_DESTINATIONS.contains(&word) {
                                    skip = true;
                                }
                            }
                        }
                    }
                }
            }
            b'\r' | b'\n' => {
                // A raw newline in the body is insignificant whitespace.
                i += 1;
            }
            _ => {
                // Printable run: copy the whole UTF-8 sequence at once.
                if !skip {
                    let ch = input[i..].chars().next().unwrap_or('\u{fffd}');
                    if ch == '\u{fffd}' {
                        i += 1;
                    } else {
                        out.push(ch);
                        i += ch.len_utf8();
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    Ok(out)
}

/// Skips the `count` fallback characters that follow a `\uN` escape, returning
/// how many bytes were consumed. A fallback character may itself be an escape
/// (`\'3f` stands in for `?`), so both forms are handled.
fn skip_unicode_fallback(input: &str, from: usize, count: usize) -> usize {
    let bytes = input.as_bytes();
    let mut i = from;
    let mut skipped = 0usize;
    while i < bytes.len() && skipped < count {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 1;
            if bytes[i] == b'\'' {
                i += 3.min(bytes.len() - i);
            } else {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
            }
            skipped += 1;
            continue;
        }
        let ch = input[i..].chars().next().unwrap_or('\u{fffd}');
        i += ch.len_utf8();
        skipped += 1;
    }
    i - from
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

fn push_byte(out: &mut String, byte: u8) {
    match char::from_u32(u32::from(byte)) {
        Some(ch) => out.push(ch),
        None => out.push('\u{fffd}'),
    }
}

// ---------------------------------------------------------------------------
// OOXML / ODF containers (docx, odt)
// ---------------------------------------------------------------------------

/// WordprocessingML (`.docx`) and OpenDocument Text (`.odt`).
///
/// Both are zip containers, so every read is budgeted and every entry name is
/// validated by [`super::zipcontainer`]. Only `w:t` / `text:span` text nodes are
/// read; macros (`vbaProject.bin`), embedded objects and styles are ignored.
#[cfg(feature = "format-documents")]
pub struct OoxmlExtractor;

#[cfg(feature = "format-documents")]
impl Extractor for OoxmlExtractor {
    fn name(&self) -> &'static str {
        "ooxml"
    }

    fn supports(&self, format: Format) -> bool {
        matches!(format, Format::Docx | Format::Odt)
    }

    fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let mut archive = super::zipcontainer::open(input.bytes)?;
        let mut budget = super::zipcontainer::Budget::new(sink.limits());

        match input.format {
            Format::Docx => {
                let xml = super::zipcontainer::read_optional(
                    &mut archive,
                    "word/document.xml",
                    &mut budget,
                )?;
                match xml {
                    Some(bytes) => extract_docx_xml(&bytes, sink),
                    None => Err(ExtractError::Malformed(
                        "docx is missing word/document.xml".into(),
                    )),
                }
            }
            Format::Odt => {
                let xml =
                    super::zipcontainer::read_optional(&mut archive, "content.xml", &mut budget)?;
                match xml {
                    Some(bytes) => extract_odt_xml(&bytes, sink),
                    None => Err(ExtractError::Malformed("odt is missing content.xml".into())),
                }
            }
            _ => Ok(()),
        }
    }
}

/// Streaming reader over an OOXML part. Tracks only what body text needs.
#[cfg(feature = "format-documents")]
struct XmlCursor<'a> {
    reader: quick_xml::Reader<&'a [u8]>,
    buf: Vec<u8>,
}

#[cfg(feature = "format-documents")]
impl<'a> XmlCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        let mut reader = quick_xml::Reader::from_reader(bytes);
        let config = reader.config_mut();
        config.trim_text(false);
        config.check_end_names = false;
        Self {
            reader,
            buf: Vec::new(),
        }
    }

    /// Next event, owned. Element names keep their namespace prefix here;
    /// callers read [`quick_xml::name::QName::local_name`]. Rebuilding the
    /// element here would discard its attributes.
    fn next(&mut self) -> Result<quick_xml::events::Event<'a>, ExtractError> {
        self.buf.clear();
        self.reader
            .read_event_into(&mut self.buf)
            .map(quick_xml::events::Event::into_owned)
            .map_err(|e| ExtractError::Malformed(format!("malformed OOXML part: {e}")))
    }
}

/// Extracts `.docx` body text: paragraphs become `Paragraph` sections and
/// `Heading1..6` styles become titled `Heading` sections. Paragraphs inside a
/// table are charged against the row budget like any other table row.
#[cfg(feature = "format-documents")]
fn extract_docx_xml(bytes: &[u8], sink: &mut Sink) -> Result<(), ExtractError> {
    use quick_xml::events::Event;

    let mut cursor = XmlCursor::new(bytes);
    let mut paragraph = String::new();
    let mut heading_level: Option<u8> = None;
    let mut in_text = false;
    let mut table_depth = 0usize;
    let mut paragraph_index = 0usize;

    loop {
        sink.tick()?;
        match cursor.next()? {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name().local_name().into_inner().to_string();
                match name.as_str() {
                    // A new paragraph closes the previous one.
                    "p" => flush_docx_paragraph(
                        sink,
                        &mut paragraph,
                        heading_level.take(),
                        &mut paragraph_index,
                        table_depth > 0,
                    ),
                    "pStyle" => {
                        if let Some(val) = attribute(&e, "val") {
                            heading_level = parse_heading_style(&val);
                        }
                    }
                    "tbl" => table_depth += 1,
                    "t" => in_text = true,
                    "tab" if in_text => paragraph.push('\t'),
                    "br" if in_text => paragraph.push(' '),
                    _ => {}
                }
            }
            Event::Text(t) => {
                if in_text {
                    // Fragments are concatenated verbatim: trimming here would
                    // eat the spaces that separate runs.
                    let decoded = super::text::decode_entities(t.into_inner().as_ref());
                    paragraph.push_str(&decoded);
                }
            }
            Event::CData(c) => {
                if in_text {
                    paragraph.push_str(c.into_inner().as_ref());
                }
            }
            Event::GeneralRef(r) => {
                // quick-xml reports `&amp;` separately from surrounding text.
                // Only the five predefined names and numeric refs resolve; an
                // unknown entity stays literal so nothing external is expanded.
                if in_text {
                    let decoded = super::text::decode_entities(&format!("&{};", r.as_ref()));
                    paragraph.push_str(&decoded);
                }
            }
            Event::End(e) => match e.name().local_name().into_inner() {
                "t" => in_text = false,
                "p" => flush_docx_paragraph(
                    sink,
                    &mut paragraph,
                    heading_level.take(),
                    &mut paragraph_index,
                    table_depth > 0,
                ),
                "tbl" => table_depth = table_depth.saturating_sub(1),
                _ => {}
            },
            _ => {}
        }
    }
    flush_docx_paragraph(
        sink,
        &mut paragraph,
        heading_level.take(),
        &mut paragraph_index,
        table_depth > 0,
    );
    Ok(())
}

#[cfg(feature = "format-documents")]
fn flush_docx_paragraph(
    sink: &mut Sink,
    buffer: &mut String,
    level: Option<u8>,
    index: &mut usize,
    in_table: bool,
) {
    let text = buffer.trim().to_string();
    buffer.clear();
    if text.is_empty() {
        return;
    }
    if in_table && !sink.charge_rows(1) {
        sink.mark_truncated("docx row cap reached");
        return;
    }
    *index += 1;
    let kind = match level {
        Some(_) => SectionKind::Heading,
        None if in_table => SectionKind::TableRow,
        None => SectionKind::Paragraph,
    };
    sink.push_section(Section {
        kind,
        title: level.map(|_| text.clone()),
        level,
        locator: Locator::Lines {
            start: *index,
            end: *index,
        },
        text,
    });
}

/// `Heading1`..`Heading6` style names map to a heading level; anything else
/// (`Normal`, `ListParagraph`, ...) stays prose.
#[cfg(feature = "format-documents")]
fn parse_heading_style(style: &str) -> Option<u8> {
    let lowered = style.to_ascii_lowercase();
    let rest = lowered.strip_prefix("heading")?;
    rest.parse::<u8>().ok().filter(|l| (1..=6).contains(l))
}

/// Reads an attribute by local name.
#[cfg(feature = "format-documents")]
fn attribute(e: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|attr| {
        if attr.key.local_name().into_inner() == key {
            Some(attr.value.into_owned())
        } else {
            None
        }
    })
}

/// Extracts `.odt` body text: `text:h` becomes a titled heading, `text:p` a
/// paragraph, and each table row is charged against the row budget.
#[cfg(feature = "format-documents")]
fn extract_odt_xml(bytes: &[u8], sink: &mut Sink) -> Result<(), ExtractError> {
    use quick_xml::events::Event;

    let mut cursor = XmlCursor::new(bytes);
    let mut buffer = String::new();
    let mut title: Option<String> = None;
    let mut level: Option<u8> = None;
    let mut in_heading = false;
    let mut in_span = false;
    let mut in_value = false;
    let mut in_table_cell = false;
    let mut element_count = 0usize;

    loop {
        if element_count.is_multiple_of(512) {
            sink.tick()?;
        }
        if element_count >= sink.limits().max_xml_elements {
            sink.mark_truncated("odt element cap reached");
            break;
        }
        element_count += 1;

        match cursor.next()? {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name().local_name().into_inner().to_string();
                match name.as_str() {
                    "h" => {
                        in_heading = true;
                        level = attribute(&e, "outline-level")
                            .and_then(|v| v.parse::<u8>().ok())
                            .filter(|l| (1..=6).contains(l));
                    }
                    "span" => in_span = true,
                    "table-cell" => in_table_cell = true,
                    // A bare `<text:p>` carries its text directly; a span or a
                    // heading nests the text one level deeper.
                    "p" => {
                        buffer.clear();
                        in_value = true;
                    }
                    "line-break" => buffer.push(' '),
                    "tab" => buffer.push('\t'),
                    _ => {}
                }
            }
            Event::Text(t) => {
                // `in_value` covers a bare `<text:p>`; a span or heading nests
                // the real text one level down.
                if in_value || in_span || in_heading {
                    let decoded = super::text::decode_entities(t.into_inner().as_ref());
                    buffer.push_str(&decoded);
                }
            }
            Event::CData(c) => {
                if in_value || in_span || in_heading {
                    buffer.push_str(c.into_inner().as_ref());
                }
            }
            Event::GeneralRef(r) => {
                if in_value || in_span || in_heading {
                    let decoded = super::text::decode_entities(&format!("&{};", r.as_ref()));
                    buffer.push_str(&decoded);
                }
            }
            Event::End(e) => {
                let name = e.name().local_name().into_inner().to_string();
                match name.as_str() {
                    "span" => in_span = false,
                    "h" => {
                        in_heading = false;
                        let text = buffer.trim().to_string();
                        if !text.is_empty() {
                            title = Some(text.clone());
                            sink.push_section(Section {
                                kind: SectionKind::Heading,
                                title: Some(text.clone()),
                                level,
                                locator: Locator::None,
                                text,
                            });
                            buffer.clear();
                        }
                        level = None;
                    }
                    "p" => {
                        let text = buffer.trim().to_string();
                        buffer.clear();
                        if !text.is_empty() {
                            if in_table_cell && !sink.charge_rows(1) {
                                sink.mark_truncated("odt row cap reached");
                                break;
                            }
                            sink.push_section(Section {
                                kind: SectionKind::Paragraph,
                                title: title.take(),
                                level: None,
                                locator: Locator::None,
                                text,
                            });
                        }
                        in_value = false;
                    }
                    "table-cell" => in_table_cell = false,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------

/// PDF text via `pdf-extract`. Pages become one section each so retrieval can
/// cite `p7`. Encrypted PDFs without a password yield a warning and no text
/// rather than an error: the file is simply unreadable to us.
#[cfg(feature = "format-pdf")]
pub struct PdfExtractor;

#[cfg(feature = "format-pdf")]
impl Extractor for PdfExtractor {
    fn name(&self) -> &'static str {
        "pdf"
    }

    fn supports(&self, format: Format) -> bool {
        format == Format::Pdf
    }

    fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let pages = match pdf_extract::extract_text_from_mem_by_pages(input.bytes) {
            Ok(pages) => pages,
            Err(e) => {
                // Encrypted or otherwise unreadable: keep whatever we can and
                // explain why, rather than failing the whole document.
                sink.warn(format!("pdf text extraction produced nothing: {e}"));
                Vec::new()
            }
        };
        for (i, page) in pages.iter().enumerate() {
            sink.tick()?;
            let page_no = i + 1;
            let text = page.trim();
            if text.is_empty() {
                sink.warn(format!("pdf page {page_no} has no extractable text"));
                continue;
            }
            let keep = sink.push_section(Section {
                kind: SectionKind::Page,
                title: Some(format!("page {page_no}")),
                level: None,
                locator: Locator::Page { page: page_no },
                text: text.to_string(),
            });
            if !keep {
                sink.mark_truncated("pdf section cap reached");
                break;
            }
        }
        Ok(())
    }
}

/// Convenience used by callers that only need the limits type in scope.
pub fn default_limits() -> ExtractionLimits {
    ExtractionLimits::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink() -> Sink {
        Sink::new(ExtractionLimits::default())
    }

    // -- RTF ---------------------------------------------------------------

    #[test]
    fn rtf_plain_text_is_extracted() {
        let mut s = sink();
        let rtf = r"{\rtf1\ansi Hello \b world\b0 .\par Second paragraph.\par}";
        let text = rtf_to_text(rtf, &mut s).unwrap();
        assert!(text.contains("Hello"), "{text:?}");
        assert!(text.contains("world"), "{text:?}");
        assert!(text.contains("Second paragraph."), "{text:?}");
    }

    #[test]
    fn rtf_skips_font_and_colour_tables() {
        let mut s = sink();
        let rtf = concat!(
            r"{\rtf1\ansi\deff0",
            r"{\fonttbl{\f0\fnil Times;}}",
            r"{\colortbl;\red0\green0\blue0;}",
            r"{\stylesheet Normal;}",
            r"Body text here.\par}"
        );
        let text = rtf_to_text(rtf, &mut s).unwrap();
        assert!(text.contains("Body text here."), "{text:?}");
        assert!(!text.contains("fnil"), "font table leaked: {text:?}");
        assert!(!text.contains("colortbl"), "colour table leaked: {text:?}");
        assert!(!text.contains("stylesheet"), "stylesheet leaked: {text:?}");
    }

    #[test]
    fn rtf_skips_pictures_and_objects() {
        let mut s = sink();
        let rtf = concat!(
            r"{\rtf1\ansi",
            r"{\pict\pngblip\secretpicturedata}",
            r"{\object\objdata\secretobjectdata}",
            r"Visible text.\par}"
        );
        let text = rtf_to_text(rtf, &mut s).unwrap();
        assert!(text.contains("Visible text."), "{text:?}");
        assert!(!text.contains("secretpicturedata"), "{text:?}");
        assert!(!text.contains("secretobjectdata"), "{text:?}");
    }

    #[test]
    fn rtf_escapes_and_hex_are_decoded() {
        let mut s = sink();
        let rtf = r"{\rtf1\ansi Braces \{ok\} backslash \\ hex \'41 end.\par}";
        let text = rtf_to_text(rtf, &mut s).unwrap();
        assert!(text.contains("Braces {ok}"), "{text:?}");
        assert!(text.contains("backslash \\"), "{text:?}");
        assert!(text.contains("hex A end."), "{text:?}");
    }

    #[test]
    fn rtf_unicode_escape_skips_its_fallback() {
        let mut s = sink();
        // \u9731? is a snowman followed by a one-character ASCII fallback.
        let rtf = r"{\rtf1\ansi\uc1 \u9731? plain.\par}";
        let text = rtf_to_text(rtf, &mut s).unwrap();
        assert!(text.contains('\u{2603}'), "missing snowman: {text:?}");
        assert!(
            !text.contains("\u{2603}?"),
            "fallback not skipped: {text:?}"
        );
    }

    #[test]
    fn rtf_unbalanced_braces_do_not_panic() {
        let mut s = sink();
        let _ = rtf_to_text(r"{\rtf1 text}} more {", &mut s).unwrap();
        let _ = rtf_to_text("no groups at all", &mut s).unwrap();
        let _ = rtf_to_text(r"{\rtf1 trailing backslash \", &mut s).unwrap();
    }

    #[test]
    fn rtf_extractor_claims_only_rtf() {
        let e = RtfExtractor;
        assert!(e.supports(Format::Rtf));
        assert!(!e.supports(Format::Pdf));
        assert_eq!(e.name(), "rtf");
    }

    #[test]
    fn rtf_produces_paragraph_sections() {
        let mut s = sink();
        let path = std::path::Path::new("memo.rtf");
        let bytes = b"{\\rtf1\\ansi First line.\\par Second line.\\par}";
        let input = Input {
            path,
            format: Format::Rtf,
            bytes,
            bytes_truncated: false,
            size: bytes.len() as u64,
            mtime: 0,
            fingerprint: "test",
        };
        RtfExtractor.extract(&input, &mut s).unwrap();
        assert!(s.sections.len() >= 2, "{:?}", s.sections);
        assert!(s.text.contains("First line."));
        assert!(s.text.contains("Second line."));
    }

    // -- docx / odt --------------------------------------------------------

    #[cfg(feature = "format-documents")]
    mod ooxml {
        use super::*;

        #[test]
        fn docx_paragraphs_and_headings() {
            let xml = br#"<?xml version="1.0"?>
                <w:document xmlns:w="w">
                  <w:body>
                    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
                         <w:r><w:t>Chapter One</w:t></w:r></w:p>
                    <w:p><w:r><w:t>First body paragraph.</w:t></w:r></w:p>
                    <w:p><w:r><w:t>Second</w:t></w:r><w:r><w:t xml:space="preserve"> body</w:t></w:r></w:p>
                  </w:body>
                </w:document>"#;
            let mut s = sink();
            extract_docx_xml(xml, &mut s).unwrap();
            let headings: Vec<_> = s
                .sections
                .iter()
                .filter(|x| x.kind == SectionKind::Heading)
                .collect();
            assert_eq!(headings.len(), 1, "{:?}", s.sections);
            assert_eq!(headings[0].title.as_deref(), Some("Chapter One"));
            assert_eq!(headings[0].level, Some(1));
            assert!(s.text.contains("First body paragraph."), "{}", s.text);
            assert!(s.text.contains("Second body"), "runs merged: {}", s.text);
        }

        #[test]
        fn docx_tables_and_styles_are_not_executed() {
            let xml = br#"<?xml version="1.0"?>
                <w:document xmlns:w="w">
                  <w:body>
                    <w:p><w:r><w:t>before</w:t></w:r></w:p>
                    <w:tbl>
                      <w:tr><w:tc><w:p><w:r><w:t>cell one</w:t></w:r></w:p></w:tc></w:tr>
                    </w:tbl>
                  </w:body>
                </w:document>"#;
            let mut s = sink();
            extract_docx_xml(xml, &mut s).unwrap();
            assert!(s.text.contains("before"));
            assert!(s.text.contains("cell one"), "{}", s.text);
        }

        #[test]
        fn docx_heading_styles_are_parsed() {
            assert_eq!(parse_heading_style("Heading1"), Some(1));
            assert_eq!(parse_heading_style("heading6"), Some(6));
            assert_eq!(parse_heading_style("Normal"), None);
            assert_eq!(parse_heading_style("Heading99"), None);
        }

        #[test]
        fn docx_entities_are_decoded() {
            let xml = br#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Tom &amp; Jerry &lt;3</w:t></w:r></w:p></w:body></w:document>"#;
            let mut s = sink();
            extract_docx_xml(xml, &mut s).unwrap();
            assert!(s.text.contains("Tom & Jerry <3"), "{}", s.text);
        }

        #[test]
        fn odt_headings_and_paragraphs() {
            let xml = br#"<?xml version="1.0"?>
                <office:document-content
                    xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
                    xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
                    xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0">
                  <office:body><office:text>
                    <text:h text:outline-level="2">Report Title</text:h>
                    <text:p><text:span>Body sentence one.</text:span></text:p>
                    <text:p>Plain paragraph.</text:p>
                    <table:table>
                      <table:table-row>
                        <table:table-cell><text:p>Cell A</text:p></table:table-cell>
                      </table:table-row>
                    </table:table>
                  </office:text></office:body>
                </office:document-content>"#;
            let mut s = sink();
            extract_odt_xml(xml, &mut s).unwrap();
            let heading = s
                .sections
                .iter()
                .find(|x| x.kind == SectionKind::Heading)
                .expect("heading section");
            assert_eq!(heading.title.as_deref(), Some("Report Title"));
            assert_eq!(heading.level, Some(2));
            assert!(s.text.contains("Body sentence one."), "{}", s.text);
            assert!(s.text.contains("Plain paragraph."), "{}", s.text);
            assert!(s.text.contains("Cell A"), "{}", s.text);
        }

        #[test]
        fn malformed_xml_keeps_what_it_read() {
            let xml = br#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>kept text</w:t></w:r></w:p><w:p><<<>>"#;
            let mut s = sink();
            // Must not panic; a parse error is a partial result, not a crash.
            let _ = extract_docx_xml(xml, &mut s);
        }

        #[test]
        fn empty_documents_produce_no_sections() {
            let mut s = sink();
            extract_docx_xml(
                br#"<w:document xmlns:w="w"><w:body></w:body></w:document>"#,
                &mut s,
            )
            .unwrap();
            extract_odt_xml(br#"<office:document-content/>"#, &mut s).unwrap();
            assert!(s.sections.is_empty());
        }

        #[test]
        fn ooxml_extractor_claims_only_containers() {
            let e = OoxmlExtractor;
            assert!(e.supports(Format::Docx));
            assert!(e.supports(Format::Odt));
            assert!(!e.supports(Format::Xlsx));
        }
    }

    // -- pdf ---------------------------------------------------------------

    #[cfg(feature = "format-pdf")]
    mod pdf {
        use super::*;

        #[test]
        fn pdf_extractor_claims_only_pdf() {
            let e = PdfExtractor;
            assert!(e.supports(Format::Pdf));
            assert!(!e.supports(Format::Docx));
            assert_eq!(e.name(), "pdf");
        }

        #[test]
        fn unreadable_pdf_warns_instead_of_erroring() {
            let mut s = sink();
            let bytes = b"%PDF-1.7\nthis is not a real pdf body";
            let path = std::path::Path::new("broken.pdf");
            let input = Input {
                path,
                format: Format::Pdf,
                bytes,
                bytes_truncated: false,
                size: bytes.len() as u64,
                mtime: 0,
                fingerprint: "test",
            };
            PdfExtractor.extract(&input, &mut s).unwrap();
            assert!(s.sections.is_empty());
            assert!(!s.warnings.is_empty(), "must explain the failure");
        }

        /// Builds a minimal but structurally valid PDF with one Helvetica text
        /// line per page, so the page accounting is exercised end to end.
        fn build_pdf(pages: &[&str]) -> Vec<u8> {
            fn obj(id: usize, body: &str) -> String {
                format!("{id} 0 obj\n{body}\nendobj\n")
            }

            let mut out = String::from("%PDF-1.4\n");
            let mut offsets: Vec<usize> = Vec::new();

            // 1 catalog, 2 page tree, 3 font, then per page: page + contents.
            offsets.push(out.len());
            out.push_str(&obj(1, "<< /Type /Catalog /Pages 2 0 R >>"));
            offsets.push(out.len());
            out.push_str(&obj(
                2,
                &format!(
                    "<< /Type /Pages /Kids [{}] /Count {} >>",
                    // Objects 1-3 are the catalog, page tree and font, so the
                    // first page object is 4.
                    (0..pages.len())
                        .map(|i| format!("{} 0 R", 4 + i * 2))
                        .collect::<Vec<_>>()
                        .join(" "),
                    pages.len()
                ),
            ));
            offsets.push(out.len());
            out.push_str(&obj(
                3,
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
            ));

            for (i, text) in pages.iter().enumerate() {
                let page_id = 4 + i * 2;
                let content_id = page_id + 1;
                let content = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
                offsets.push(out.len());
                out.push_str(&obj(
                    page_id,
                    &format!(
                        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                         /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
                    ),
                ));
                offsets.push(out.len());
                out.push_str(&obj(
                    content_id,
                    &format!(
                        "<< /Length {} >>\nstream\n{content}\nendstream",
                        content.len()
                    ),
                ));
            }

            let xref_at = out.len();
            out.push_str(&format!("xref\n0 {}\n", offsets.len() + 1));
            out.push_str("0000000000 65535 f \n");
            for offset in &offsets {
                out.push_str(&format!("{offset:010} 00000 n \n"));
            }
            out.push_str(&format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
                offsets.len() + 1
            ));
            out.into_bytes()
        }

        #[test]
        fn pdf_pages_become_located_sections() {
            let mut s = sink();
            let bytes = build_pdf(&["Page one text.", "Page two text."]);
            let path = std::path::Path::new("doc.pdf");
            let input = Input {
                path,
                format: Format::Pdf,
                bytes: &bytes,
                bytes_truncated: false,
                size: bytes.len() as u64,
                mtime: 0,
                fingerprint: "test",
            };
            PdfExtractor.extract(&input, &mut s).unwrap();

            let pages: Vec<_> = s
                .sections
                .iter()
                .filter(|x| x.kind == SectionKind::Page)
                .collect();
            assert_eq!(pages.len(), 2, "{:?}", s.sections);
            assert_eq!(pages[0].locator, Locator::Page { page: 1 });
            assert_eq!(pages[0].locator.label(), "p1");
            assert_eq!(pages[1].locator.label(), "p2");
            assert!(s.text.contains("Page one text."), "{}", s.text);
            assert!(s.text.contains("Page two text."), "{}", s.text);
        }

        #[test]
        fn pdf_page_count_is_not_invented() {
            let mut s = sink();
            let bytes = build_pdf(&["only page"]);
            let path = std::path::Path::new("one.pdf");
            let input = Input {
                path,
                format: Format::Pdf,
                bytes: &bytes,
                bytes_truncated: false,
                size: bytes.len() as u64,
                mtime: 0,
                fingerprint: "test",
            };
            PdfExtractor.extract(&input, &mut s).unwrap();
            assert_eq!(
                s.sections
                    .iter()
                    .filter(|x| x.kind == SectionKind::Page)
                    .count(),
                1
            );
        }
    }
}
