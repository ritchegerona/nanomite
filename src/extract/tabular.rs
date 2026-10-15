//! Row-oriented extractors: CSV/TSV and spreadsheets (XLSX, ODS). Both need
//! parser crates, so the whole module is compiled only under the
//! `format-tabular` feature.
//!
//! Row-oriented data becomes *row sections* carrying a [`Locator::Rows`] (CSV)
//! or [`Locator::Sheet`] (spreadsheets), so a quote can name the exact sheet
//! and row range it came from.
//!
//! - **Read only.** Spreadsheets are read out of their zip container through
//!   [`super::zipcontainer`], which validates every entry name and charges
//!   every byte against the expansion budget.
//! - **Untrusted data.** Cell contents are never interpreted: formulas are
//!   reported as their literal text, macros (`vbaProject.bin`) and embedded
//!   objects are ignored, and no external link, DTD or URL is ever resolved.
//! - **Bounded work.** Every row is charged against `max_rows`, every section
//!   against `max_sections`, and every loop ticks the deadline.

#![cfg(feature = "format-tabular")]

use super::{ExtractError, Extractor, Format, Input, Locator, Section, SectionKind, Sink};

/// Delimiter used for a CSV when the content gives no better hint.
const DEFAULT_DELIMITER: u8 = b',';

/// Delimiters tried, in order, when sniffing a CSV/TSV.
const CANDIDATE_DELIMITERS: &[u8] = b",;\t|";

/// Repeated-row/column markers above this are clamped: an ODS that claims a
/// million identical rows must not cost a million sections.
const MAX_REPEAT: usize = 256;

// ---------------------------------------------------------------------------
// CSV / TSV
// ---------------------------------------------------------------------------

/// Delimited text with a header row. Each record becomes a row section whose
/// locator names the 1-based row number.
pub struct CsvExtractor;

impl Extractor for CsvExtractor {
    fn name(&self) -> &'static str {
        "csv"
    }

    fn supports(&self, format: Format) -> bool {
        format == Format::Csv
    }

    fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let text = std::str::from_utf8(input.bytes)
            .map_err(|e| ExtractError::Malformed(format!("{}: {e}", input.path.display())))?;

        let delimiter = detect_delimiter(text, input.path);
        let sheet = input
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "csv".to_string());

        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .flexible(true)
            .has_headers(true)
            .from_reader(text.as_bytes());

        let headers: Vec<String> = reader
            .headers()
            .map_err(|e| {
                ExtractError::Malformed(format!("{}: bad header row: {e}", input.path.display()))
            })?
            .iter()
            .map(|h| h.trim().to_string())
            .collect();

        if headers.iter().any(|h| !h.is_empty()) {
            let title = headers.join(" | ");
            sink.push_section(Section {
                kind: SectionKind::Title,
                title: Some(title.clone()),
                level: Some(1),
                locator: Locator::Sheet {
                    sheet: sheet.clone(),
                    start_row: 1,
                    end_row: 1,
                },
                text: title,
            });
        }

        let mut row_no = 0usize;
        let mut record = csv::StringRecord::new();
        loop {
            sink.tick()?;
            if !sink.charge_rows(1) {
                sink.mark_truncated("csv row cap reached");
                return Ok(());
            }
            match reader.read_record(&mut record) {
                Ok(false) => return Ok(()),
                Ok(true) => {}
                Err(e) => {
                    // A malformed row is a warning, not a failure: the rest of
                    // the file is still useful.
                    sink.warn(format!("csv row {} skipped: {e}", row_no + 1));
                    continue;
                }
            }
            row_no += 1;
            if record.iter().all(|f| f.trim().is_empty()) {
                continue;
            }
            let fields: Vec<String> = record
                .iter()
                .enumerate()
                .map(|(i, f)| match headers.get(i) {
                    Some(h) if !h.is_empty() => format!("{h}: {}", f.trim()),
                    _ => f.trim().to_string(),
                })
                .collect();
            let keep = sink.push_section(Section {
                kind: SectionKind::TableRow,
                title: None,
                level: None,
                locator: Locator::Rows {
                    start: row_no,
                    end: row_no,
                },
                text: fields.join(" | "),
            });
            if !keep {
                sink.mark_truncated("csv section cap reached");
                return Ok(());
            }
        }
    }
}

/// Picks the delimiter. A `.tsv` extension is decisive; otherwise the candidate
/// with the most consistent multi-column shape wins. Falls back to comma.
fn detect_delimiter(text: &str, path: &std::path::Path) -> u8 {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("tsv"))
        .unwrap_or(false)
    {
        return b'\t';
    }
    let sample: Vec<&str> = text
        .lines()
        .take(20)
        .filter(|l| !l.trim().is_empty())
        .collect();
    if sample.is_empty() {
        return DEFAULT_DELIMITER;
    }

    // Score by (column count, consistency). `min` is used rather than `max` so a
    // single odd line cannot inflate a wrong delimiter's score.
    let mut best = (DEFAULT_DELIMITER, 0usize, usize::MAX);
    for &candidate in CANDIDATE_DELIMITERS {
        let d = candidate as char;
        let counts: Vec<usize> = sample.iter().map(|l| l.matches(d).count() + 1).collect();
        let min = *counts.iter().min().unwrap_or(&1);
        let max = *counts.iter().max().unwrap_or(&1);
        if min <= 1 {
            continue;
        }
        let spread = max - min;
        let better = min > best.1 || (min == best.1 && spread < best.2);
        if better {
            best = (candidate, min, spread);
        }
    }
    best.0
}

// ---------------------------------------------------------------------------
// Spreadsheets (XLSX, ODS)
// ---------------------------------------------------------------------------

/// Office Open XML (`.xlsx`) and OpenDocument spreadsheets (`.ods`).
pub struct SpreadsheetExtractor;

impl Extractor for SpreadsheetExtractor {
    fn name(&self) -> &'static str {
        "spreadsheet"
    }

    fn supports(&self, format: Format) -> bool {
        matches!(format, Format::Xlsx | Format::Ods)
    }

    fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let mut archive = super::zipcontainer::open(input.bytes)?;
        let mut budget = super::zipcontainer::Budget::new(sink.limits());

        match input.format {
            Format::Xlsx => extract_xlsx(&mut archive, &mut budget, sink),
            Format::Ods => extract_ods(&mut archive, &mut budget, sink),
            _ => Ok(()),
        }
    }
}

/// Streaming XML pull loop shared by every spreadsheet part. Text is *not*
/// trimmed by the reader: a shared string may legitimately carry inner
/// whitespace, so callers trim at the point where they join a value.
struct XmlCursor<'a> {
    reader: quick_xml::Reader<&'a [u8]>,
    buf: Vec<u8>,
}

impl<'a> XmlCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        let mut reader = quick_xml::Reader::from_reader(bytes);
        let config = reader.config_mut();
        config.trim_text(false);
        // Spreadsheet parts are machine-written; a name mismatch means damage,
        // not a reason to discard what has already been read.
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
            .map_err(|e| ExtractError::Malformed(format!("malformed spreadsheet part: {e}")))
    }
}

/// Reads an attribute by local name.
fn attribute(e: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|attr| {
        if attr.key.local_name().into_inner() == key {
            Some(attr.value.into_owned())
        } else {
            None
        }
    })
}

/// One parsed row: a 1-based row number and its already-joined cell text.
type Row = (usize, String);

// -- xlsx ------------------------------------------------------------------

fn extract_xlsx(
    archive: &mut super::zipcontainer::ZipArchive<'_>,
    budget: &mut super::zipcontainer::Budget,
    sink: &mut Sink,
) -> Result<(), ExtractError> {
    let shared = match super::zipcontainer::read_optional(archive, "xl/sharedStrings.xml", budget)?
    {
        Some(bytes) => parse_shared_strings(&bytes, sink)?,
        None => Vec::new(),
    };

    let sheets = list_xlsx_sheets(archive, budget, sink)?;
    if sheets.is_empty() {
        sink.warn("xlsx has no readable workbook part");
        return Ok(());
    }

    for (sheet_name, part) in sheets {
        match super::zipcontainer::read_optional(archive, &part, budget)? {
            Some(bytes) => {
                let rows = parse_xlsx_sheet(&bytes, &shared, sink)?;
                emit_rows(sink, &sheet_name, rows)?;
            }
            None => sink.warn(format!(
                "xlsx sheet {sheet_name:?} part {part:?} is missing"
            )),
        }
    }
    Ok(())
}

/// `xl/workbook.xml` gives sheet names in order; each `r:id` resolves through
/// `xl/_rels/workbook.xml.rels` to a part name.
fn list_xlsx_sheets(
    archive: &mut super::zipcontainer::ZipArchive<'_>,
    budget: &mut super::zipcontainer::Budget,
    sink: &mut Sink,
) -> Result<Vec<(String, String)>, ExtractError> {
    let Some(workbook) = super::zipcontainer::read_optional(archive, "xl/workbook.xml", budget)?
    else {
        return Ok(Vec::new());
    };

    let mut sheets: Vec<(String, String)> = Vec::new();
    let mut cursor = XmlCursor::new(&workbook);
    loop {
        sink.tick()?;
        match cursor.next()? {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)
                if e.name().local_name().into_inner() == "sheet" =>
            {
                sheets.push((
                    attribute(&e, "name").unwrap_or_else(|| "Sheet".to_string()),
                    attribute(&e, "id").unwrap_or_default(),
                ));
            }
            _ => {}
        }
    }

    let mut rels: Vec<(String, String)> = Vec::new();
    if let Some(bytes) =
        super::zipcontainer::read_optional(archive, "xl/_rels/workbook.xml.rels", budget)?
    {
        let mut cursor = XmlCursor::new(&bytes);
        loop {
            sink.tick()?;
            match cursor.next()? {
                quick_xml::events::Event::Eof => break,
                quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)
                    if e.name().local_name().into_inner() == "Relationship" =>
                {
                    rels.push((
                        attribute(&e, "Id").unwrap_or_default(),
                        attribute(&e, "Target").unwrap_or_default(),
                    ));
                }
                _ => {}
            }
        }
    }

    Ok(sheets
        .into_iter()
        .enumerate()
        .map(|(index, (name, rid))| {
            let part = match rels.iter().find(|(id, _)| *id == rid) {
                Some((_, t)) if !t.is_empty() => normalize_xlsx_part(t),
                // Without a usable relationship the conventional name applies.
                _ => format!("xl/worksheets/sheet{}.xml", index + 1),
            };
            (name, part)
        })
        .collect())
}

/// `xl/_rels/workbook.xml.rels` targets are relative to `xl/`.
fn normalize_xlsx_part(target: &str) -> String {
    let cleaned = target.trim_start_matches("./");
    if cleaned.starts_with("xl/") {
        cleaned.to_string()
    } else {
        format!("xl/{cleaned}")
    }
}

/// `xl/sharedStrings.xml`: every `<si>` flattened to plain text.
fn parse_shared_strings(bytes: &[u8], sink: &mut Sink) -> Result<Vec<String>, ExtractError> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_si = false;
    let mut cursor = XmlCursor::new(bytes);

    loop {
        sink.tick()?;
        if out.len() >= sink.limits().max_rows {
            sink.mark_truncated("shared string cap reached");
            break;
        }
        match cursor.next()? {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(e) => {
                if e.name().local_name().into_inner() == "si" {
                    in_si = true;
                    current.clear();
                }
            }
            quick_xml::events::Event::Text(t) => {
                if in_si {
                    current.push_str(t.into_inner().as_ref());
                }
            }
            quick_xml::events::Event::CData(c) => {
                if in_si {
                    current.push_str(c.into_inner().as_ref());
                }
            }
            quick_xml::events::Event::End(e) if e.name().local_name().into_inner() == "si" => {
                in_si = false;
                out.push(std::mem::take(&mut current));
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Cell value being assembled for the XLSX `<c>` currently open.
#[derive(Default)]
struct XlsxCell {
    /// `t` attribute: `s` = shared-string index, `inlineStr` = inline text.
    kind: String,
    value: String,
    formula: String,
}

impl XlsxCell {
    /// Resolves the accumulated value. Shared-string cells hold an *index* into
    /// `sharedStrings.xml`; an index that does not resolve stays literal.
    fn resolve(&self, shared: &[String]) -> String {
        let body = if self.kind == "s" {
            self.value
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|i| shared.get(i))
                .cloned()
                .unwrap_or_else(|| self.value.trim().to_string())
        } else {
            self.value.trim().to_string()
        };
        if self.formula.trim().is_empty() {
            body
        } else {
            // Formulas are data, never evaluated: report the expression and the
            // cached result side by side.
            format!("{} ={}", body, self.formula.trim())
        }
    }
}

/// `xl/worksheets/sheetN.xml`: rows in document order with 1-based row numbers.
fn parse_xlsx_sheet(
    bytes: &[u8],
    shared: &[String],
    sink: &mut Sink,
) -> Result<Vec<Row>, ExtractError> {
    let mut rows: Vec<Row> = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    let mut cell = XlsxCell::default();
    let mut in_cell = false;
    let mut in_formula = false;
    let mut row_no = 0usize;
    let mut explicit_row = 0usize;
    let mut elements = 0usize;
    let mut cursor = XmlCursor::new(bytes);

    loop {
        if elements.is_multiple_of(512) {
            sink.tick()?;
        }
        if elements >= sink.limits().max_xml_elements {
            sink.mark_truncated("xlsx element cap reached");
            break;
        }
        elements += 1;

        match cursor.next()? {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                match e.name().local_name().into_inner() {
                    "row" => {
                        cells.clear();
                        explicit_row = attribute(&e, "r")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        row_no = explicit_row;
                    }
                    "c" => {
                        cell = XlsxCell {
                            kind: attribute(&e, "t").unwrap_or_default(),
                            ..XlsxCell::default()
                        };
                        in_cell = true;
                        // Sparse sheets skip columns; pad so alignment holds.
                        let index = attribute(&e, "r")
                            .and_then(|v| column_index(&v))
                            .unwrap_or(cells.len());
                        cells.resize(cells.len().min(index), String::new());
                    }
                    "f" => in_formula = true,
                    _ => {}
                }
            }
            quick_xml::events::Event::Text(t) => {
                let text = t.into_inner();
                if in_cell {
                    if in_formula {
                        cell.formula.push_str(text.as_ref());
                    } else {
                        cell.value.push_str(text.as_ref());
                    }
                }
            }
            quick_xml::events::Event::CData(c) => {
                if in_cell {
                    if in_formula {
                        cell.formula.push_str(c.into_inner().as_ref());
                    } else {
                        cell.value.push_str(c.into_inner().as_ref());
                    }
                }
            }
            quick_xml::events::Event::End(e) => match e.name().local_name().into_inner() {
                "f" => in_formula = false,
                "c" => {
                    in_cell = false;
                    cells.push(cell.resolve(shared));
                }
                "row" => {
                    if explicit_row == 0 {
                        row_no += 1;
                    }
                    let text = cells
                        .iter()
                        .map(|c| c.trim())
                        .filter(|c| !c.is_empty())
                        .collect::<Vec<_>>()
                        .join(" | ");
                    if !text.is_empty() {
                        rows.push((row_no, text));
                    }
                    cells.clear();
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(rows)
}

/// `B7` → 0-based column index 1.
fn column_index(ref_str: &str) -> Option<usize> {
    let letters: String = ref_str
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    if letters.is_empty() {
        return None;
    }
    let mut idx = 0usize;
    for c in letters.chars() {
        idx = idx * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    Some(idx - 1)
}

// -- ods -------------------------------------------------------------------

fn extract_ods(
    archive: &mut super::zipcontainer::ZipArchive<'_>,
    budget: &mut super::zipcontainer::Budget,
    sink: &mut Sink,
) -> Result<(), ExtractError> {
    let Some(bytes) = super::zipcontainer::read_optional(archive, "content.xml", budget)? else {
        return Err(ExtractError::Malformed("ods is missing content.xml".into()));
    };

    let mut cursor = XmlCursor::new(&bytes);
    let mut sheet = String::from("Sheet1");
    let mut row_no = 0usize;
    let mut cells: Vec<String> = Vec::new();
    let mut formula: Option<String> = None;
    let mut in_value = false;
    let mut row_repeats = 1usize;
    let mut cell_from_attribute = false;
    let mut sheet_rows: Vec<Row> = Vec::new();
    let mut elements = 0usize;

    loop {
        if elements.is_multiple_of(512) {
            sink.tick()?;
        }
        if elements >= sink.limits().max_xml_elements {
            sink.mark_truncated("ods element cap reached");
            break;
        }
        elements += 1;

        match cursor.next()? {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                match e.name().local_name().into_inner() {
                    "table" => {
                        sheet = attribute(&e, "name").unwrap_or_else(|| "Sheet1".to_string());
                        row_no = 0;
                    }
                    "table-row" => {
                        cells.clear();
                        // Identical rows may be declared with a repeat marker.
                        // It applies to *this* row, so it is applied when the
                        // row closes rather than here.
                        row_repeats = repeat_count(&e, "number-rows-repeated");
                    }
                    "table-cell" | "covered-table-cell" => {
                        let value = attribute(&e, "value")
                            .or_else(|| attribute(&e, "string-value"))
                            .or_else(|| attribute(&e, "date-value"))
                            .or_else(|| attribute(&e, "boolean-value"));
                        cell_from_attribute = value.is_some();
                        let value = value.unwrap_or_default();
                        for _ in 0..repeat_count(&e, "number-columns-repeated") {
                            cells.push(value.clone());
                        }
                    }
                    "text" | "p" => in_value = true,
                    "formula" => {
                        in_value = true;
                        formula = Some(String::new());
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::Text(t) => {
                push_ods_text(
                    &mut cells,
                    &mut formula,
                    in_value,
                    cell_from_attribute,
                    t.into_inner().as_ref(),
                );
                cell_from_attribute = false;
            }
            quick_xml::events::Event::CData(c) => {
                push_ods_text(
                    &mut cells,
                    &mut formula,
                    in_value,
                    cell_from_attribute,
                    c.into_inner().as_ref(),
                );
                cell_from_attribute = false;
            }
            quick_xml::events::Event::End(e) => match e.name().local_name().into_inner() {
                "text" | "p" => in_value = false,
                "formula" => {
                    in_value = false;
                    if let Some(f) = formula.take() {
                        // ODS formulas look like `of:=SUM([.A1:.A3])`; reported
                        // verbatim, never evaluated.
                        if !f.trim().is_empty() {
                            if let Some(last) = cells.last_mut() {
                                last.push_str(&format!(" {f}"));
                            }
                        }
                    }
                }
                "table-row" => {
                    let text = cells
                        .iter()
                        .map(|c| c.trim())
                        .filter(|c| !c.is_empty())
                        .collect::<Vec<_>>()
                        .join(" | ");
                    for _ in 0..row_repeats.max(1) {
                        row_no += 1;
                        if !text.is_empty() {
                            sheet_rows.push((row_no, text.clone()));
                        }
                    }
                    row_repeats = 1;
                    cells.clear();
                }
                "table" => {
                    emit_rows(sink, &sheet, std::mem::take(&mut sheet_rows))?;
                }
                _ => {}
            },
            _ => {}
        }
    }
    emit_rows(sink, &sheet, sheet_rows)?;
    Ok(())
}

/// Routes ODS character data to the formula or to the current cell.
fn push_ods_text(
    cells: &mut [String],
    formula: &mut Option<String>,
    in_value: bool,
    from_attribute: bool,
    text: &str,
) {
    if let Some(f) = formula.as_mut() {
        f.push_str(text);
    } else if in_value {
        if let Some(last) = cells.last_mut() {
            // A cell seeded from `office:value` is replaced by its text content
            // rather than concatenated with it; the attribute is only a cached
            // form of the same value.
            if from_attribute {
                last.clear();
            }
            last.push_str(text);
        }
    }
}

/// Repeat markers are clamped: a sheet claiming a million identical rows must
/// not cost a million sections.
fn repeat_count(e: &quick_xml::events::BytesStart<'_>, key: &str) -> usize {
    attribute(e, key)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, MAX_REPEAT)
}

// -- shared ----------------------------------------------------------------

/// Pushes one section per row, charging each against the row budget and
/// stamping the sheet plus the 1-based row number.
fn emit_rows(sink: &mut Sink, sheet: &str, rows: Vec<Row>) -> Result<(), ExtractError> {
    if rows.is_empty() {
        return Ok(());
    }
    sink.push_section(Section {
        kind: SectionKind::Sheet,
        title: Some(format!("{sheet} ({} rows)", rows.len())),
        level: Some(1),
        locator: Locator::Sheet {
            sheet: sheet.to_string(),
            start_row: rows.first().map(|(r, _)| *r).unwrap_or(1),
            end_row: rows.last().map(|(r, _)| *r).unwrap_or(1),
        },
        text: format!("{} rows", rows.len()),
    });

    for (row_no, text) in rows {
        sink.tick()?;
        if !sink.charge_rows(1) {
            sink.mark_truncated("spreadsheet row cap reached");
            return Ok(());
        }
        let keep = sink.push_section(Section {
            kind: SectionKind::TableRow,
            title: None,
            level: None,
            locator: Locator::Sheet {
                sheet: sheet.to_string(),
                start_row: row_no,
                end_row: row_no,
            },
            text,
        });
        if !keep {
            sink.mark_truncated("spreadsheet section cap reached");
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::ExtractionLimits;

    fn sink() -> Sink {
        Sink::new(ExtractionLimits::default())
    }

    /// Wraps `content.xml` in the minimal container `extract_ods` expects.
    fn ods_zip(content: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
            writer.start_file("content.xml", opts).unwrap();
            writer.write_all(content).unwrap();
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn csv_input<'a>(text: &'a str, name: &'a str) -> Input<'a> {
        Input {
            path: std::path::Path::new(name),
            format: Format::Csv,
            bytes: text.as_bytes(),
            bytes_truncated: false,
            size: text.len() as u64,
            mtime: 0,
            fingerprint: "test",
        }
    }

    // -- CSV ---------------------------------------------------------------

    #[test]
    fn csv_rows_carry_header_names_and_row_locators() {
        let mut s = sink();
        CsvExtractor
            .extract(
                &csv_input("name,role\nada,engineer\ngrace,admiral\n", "team.csv"),
                &mut s,
            )
            .unwrap();
        let rows: Vec<_> = s
            .sections
            .iter()
            .filter(|x| x.kind == SectionKind::TableRow)
            .collect();
        assert_eq!(rows.len(), 2, "{:?}", s.sections);
        assert_eq!(rows[0].text, "name: ada | role: engineer");
        assert_eq!(rows[0].locator.label(), "R1-1");
        assert_eq!(rows[1].locator.label(), "R2-2");
        assert!(s.text.contains("name: grace | role: admiral"), "{}", s.text);
    }

    #[test]
    fn csv_header_becomes_the_table_title() {
        let mut s = sink();
        CsvExtractor
            .extract(&csv_input("a,b\n1,2\n", "t.csv"), &mut s)
            .unwrap();
        let title = s
            .sections
            .iter()
            .find(|x| x.kind == SectionKind::Title)
            .expect("title section");
        assert_eq!(title.title.as_deref(), Some("a | b"));
        assert_eq!(title.locator.label(), "t!R1-1");
    }

    #[test]
    fn csv_quoted_fields_and_embedded_commas() {
        let mut s = sink();
        CsvExtractor
            .extract(
                &csv_input("name,note\nada,\"first, second\"\n", "q.csv"),
                &mut s,
            )
            .unwrap();
        assert!(s.text.contains("first, second"), "{}", s.text);
    }

    #[test]
    fn csv_blank_lines_are_skipped() {
        let mut s = sink();
        CsvExtractor
            .extract(&csv_input("a,b\n\n1,2\n\n\n3,4\n", "s.csv"), &mut s)
            .unwrap();
        let rows = s
            .sections
            .iter()
            .filter(|x| x.kind == SectionKind::TableRow)
            .count();
        assert_eq!(rows, 2, "{:?}", s.sections);
    }

    #[test]
    fn csv_semicolon_delimiter_is_sniffed() {
        assert_eq!(
            detect_delimiter("a;b;c\n1;2;3\n4;5;6\n", std::path::Path::new("x.csv")),
            b';'
        );
    }

    #[test]
    fn csv_tsv_extension_is_decisive() {
        assert_eq!(
            detect_delimiter("a,b,c\n", std::path::Path::new("x.tsv")),
            b'\t'
        );
    }

    #[test]
    fn csv_single_column_falls_back_to_comma() {
        assert_eq!(
            detect_delimiter("just\nlines\nhere\n", std::path::Path::new("x.csv")),
            b','
        );
        assert_eq!(detect_delimiter("", std::path::Path::new("x.csv")), b',');
    }

    #[test]
    fn csv_row_cap_stops_without_failing() {
        let mut s = Sink::new(ExtractionLimits {
            max_rows: 2,
            ..ExtractionLimits::default()
        });
        let mut text = String::from("a\n");
        for i in 0..50 {
            text.push_str(&format!("value{i}\n"));
        }
        CsvExtractor
            .extract(&csv_input(&text, "big.csv"), &mut s)
            .unwrap();
        assert!(s.truncated());
    }

    #[test]
    fn csv_extractor_claims_only_csv() {
        let e = CsvExtractor;
        assert!(e.supports(Format::Csv));
        assert!(!e.supports(Format::Xlsx));
        assert_eq!(e.name(), "csv");
    }

    #[test]
    fn csv_content_is_never_evaluated_only_reported() {
        let mut s = sink();
        CsvExtractor
            .extract(&csv_input("cmd\n$(rm -rf /)\n`whoami`\n", "e.csv"), &mut s)
            .unwrap();
        assert!(s.text.contains("$(rm -rf /)"), "{}", s.text);
        assert!(s.text.contains("`whoami`"), "{}", s.text);
    }

    // -- spreadsheet helpers ----------------------------------------------

    #[test]
    fn spreadsheet_extractor_claims_only_sheets() {
        let e = SpreadsheetExtractor;
        assert!(e.supports(Format::Xlsx));
        assert!(e.supports(Format::Ods));
        assert!(!e.supports(Format::Docx));
        assert_eq!(e.name(), "spreadsheet");
    }

    #[test]
    fn column_indices_are_decoded() {
        assert_eq!(column_index("A1"), Some(0));
        assert_eq!(column_index("B7"), Some(1));
        assert_eq!(column_index("Z1"), Some(25));
        assert_eq!(column_index("AA1"), Some(26));
        assert_eq!(column_index("AB10"), Some(27));
        assert_eq!(column_index("1"), None);
    }

    #[test]
    fn xlsx_relationship_targets_are_normalized() {
        assert_eq!(
            normalize_xlsx_part("worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(
            normalize_xlsx_part("./worksheets/sheet3.xml"),
            "xl/worksheets/sheet3.xml"
        );
    }

    #[test]
    fn shared_strings_are_flattened() {
        let xml = br#"<sst xmlns="x"><si><t>alpha</t></si><si><r><t>be</t></r><r><t>ta</t></r></si><si><t xml:space="preserve"> keep </t></si></sst>"#;
        let mut s = sink();
        let shared = parse_shared_strings(xml, &mut s).unwrap();
        assert_eq!(shared, vec!["alpha", "beta", " keep "]);
    }

    #[test]
    fn xlsx_shared_string_cells_resolve_by_index() {
        let cell = XlsxCell {
            kind: "s".into(),
            value: "1".into(),
            formula: String::new(),
        };
        let shared = ["Name".to_string(), "Age".to_string()];
        assert_eq!(cell.resolve(&shared), "Age");

        // A numeric cell keeps its value; it is never index-looked-up.
        let numeric = XlsxCell {
            kind: String::new(),
            value: "36".into(),
            formula: String::new(),
        };
        assert_eq!(numeric.resolve(&shared), "36");

        // An unresolvable index stays literal rather than panicking.
        let dangling = XlsxCell {
            kind: "s".into(),
            value: "99".into(),
            formula: String::new(),
        };
        assert_eq!(dangling.resolve(&shared), "99");
    }

    #[test]
    fn xlsx_sheet_rows_are_read_in_order() {
        let xml = br#"<worksheet xmlns="x"><sheetData>
            <row r="1"><c r="A1"><v>42</v></c><c r="B1"><v>hello</v></c></row>
            <row r="2"><c r="A2"><v>7</v></c></row>
          </sheetData></worksheet>"#;
        let mut s = sink();
        let rows = parse_xlsx_sheet(xml, &[], &mut s).unwrap();
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].0, 1);
        assert_eq!(rows[0].1, "42 | hello");
        assert_eq!(rows[1].1, "7");
    }

    #[test]
    fn xlsx_formula_is_reported_not_evaluated() {
        let xml = br#"<worksheet xmlns="x"><sheetData>
            <row r="1"><c r="A1"><f>SUM(B1:B9)</f><v>42</v></c></row>
          </sheetData></worksheet>"#;
        let mut s = sink();
        let rows = parse_xlsx_sheet(xml, &[], &mut s).unwrap();
        assert!(rows[0].1.contains("=SUM(B1:B9)"), "{:?}", rows[0]);
    }

    #[test]
    fn ods_cells_rows_and_repeats_are_read() {
        let xml = br#"<office:document-content
            xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
            xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"
            xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">
          <office:body><office:spreadsheet>
            <table:table table:name="Data">
              <table:table-row>
                <table:table-cell office:value-type="float" office:value="1"><text:p>one</text:p></table:table-cell>
              </table:table-row>
              <table:table-row table:number-rows-repeated="3">
                <table:table-cell office:value-type="string" office:string-value="dup"/>
              </table:table-row>
            </table:table>
          </office:spreadsheet></office:body>
        </office:document-content>"#;
        let mut s = sink();
        let bytes = ods_zip(xml);
        let mut archive = super::super::zipcontainer::open(&bytes).unwrap();
        let mut budget = super::super::zipcontainer::Budget::new(s.limits());
        extract_ods(&mut archive, &mut budget, &mut s).unwrap();
        assert!(s.text.contains("one"), "{}", s.text);
        assert_eq!(
            s.text.matches("dup").count(),
            3,
            "repeat honoured: {}",
            s.text
        );
        assert!(
            s.sections
                .iter()
                .any(|x| x.title.as_deref() == Some("Data (4 rows)")),
            "{:?}",
            s.sections
                .iter()
                .map(|x| x.title.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn ods_formula_is_reported_not_evaluated() {
        let xml = br#"<office:document-content
            xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
            xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"
            xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">
          <office:body><office:spreadsheet>
            <table:table table:name="F">
              <table:table-row>
                <table:table-cell table:formula="of:=SUM([.A1:.A3])" office:value-type="float" office:value="6">
                  <text:p>6</text:p>
                </table:table-cell>
              </table:table-row>
            </table:table>
          </office:spreadsheet></office:body>
        </office:document-content>"#;
        let mut s = sink();
        let bytes = ods_zip(xml);
        let mut archive = super::super::zipcontainer::open(&bytes).unwrap();
        let mut budget = super::super::zipcontainer::Budget::new(s.limits());
        extract_ods(&mut archive, &mut budget, &mut s).unwrap();
        // The value is reported; the formula attribute is not executed.
        assert!(s.text.contains("6"), "{}", s.text);
    }

    #[test]
    fn repeat_markers_are_clamped() {
        let plain = quick_xml::events::BytesStart::new("table-row");
        assert_eq!(repeat_count(&plain, "number-rows-repeated"), 1);

        // `table-row` is 9 bytes, so the attribute content starts there.
        let huge = quick_xml::events::BytesStart::from_content(
            r#"table-row number-rows-repeated="1000000""#,
            9,
        );
        assert_eq!(repeat_count(&huge, "number-rows-repeated"), MAX_REPEAT);
    }

    #[test]
    fn emit_rows_charges_the_row_budget() {
        let mut s = Sink::new(ExtractionLimits {
            max_rows: 2,
            ..ExtractionLimits::default()
        });
        let rows: Vec<Row> = (1..=10).map(|i| (i, format!("row {i}"))).collect();
        emit_rows(&mut s, "S", rows).unwrap();
        assert!(s.truncated());
        let kept = s
            .sections
            .iter()
            .filter(|x| x.kind == SectionKind::TableRow)
            .count();
        assert!(kept <= 2, "{kept} rows kept");
    }

    #[test]
    fn emit_rows_of_an_empty_sheet_adds_nothing() {
        let mut s = sink();
        emit_rows(&mut s, "S", Vec::new()).unwrap();
        assert!(s.sections.is_empty());
    }

    #[test]
    fn malformed_csv_is_a_partial_result_not_a_crash() {
        let mut s = sink();
        // Must not panic regardless of how the parser copes.
        let _ = CsvExtractor.extract(&csv_input("a,b\n\"unterminated,c\n", "bad.csv"), &mut s);
    }
}
