//! Universal file intelligence: a single document model plus per-format
//! extractors behind one interface.
//!
//! Design rules, in order of importance:
//!
//! 1. **Originals are never modified** - every extractor only reads.
//! 2. **Everything extracted is untrusted data.** No macro, script or embedded
//!    active content is ever executed; XML entities are not resolved beyond the
//!    five predefined ones; archive entry names are validated before use.
//! 3. **Bounded work.** File size, output size, section count, archive entry
//!    count, expansion ratio and wall-clock time are all capped by
//!    [`ExtractionLimits`], and every loop ticks a [`Deadline`].
//! 4. **Partial results beat failure.** A malformed row, sheet or paragraph
//!    produces a warning and whatever text could be recovered, not an error.
//!    Only unusable input (unsupported format, unreadable file, blown limit)
//!    is an error.
//!
//! Parser crates are feature-gated (`format-structured`, `format-tabular`,
//! `format-documents`, `format-pdf`); text/Markdown/HTML/code/RTF extractors are
//! dependency-free and always compiled.

pub mod code;
pub mod document;
pub mod structured;
pub mod tabular;
pub mod text;
pub mod zipcontainer;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Detected document format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Text,
    Markdown,
    Html,
    Json,
    Jsonl,
    Yaml,
    Toml,
    Xml,
    Csv,
    Xlsx,
    Ods,
    Pdf,
    Docx,
    Odt,
    Rtf,
    /// Source code; the language lives in [`Document::language`].
    Code,
    /// No parser claims this input.
    Unsupported,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Text => "text",
            Format::Markdown => "markdown",
            Format::Html => "html",
            Format::Json => "json",
            Format::Jsonl => "jsonl",
            Format::Yaml => "yaml",
            Format::Toml => "toml",
            Format::Xml => "xml",
            Format::Csv => "csv",
            Format::Xlsx => "xlsx",
            Format::Ods => "ods",
            Format::Pdf => "pdf",
            Format::Docx => "docx",
            Format::Odt => "odt",
            Format::Rtf => "rtf",
            Format::Code => "code",
            Format::Unsupported => "unsupported",
        }
    }

    /// Container formats are zip (or pdf) files whose entries are parsed.
    pub fn is_container(self) -> bool {
        matches!(
            self,
            Format::Xlsx | Format::Ods | Format::Docx | Format::Odt | Format::Pdf
        )
    }

    /// Maps a file extension to a format. `None` means "try the sniffers".
    pub fn from_extension(ext: &str) -> Option<Format> {
        let ext = ext.to_ascii_lowercase();
        Some(match ext.as_str() {
            "txt" | "text" | "log" | "ini" | "cfg" | "conf" | "env" => Format::Text,
            "md" | "markdown" | "mdown" | "rst" => Format::Markdown,
            "html" | "htm" | "xhtml" => Format::Html,
            "json" => Format::Json,
            "jsonl" | "ndjson" => Format::Jsonl,
            "yaml" | "yml" => Format::Yaml,
            "toml" => Format::Toml,
            "xml" | "svg" | "plist" | "xsl" | "xsd" => Format::Xml,
            "csv" | "tsv" => Format::Csv,
            "xlsx" | "xlsm" => Format::Xlsx,
            "ods" => Format::Ods,
            "pdf" => Format::Pdf,
            "docx" | "docm" => Format::Docx,
            "odt" => Format::Odt,
            "rtf" => Format::Rtf,
            _ => return None,
        })
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Structural location of a section inside its source, when the format knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locator {
    /// No location information (plain prose, HTML body, ...).
    None,
    /// 1-based inclusive line range in the extracted text's source.
    Lines { start: usize, end: usize },
    /// 1-based page number (PDF).
    Page { page: usize },
    /// Sheet name plus 1-based inclusive row range (XLSX/ODS/CSV).
    Sheet {
        sheet: String,
        start_row: usize,
        end_row: usize,
    },
    /// 1-based row range in a row-oriented source (CSV, JSONL).
    Rows { start: usize, end: usize },
}

impl Default for Locator {
    fn default() -> Self {
        Locator::None
    }
}

impl Locator {
    /// Compact human/machine label used in tool payloads and source references.
    pub fn label(&self) -> String {
        match self {
            Locator::None => String::new(),
            Locator::Lines { start, end } => format!("L{start}-{end}"),
            Locator::Page { page } => format!("p{page}"),
            Locator::Sheet {
                sheet,
                start_row,
                end_row,
            } => format!("{sheet}!R{start_row}-{end_row}"),
            Locator::Rows { start, end } => format!("R{start}-{end}"),
        }
    }
}

/// What kind of structural element a section represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Title,
    Heading,
    Paragraph,
    ListItem,
    TableRow,
    CodeBlock,
    KeyValues,
    Sheet,
    Page,
    Block,
}

/// One structural element of a document. Sections are what the chunker splits
/// and what source references point at, so they carry their own location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub kind: SectionKind,
    /// Heading text / sheet name / table name; `None` for unlabelled prose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Heading level (1-6) when the format has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    pub locator: Locator,
    pub text: String,
}

/// Everything one extraction produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// Path as the caller supplied it (usually workspace-relative).
    pub path: String,
    /// Size of the source file in bytes.
    pub size: u64,
    /// Source `mtime` in unix seconds, when available.
    #[serde(default)]
    pub mtime: i64,
    pub format: Format,
    /// Source language for [`Format::Code`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Extracted text, bounded by `max_text_chars`.
    pub text: String,
    pub sections: Vec<Section>,
    /// Non-fatal problems: skipped rows, unsupported features, caps hit.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// True when a cap (file size, text size, section count, row count) stopped
    /// extraction early. The source file itself was never modified.
    #[serde(default)]
    pub truncated: bool,
    /// Content fingerprint used by the incremental index.
    pub fingerprint: String,
}

impl Document {
    /// First heading/title text, used as an index display name.
    pub fn title(&self) -> Option<&str> {
        self.sections
            .iter()
            .find(|s| matches!(s.kind, SectionKind::Title | SectionKind::Heading))
            .and_then(|s| s.title.as_deref())
            .map(str::trim)
            .filter(|t| !t.is_empty())
    }

    pub fn chars(&self) -> usize {
        self.text.chars().count()
    }
}

/// Hard ceilings applied to every extraction. All fields are opt-in knobs; the
/// defaults are sized so a normal source tree indexes in bounded time and RAM.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractionLimits {
    /// Bytes read from a single source file. Larger files are read to this cap
    /// and marked truncated; they are never fully materialised.
    pub max_file_bytes: u64,
    /// Characters of extracted text kept per document.
    pub max_text_chars: usize,
    /// Sections kept per document.
    pub max_sections: usize,
    /// Rows kept across all tables in one document.
    pub max_rows: usize,
    /// Entries read from one archive (xlsx/ods/docx/odt).
    pub max_archive_entries: usize,
    /// Total uncompressed bytes allowed to be read out of one archive.
    pub max_expanded_bytes: u64,
    /// Nesting depth for archives inside archives (0 = no nesting allowed).
    pub max_nesting: usize,
    /// Wall-clock budget for one extraction.
    pub max_duration: Duration,
    /// XML elements visited before an XML document is cut off.
    pub max_xml_elements: usize,
}

impl Default for ExtractionLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 8 * 1024 * 1024,
            max_text_chars: 400_000,
            max_sections: 1_000,
            max_rows: 50_000,
            max_archive_entries: 32,
            max_expanded_bytes: 64 * 1024 * 1024,
            max_nesting: 0,
            max_duration: Duration::from_secs(10),
            max_xml_elements: 200_000,
        }
    }
}

impl ExtractionLimits {
    /// Tighter limits for on-demand tool calls (bounded latency).
    pub fn for_tool(max_file_bytes: usize) -> Self {
        Self {
            max_file_bytes: max_file_bytes.max(1024) as u64,
            max_text_chars: 200_000,
            max_sections: 400,
            max_rows: 20_000,
            max_duration: Duration::from_secs(5),
            ..Self::default()
        }
    }
}

/// Why an extraction failed. Codes are stable and echoed to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractError {
    /// No extractor claims this format.
    Unsupported(String),
    /// The file (or archive expansion) is over a configured limit.
    TooLarge(String),
    /// The wall-clock budget ran out.
    Timeout(String),
    /// The bytes are not valid for the claimed format.
    Malformed(String),
    /// Filesystem failure.
    Io(String),
    /// A path or archive entry tried to escape the workspace/root.
    PathEscape(String),
    /// A structural cap (sections/rows/entries) was hit and the result would be
    /// unusable rather than partial.
    Limit(String),
}

impl ExtractError {
    pub fn code(&self) -> &'static str {
        match self {
            ExtractError::Unsupported(_) => "unsupported",
            ExtractError::TooLarge(_) => "too_large",
            ExtractError::Timeout(_) => "timeout",
            ExtractError::Malformed(_) => "malformed",
            ExtractError::Io(_) => "io",
            ExtractError::PathEscape(_) => "path_escape",
            ExtractError::Limit(_) => "limit",
        }
    }

    fn unsupported(path: &Path, ext: &str) -> Self {
        ExtractError::Unsupported(format!(
            "{}: no extractor for `.{ext}` - supported: txt, md, html, json, jsonl, yaml, toml, \
             xml, csv, xlsx, ods, pdf, docx, odt, rtf and source code",
            path.display()
        ))
    }
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (label, detail) = match self {
            ExtractError::Unsupported(d) => ("unsupported format", d),
            ExtractError::TooLarge(d) => ("file too large", d),
            ExtractError::Timeout(d) => ("extraction timed out", d),
            ExtractError::Malformed(d) => ("malformed file", d),
            ExtractError::Io(d) => ("read failed", d),
            ExtractError::PathEscape(d) => ("refused: path escape", d),
            ExtractError::Limit(d) => ("extraction limit", d),
        };
        write!(f, "{label}: {detail}")
    }
}

impl std::error::Error for ExtractError {}

impl From<std::io::Error> for ExtractError {
    fn from(e: std::io::Error) -> Self {
        ExtractError::Io(e.to_string())
    }
}

/// Wall-clock guard shared by every extractor loop.
#[derive(Debug, Clone)]
pub struct Deadline {
    start: Instant,
    budget: Duration,
}

impl Deadline {
    pub fn new(budget: Duration) -> Self {
        Self {
            start: Instant::now(),
            budget,
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    pub fn expired(&self) -> bool {
        self.start.elapsed() >= self.budget
    }
}

/// Inputs handed to an extractor. Bytes are already capped to
/// `limits.max_file_bytes`.
pub struct Input<'a> {
    pub path: &'a Path,
    pub format: Format,
    pub bytes: &'a [u8],
    /// True when the source is larger than the read cap.
    pub bytes_truncated: bool,
    pub size: u64,
    pub mtime: i64,
    pub fingerprint: &'a str,
}

/// Accumulates extracted text and sections while enforcing every cap, so an
/// extractor can simply `push` without re-checking limits itself.
pub struct Sink {
    pub text: String,
    pub sections: Vec<Section>,
    pub warnings: Vec<String>,
    truncated: bool,
    rows: usize,
    deadline: Deadline,
    limits: ExtractionLimits,
}

impl Sink {
    pub fn new(limits: ExtractionLimits) -> Self {
        Self {
            text: String::new(),
            sections: Vec::new(),
            warnings: Vec::new(),
            truncated: false,
            rows: 0,
            deadline: Deadline::new(limits.max_duration),
            limits,
        }
    }

    /// Call from every extraction loop: returns `Err(Timeout)` when the budget
    /// is gone.
    pub fn tick(&mut self) -> Result<(), ExtractError> {
        if self.deadline.expired() {
            return Err(ExtractError::Timeout(format!(
                "{}: exceeded {}ms",
                self.limits.max_duration.as_millis().max(1),
                self.limits.max_duration.as_millis()
            )));
        }
        Ok(())
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        if self.warnings.len() < 32 && !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }

    pub fn limits(&self) -> &ExtractionLimits {
        &self.limits
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Charges `count` table rows against the row budget; `false` when spent.
    pub fn charge_rows(&mut self, count: usize) -> bool {
        if self.rows + count > self.limits.max_rows {
            self.truncated = true;
            return false;
        }
        self.rows += count;
        true
    }

    /// Appends raw extracted text. Returns `false` once the character cap is
    /// reached (subsequent calls are no-ops and the document is marked
    /// truncated).
    pub fn push_text(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return true;
        }
        let room = self.limits.max_text_chars.saturating_sub(self.text.len());
        if room == 0 {
            self.truncated = true;
            return false;
        }
        if text.len() <= room {
            self.text.push_str(text);
            return true;
        }
        self.text.reserve(room);
        self.text.push_str(&text[..room]);
        self.truncated = true;
        false
    }

    /// Appends a line plus its newline.
    pub fn push_line(&mut self, line: &str) -> bool {
        if !self.push_text(line) {
            return false;
        }
        self.push_text("\n")
    }

    /// Records one structural section. Section text also flows into the flat
    /// [`Sink::text`] so keyword search sees it either way.
    pub fn push_section(&mut self, mut section: Section) -> bool {
        if self.sections.len() >= self.limits.max_sections {
            self.truncated = true;
            return false;
        }
        if section.text.len() > self.limits.max_text_chars {
            section.text.truncate(self.limits.max_text_chars);
            self.truncated = true;
        }
        let ok = self.push_text(&section.text) && self.push_text("\n");
        self.sections.push(section);
        ok
    }

    /// Marks the document as partially extracted without an error.
    pub fn mark_truncated(&mut self, why: impl Into<String>) {
        self.truncated = true;
        self.warn(why);
    }
}

/// The common interface every format-specific extractor implements.
///
/// Extractors receive capped bytes and push results into a [`Sink`]; they never
/// touch the filesystem directly, which keeps path handling and limits in one
/// audited place.
pub trait Extractor: Send + Sync {
    /// Stable extractor name for diagnostics.
    fn name(&self) -> &'static str;
    /// Formats this extractor claims.
    fn supports(&self, format: Format) -> bool;
    /// Extract into `sink`. `Err` means "unusable"; a successful return with
    /// warnings/truncated flags means "partial result".
    fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError>;
}

/// Registry of extractors, consulted in registration order.
#[derive(Clone)]
pub struct ExtractorRegistry {
    extractors: std::sync::Arc<Vec<std::sync::Arc<dyn Extractor>>>,
}

impl Default for ExtractorRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl ExtractorRegistry {
    /// Every extractor compiled into this build (feature-gated ones included).
    pub fn builtin() -> Self {
        let mut extractors: Vec<std::sync::Arc<dyn Extractor>> = Vec::new();
        // Containers first: they must claim their formats before any generic
        // sniffing could mistake a zip for something else.
        #[cfg(feature = "format-tabular")]
        extractors.push(std::sync::Arc::new(tabular::SpreadsheetExtractor));
        #[cfg(feature = "format-documents")]
        extractors.push(std::sync::Arc::new(document::OoxmlExtractor));
        #[cfg(feature = "format-pdf")]
        extractors.push(std::sync::Arc::new(document::PdfExtractor));
        extractors.push(std::sync::Arc::new(document::RtfExtractor));
        #[cfg(feature = "format-structured")]
        extractors.push(std::sync::Arc::new(structured::StructuredExtractor));
        extractors.push(std::sync::Arc::new(structured::JsonExtractor));
        #[cfg(feature = "format-tabular")]
        extractors.push(std::sync::Arc::new(tabular::CsvExtractor));
        extractors.push(std::sync::Arc::new(text::HtmlExtractor));
        extractors.push(std::sync::Arc::new(text::MarkdownExtractor));
        extractors.push(std::sync::Arc::new(code::CodeExtractor));
        extractors.push(std::sync::Arc::new(text::TextExtractor));
        Self {
            extractors: std::sync::Arc::new(extractors),
        }
    }

    pub fn len(&self) -> usize {
        self.extractors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.extractors.is_empty()
    }

    fn for_format(&self, format: Format) -> Option<&dyn Extractor> {
        self.extractors
            .iter()
            .find(|e| e.supports(format))
            .map(|e| e.as_ref())
    }

    /// Detects the format of `path` (extension first, then content sniffing).
    pub fn detect(&self, path: &Path, head: &[u8]) -> (Format, Option<String>) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        // Magic bytes win when they are unambiguous: a PDF named `.txt` is
        // still a PDF, and a zip named `.bin` is still a container.
        if head.starts_with(b"%PDF-") {
            return (Format::Pdf, None);
        }
        if head.starts_with(b"PK\x03\x04") {
            if let Some(fmt) = zipcontainer::format_from_entry_names(&ext) {
                return (fmt, None);
            }
        }
        if looks_binary(head) {
            return (Format::Unsupported, None);
        }

        if let Some(fmt) = Format::from_extension(&ext) {
            return (fmt, code::language_for_extension(&ext));
        }

        // Source code is a recognised extension too, but it is not a `Format`
        // mapping of its own: `Code` carries the language separately. Checked
        // before content sniffing so a Python file opening with a dict literal
        // is not mistaken for JSON.
        if code::is_code_extension(&ext) {
            return (Format::Code, code::language_for_extension(&ext));
        }

        // A present-but-unrecognised extension is not licence to guess: the
        // user named the file and the name means nothing to us. Only a file
        // with no extension at all falls through to content sniffing, which is
        // what makes extension-less files like `LICENSE` work.
        if !ext.is_empty() {
            return (Format::Unsupported, None);
        }

        // No usable extension: sniff the common text shapes.
        let probe = String::from_utf8_lossy(&head[..head.len().min(512)]);
        let lower = probe.trim_start().to_ascii_lowercase();
        if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
            return (Format::Html, None);
        }
        if lower.starts_with("<?xml") {
            return (Format::Xml, None);
        }
        if probe.trim_start().starts_with('{') || probe.trim_start().starts_with('[') {
            return (Format::Json, None);
        }
        if self.for_format(Format::Text).is_some() {
            return (Format::Text, None);
        }
        (Format::Unsupported, None)
    }

    /// Reads, detects and extracts `path` in one call.
    pub fn extract_path(
        &self,
        path: &Path,
        limits: &ExtractionLimits,
    ) -> Result<Document, ExtractError> {
        let file = std::fs::File::open(path)
            .map_err(|e| ExtractError::Io(format!("{}: {e}", path.display())))?;
        let meta = file.metadata()?;
        let size = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Stream at most `max_file_bytes`: a 4 GB file becomes a capped read,
        // never an allocation.
        let cap = limits.max_file_bytes.min(size) as usize;
        let mut bytes = Vec::with_capacity(cap.min(1024 * 1024));
        let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
        {
            use std::io::Read;
            reader
                .by_ref()
                .take(limits.max_file_bytes)
                .read_to_end(&mut bytes)?;
        }
        let bytes_truncated = (bytes.len() as u64) < size;
        let fingerprint = fingerprint_parts(size, mtime, &bytes);

        let (format, language) = self.detect(path, &bytes);
        let extractor = self.for_format(format).ok_or_else(|| {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if format == Format::Unsupported && ext.is_empty() {
                // Nothing named the file and nothing in it was recognisable.
                ExtractError::Unsupported(format!(
                    "{}: binary or unknown content ({} bytes); no extractor applies",
                    path.display(),
                    size
                ))
            } else {
                // Naming the extension gets the user the list of what *is*
                // supported, which is the only actionable part of the message.
                ExtractError::unsupported(path, ext)
            }
        })?;

        let input = Input {
            path,
            format,
            bytes: &bytes,
            bytes_truncated,
            size,
            mtime,
            fingerprint: &fingerprint,
        };
        let mut sink = Sink::new(limits.clone());
        if bytes_truncated {
            sink.mark_truncated(format!(
                "source is {size} bytes, read cap is {} bytes",
                limits.max_file_bytes
            ));
        }
        extractor.extract(&input, &mut sink)?;

        Ok(Document {
            path: path.display().to_string(),
            size,
            mtime,
            format,
            language,
            text: sink.text.trim_end().to_string(),
            sections: sink.sections,
            warnings: sink.warnings,
            truncated: sink.truncated,
            fingerprint,
        })
    }
}

/// Convenience: extract one file with the built-in registry.
pub fn extract_path(path: &Path, limits: &ExtractionLimits) -> Result<Document, ExtractError> {
    ExtractorRegistry::builtin().extract_path(path, limits)
}

/// Content fingerprint of a file: exact for files at or below 8 MiB
/// (size + mtime + full-content FNV-1a), sampled above that (size + mtime +
/// first 8 MiB) where reading the remainder would defeat the purpose of a cap.
pub fn fingerprint_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let meta = std::fs::metadata(path)?;
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
    let cap = size.min(8 * 1024 * 1024) as usize;
    let mut buf = Vec::with_capacity(cap.min(1024 * 1024));
    reader.by_ref().take(cap as u64).read_to_end(&mut buf)?;
    Ok(fingerprint_parts(size, mtime, &buf))
}

/// Fingerprint of in-memory bytes (size derived from the slice).
pub fn fingerprint_bytes(bytes: &[u8]) -> String {
    fingerprint_parts(bytes.len() as u64, 0, bytes)
}

fn fingerprint_parts(size: u64, mtime: i64, content: &[u8]) -> String {
    let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
    for word in size.to_le_bytes().iter().chain(mtime.to_le_bytes().iter()) {
        acc ^= u64::from(*word);
        acc = acc.wrapping_mul(0x1000_0000_01b3);
    }
    for byte in content {
        acc ^= u64::from(*byte);
        acc = acc.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{acc:016x}")
}

/// Heuristic binary check over the first 8 KiB: NUL bytes or a high density of
/// other control characters mean "not a text format".
pub fn looks_binary(head: &[u8]) -> bool {
    let probe = &head[..head.len().min(8192)];
    if probe.is_empty() {
        return false;
    }
    if probe.contains(&0) {
        return true;
    }
    let odd = probe
        .iter()
        .filter(|b| **b < 9 || (**b > 13 && **b < 32))
        .count();
    odd * 10 > probe.len()
}

/// Splits text into paragraph sections with line locators. Shared by the text
/// and code extractors so prose always looks the same downstream.
pub(crate) fn paragraph_sections(text: &str, max_sections: usize) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut start_line = 1usize;
    let mut lines_in_block = 0usize;
    let mut block = String::new();
    let mut line_no = 0usize;

    for line in text.split_inclusive('\n') {
        line_no += 1;
        if line.trim().is_empty() {
            if !block.trim().is_empty() {
                sections.push(Section {
                    kind: SectionKind::Paragraph,
                    title: None,
                    level: None,
                    locator: Locator::Lines {
                        start: start_line,
                        end: line_no.saturating_sub(1).max(start_line),
                    },
                    text: block.trim_end().to_string(),
                });
                block.clear();
                lines_in_block = 0;
                if sections.len() >= max_sections {
                    return sections;
                }
            }
            start_line = line_no + 1;
            continue;
        }
        if lines_in_block == 0 {
            start_line = line_no;
        }
        lines_in_block += 1;
        block.push_str(line);
    }
    if !block.trim().is_empty() && sections.len() < max_sections {
        sections.push(Section {
            kind: SectionKind::Paragraph,
            title: None,
            level: None,
            locator: Locator::Lines {
                start: start_line,
                end: line_no.max(start_line),
            },
            text: block.trim_end().to_string(),
        });
    }
    sections
}

/// Path used in tool payloads: workspace-relative with forward slashes.
pub fn display_path(path: &Path, root: Option<&Path>) -> String {
    let rel = root.and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

/// Workspace-relative path of an extracted document, computed once.
pub fn document_path(path: &Path, root: Option<&Path>) -> String {
    display_path(path, root)
}

/// Guards used by callers that resolve user-supplied paths before extraction.
pub fn ensure_no_escape(root: &Path, candidate: &Path) -> Result<PathBuf, ExtractError> {
    let normalized = crate::tools::file_tools::normalize(candidate);
    if !normalized.starts_with(root) {
        return Err(ExtractError::PathEscape(format!(
            "{} escapes the workspace",
            candidate.display()
        )));
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nanomite-extract-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn format_detection_prefers_magic_bytes() {
        let reg = ExtractorRegistry::builtin();
        let (fmt, _) = reg.detect(Path::new("notes.txt"), b"%PDF-1.7\n");
        assert_eq!(fmt, Format::Pdf);
        // A zip with no recognisable document entries is just a zip: claiming
        // a spreadsheet format from the magic bytes alone would be a guess.
        let (fmt, _) = reg.detect(Path::new("data.bin"), b"PK\x03\x04....");
        assert_eq!(fmt, Format::Unsupported);
        let (fmt, lang) = reg.detect(Path::new("main.rs"), b"fn main() {}\n");
        assert_eq!(fmt, Format::Code);
        assert_eq!(lang.as_deref(), Some("rust"));
        let (fmt, _) = reg.detect(Path::new("index.html"), b"<!doctype html><p>hi");
        assert_eq!(fmt, Format::Html);
        let (fmt, _) = reg.detect(Path::new("mystery"), b"{\"a\":1}");
        assert_eq!(fmt, Format::Json);
    }

    #[test]
    fn binary_content_is_unsupported_not_a_crash() {
        let reg = ExtractorRegistry::builtin();
        let (fmt, _) = reg.detect(Path::new("blob.txt"), b"\x00\x01\x02\x03binary");
        assert_eq!(fmt, Format::Unsupported);
        let path = scratch("blob.txt", b"\x00\x01\x02\x03binary");
        let err = reg
            .extract_path(&path, &ExtractionLimits::default())
            .unwrap_err();
        assert_eq!(err.code(), "unsupported", "{err}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unsupported_extension_reports_a_useful_error() {
        let reg = ExtractorRegistry::builtin();
        let path = scratch("thing.xyz", b"hello world");
        let err = reg
            .extract_path(&path, &ExtractionLimits::default())
            .unwrap_err();
        assert_eq!(err.code(), "unsupported");
        assert!(err.to_string().contains("supported:"), "{err}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn oversized_files_are_capped_and_marked_truncated() {
        let path = scratch("big.txt", &vec![b'a'; 4096]);
        let limits = ExtractionLimits {
            max_file_bytes: 1024,
            ..ExtractionLimits::default()
        };
        let doc = extract_path(&path, &limits).expect("extract");
        assert!(doc.truncated, "cap must mark the document truncated");
        assert!(doc.text.len() <= 1024, "read cap respected");
        assert!(
            doc.warnings.iter().any(|w| w.contains("read cap")),
            "warning explains the cap: {:?}",
            doc.warnings
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn text_cap_stops_output_without_failing() {
        let path = scratch("long.txt", &vec![b'x'; 50_000]);
        let limits = ExtractionLimits {
            max_text_chars: 1_000,
            ..ExtractionLimits::default()
        };
        let doc = extract_path(&path, &limits).expect("extract");
        assert!(doc.truncated);
        assert!(
            doc.text.chars().count() <= 1_000,
            "{} chars",
            doc.text.len()
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn deadlines_stop_runaway_loops() {
        let mut sink = Sink::new(ExtractionLimits {
            max_duration: Duration::from_nanos(1),
            ..ExtractionLimits::default()
        });
        std::thread::sleep(Duration::from_millis(2));
        let err = sink.tick().unwrap_err();
        assert_eq!(err.code(), "timeout");
    }

    #[test]
    fn sink_caps_sections_and_rows() {
        let mut sink = Sink::new(ExtractionLimits {
            max_sections: 2,
            max_rows: 3,
            ..ExtractionLimits::default()
        });
        for i in 0..5 {
            let keep = sink.push_section(Section {
                kind: SectionKind::Paragraph,
                title: None,
                level: None,
                locator: Locator::None,
                text: format!("block {i}"),
            });
            if i >= 2 {
                assert!(!keep);
            }
        }
        assert_eq!(sink.sections.len(), 2);
        assert!(sink.truncated());

        assert!(sink.charge_rows(2));
        assert!(!sink.charge_rows(2));
        assert!(sink.truncated());
    }

    #[test]
    fn fingerprints_change_with_content_and_size() {
        let a = fingerprint_bytes(b"alpha");
        let b = fingerprint_bytes(b"beta");
        assert_ne!(a, b);
        assert_eq!(a, fingerprint_bytes(b"alpha"));
        let path = scratch("fp.txt", b"alpha");
        let on_disk = fingerprint_file(&path).unwrap();
        assert_ne!(on_disk, a, "mtime participates in the fingerprint");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn locator_labels_are_compact() {
        assert_eq!(
            Locator::Sheet {
                sheet: "Sheet1".into(),
                start_row: 2,
                end_row: 9
            }
            .label(),
            "Sheet1!R2-9"
        );
        assert_eq!(Locator::Page { page: 3 }.label(), "p3");
        assert_eq!(Locator::Lines { start: 4, end: 9 }.label(), "L4-9");
    }

    #[test]
    fn original_files_are_never_modified() {
        let path = scratch("keep.txt", b"original content");
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        let doc = extract_path(&path, &ExtractionLimits::default()).unwrap();
        let after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(before, after);
        assert_eq!(doc.text, "original content");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
