//! Structured-data extractors: JSON/JSONL (always compiled), YAML, TOML and
//! XML (behind the `format-structured` feature).
//!
//! All of them turn structure into *key-path sections* so retrieval can quote
//! `config/database.host` rather than a blob. Parsing never evaluates code:
//! YAML is parsed with the safe loader (no custom tags executed), TOML through
//! `toml::Value`, XML as a streaming pull parser that resolves only the five
//! predefined entities (see `text::decode_entities`).

use super::{ExtractError, Extractor, Format, Input, Locator, Section, SectionKind, Sink};

/// JSON and JSON Lines.
pub struct JsonExtractor;

impl Extractor for JsonExtractor {
    fn name(&self) -> &'static str {
        "json"
    }

    fn supports(&self, format: Format) -> bool {
        matches!(format, Format::Json | Format::Jsonl)
    }

    fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let text = std::str::from_utf8(input.bytes)
            .map_err(|e| ExtractError::Malformed(format!("{}: {e}", input.path.display())))?;
        if input.format == Format::Jsonl {
            extract_jsonl(text, sink)
        } else {
            extract_json(text, input, sink)
        }
    }
}

fn extract_json(text: &str, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
        ExtractError::Malformed(format!("{}: invalid JSON: {e}", input.path.display()))
    })?;
    emit_json_sections(&value, sink, "")
}

fn emit_json_sections(
    value: &serde_json::Value,
    sink: &mut Sink,
    prefix: &str,
) -> Result<(), ExtractError> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                sink.tick()?;
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                if child.is_object() || child.is_array() {
                    // One section per container member, then recurse into
                    // *small* containers only; deep trees flatten below.
                    let rendered = pretty(child, 64 * 1024);
                    sink.push_section(Section {
                        kind: SectionKind::KeyValues,
                        title: Some(path.clone()),
                        level: None,
                        locator: Locator::None,
                        text: rendered,
                    });
                    if path.matches('.').count() < 4 {
                        emit_json_sections(child, sink, &path)?;
                    }
                } else {
                    sink.push_section(Section {
                        kind: SectionKind::KeyValues,
                        title: Some(path),
                        level: None,
                        locator: Locator::None,
                        text: pretty(child, 8 * 1024),
                    });
                }
                if sink.sections.len() >= sink.limits().max_sections {
                    sink.mark_truncated("section cap reached while walking JSON");
                    return Ok(());
                }
            }
        }
        serde_json::Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                sink.tick()?;
                let path = if prefix.is_empty() {
                    format!("[{i}]")
                } else {
                    format!("{prefix}[{i}]")
                };
                sink.push_section(Section {
                    kind: SectionKind::KeyValues,
                    title: Some(path.clone()),
                    level: None,
                    locator: Locator::None,
                    text: pretty(child, 16 * 1024),
                });
                if sink.sections.len() >= sink.limits().max_sections {
                    sink.mark_truncated("section cap reached while walking JSON array");
                    return Ok(());
                }
                if child.is_object() && path.matches('.').count() < 3 {
                    emit_json_sections(child, sink, &path)?;
                }
            }
        }
        other => {
            sink.push_section(Section {
                kind: SectionKind::KeyValues,
                title: Some(prefix.to_string()),
                level: None,
                locator: Locator::None,
                text: pretty(other, 8 * 1024),
            });
        }
    }
    Ok(())
}

fn pretty(value: &serde_json::Value, cap: usize) -> String {
    let mut out = serde_json::to_string_pretty(value).unwrap_or_default();
    if out.len() > cap {
        out.truncate(cap);
        out.push_str("\n… (truncated)");
    }
    out
}

/// JSON Lines: every line is an independent record. A malformed line is a
/// warning and a skip, never a failure - partial extraction is the contract.
fn extract_jsonl(text: &str, sink: &mut Sink) -> Result<(), ExtractError> {
    const BLOCK: usize = 50;
    let mut block: Vec<(usize, serde_json::Value)> = Vec::new();
    let mut skipped = 0usize;
    let mut first_bad = 0usize;
    let total_lines = text.lines().count();

    for (idx, raw) in text.lines().enumerate() {
        sink.tick()?;
        let line_no = idx + 1;
        if raw.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(value) => block.push((line_no, value)),
            Err(_) => {
                skipped += 1;
                if first_bad == 0 {
                    first_bad = line_no;
                }
            }
        }
        if block.len() >= BLOCK || line_no == total_lines {
            flush_jsonl_block(sink, &mut block)?;
        }
        if !sink.charge_rows(1) {
            sink.warn("jsonl row budget exhausted");
            break;
        }
    }
    if skipped > 0 {
        sink.warn(format!(
            "skipped {skipped} malformed line(s), first at line {first_bad}"
        ));
    }
    if sink.sections.is_empty() && skipped > 0 {
        return Err(ExtractError::Malformed(
            "no valid JSON records found".into(),
        ));
    }
    Ok(())
}

/// Emits one section per record. The line number is kept on the locator so a
/// quote from a log can name the line it came from, which is the whole point of
/// a row-oriented source.
fn flush_jsonl_block(
    sink: &mut Sink,
    block: &mut Vec<(usize, serde_json::Value)>,
) -> Result<(), ExtractError> {
    for (line_no, value) in block.drain(..) {
        sink.tick()?;
        let mut text = serde_json::to_string(&value).unwrap_or_default();
        if text.len() > 64 * 1024 {
            text.truncate(64 * 1024);
            text.push_str("… (truncated)");
        }
        let keep = sink.push_section(Section {
            kind: SectionKind::Block,
            title: record_title(&value),
            level: None,
            locator: Locator::Rows {
                start: line_no,
                end: line_no,
            },
            text,
        });
        if !keep {
            return Ok(());
        }
    }
    Ok(())
}

fn record_title(value: &serde_json::Value) -> Option<String> {
    let obj = value.as_object()?;
    for key in ["id", "name", "title", "key", "path", "type"] {
        if let Some(s) = obj.get(key).and_then(|v| v.as_str()) {
            return Some(format!("{key}: {s}"));
        }
    }
    None
}

#[cfg(feature = "format-structured")]
// Inner module keeps the file's layout mirrors docs/ARCHITECTURE.md; the
// shared name is intentional (module inception), so silence the lint.
#[allow(clippy::module_inception)]
mod structured {
    use super::*;
    use quick_xml::events::Event;
    use quick_xml::Reader;

    /// YAML, TOML and XML in one extractor.
    pub struct StructuredExtractor;

    impl Extractor for StructuredExtractor {
        fn name(&self) -> &'static str {
            "structured"
        }

        fn supports(&self, format: Format) -> bool {
            matches!(format, Format::Yaml | Format::Toml | Format::Xml)
        }

        fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
            let text = std::str::from_utf8(input.bytes)
                .map_err(|e| ExtractError::Malformed(format!("{}: {e}", input.path.display())))?;
            match input.format {
                Format::Yaml => extract_yaml(text, input, sink),
                Format::Toml => extract_toml(text, input, sink),
                Format::Xml => extract_xml(text, sink),
                _ => Ok(()),
            }
        }
    }

    fn extract_yaml(text: &str, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let docs = yaml_rust2::YamlLoader::load_from_str(text).map_err(|e| {
            ExtractError::Malformed(format!("{}: invalid YAML: {e}", input.path.display()))
        })?;
        for (i, doc) in docs.iter().enumerate() {
            sink.tick()?;
            if docs.len() > 1 {
                sink.push_section(Section {
                    kind: SectionKind::Block,
                    title: Some(format!("document {i}")),
                    level: None,
                    locator: Locator::None,
                    text: render_yaml(doc, 0),
                });
                continue;
            }
            match doc {
                yaml_rust2::Yaml::Hash(map) => {
                    for (key, value) in map {
                        sink.tick()?;
                        let title = yaml_key(key);
                        let body = render_yaml(value, 0);
                        sink.push_section(Section {
                            kind: SectionKind::KeyValues,
                            title: Some(title.clone()),
                            level: None,
                            locator: Locator::None,
                            text: body,
                        });
                        if sink.sections.len() >= sink.limits().max_sections {
                            sink.mark_truncated("section cap reached in YAML");
                            return Ok(());
                        }
                    }
                }
                other => {
                    sink.push_section(Section {
                        kind: SectionKind::KeyValues,
                        title: None,
                        level: None,
                        locator: Locator::None,
                        text: render_yaml(other, 0),
                    });
                }
            }
        }
        Ok(())
    }

    fn yaml_key(key: &yaml_rust2::Yaml) -> String {
        match key {
            yaml_rust2::Yaml::String(s) => s.clone(),
            yaml_rust2::Yaml::Integer(i) => i.to_string(),
            yaml_rust2::Yaml::Real(r) => r.clone(),
            yaml_rust2::Yaml::Boolean(b) => b.to_string(),
            other => render_yaml(other, 0),
        }
    }

    /// Own renderer instead of relying on `Yaml`'s Display, with depth and
    /// width caps so cyclic-ish/very deep documents terminate.
    fn render_yaml(y: &yaml_rust2::Yaml, depth: usize) -> String {
        let mut out = String::new();
        render_yaml_into(y, depth, &mut out);
        out
    }

    fn render_yaml_into(y: &yaml_rust2::Yaml, depth: usize, out: &mut String) {
        if depth > 12 || out.len() > 64 * 1024 {
            out.push('…');
            return;
        }
        match y {
            yaml_rust2::Yaml::Real(s) | yaml_rust2::Yaml::String(s) => out.push_str(s),
            yaml_rust2::Yaml::Integer(i) => out.push_str(&i.to_string()),
            yaml_rust2::Yaml::Boolean(b) => out.push_str(&b.to_string()),
            yaml_rust2::Yaml::Null => out.push_str("null"),
            yaml_rust2::Yaml::BadValue => out.push_str("<bad>"),
            yaml_rust2::Yaml::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&format!("{i}: "));
                    render_yaml_into(item, depth + 1, out);
                    out.push('\n');
                }
            }
            yaml_rust2::Yaml::Hash(map) => {
                for (k, v) in map {
                    out.push_str(&yaml_key(k));
                    out.push_str(": ");
                    if matches!(v, yaml_rust2::Yaml::Hash(_) | yaml_rust2::Yaml::Array(_)) {
                        out.push('\n');
                    }
                    render_yaml_into(v, depth + 1, out);
                    out.push('\n');
                }
            }
            yaml_rust2::Yaml::Alias(a) => out.push_str(&format!("alias({a})")),
        }
    }

    fn extract_toml(text: &str, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let value: toml::Value = toml::from_str(text).map_err(|e| {
            ExtractError::Malformed(format!("{}: invalid TOML: {e}", input.path.display()))
        })?;
        match value {
            toml::Value::Table(table) => {
                emit_toml_table(&table, String::new(), sink, 0)?;
                Ok(())
            }
            other => {
                sink.push_section(Section {
                    kind: SectionKind::KeyValues,
                    title: None,
                    level: None,
                    locator: Locator::None,
                    text: render_toml_value(&other),
                });
                Ok(())
            }
        }
    }

    fn emit_toml_table(
        table: &toml::Table,
        path: String,
        sink: &mut Sink,
        depth: usize,
    ) -> Result<(), ExtractError> {
        let mut scalars = String::new();
        for (key, value) in table {
            sink.tick()?;
            let child_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            match value {
                toml::Value::Table(sub) if depth < 3 => {
                    // Flush pending scalars as their own section first.
                    if !scalars.trim().is_empty() {
                        push_kv(sink, path_label(&path), scalars.clone());
                        scalars.clear();
                    }
                    push_kv(sink, Some(child_path.clone()), render_toml_value(value));
                    emit_toml_table(sub, child_path, sink, depth + 1)?;
                }
                other => {
                    scalars.push_str(&format!("{key} = {}\n", render_toml_value(other)));
                    if scalars.len() > 32 * 1024 {
                        push_kv(sink, path_label(&path), scalars.clone());
                        scalars.clear();
                    }
                }
            }
            if sink.sections.len() >= sink.limits().max_sections {
                sink.mark_truncated("section cap reached in TOML");
                return Ok(());
            }
        }
        if !scalars.trim().is_empty() {
            push_kv(sink, path_label(&path), scalars);
        }
        Ok(())
    }

    fn path_label(path: &str) -> Option<String> {
        if path.is_empty() {
            None
        } else {
            Some(path.to_string())
        }
    }

    fn push_kv(sink: &mut Sink, title: Option<String>, text: String) {
        sink.push_section(Section {
            kind: SectionKind::KeyValues,
            title,
            level: None,
            locator: Locator::None,
            text,
        });
    }

    /// Renders a TOML value as JSON text: serde_json is already a hard
    /// dependency, and JSON is unambiguous for retrieval.
    fn render_toml_value(value: &toml::Value) -> String {
        let as_json = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
        let mut out = serde_json::to_string_pretty(&as_json).unwrap_or_default();
        if out.len() > 64 * 1024 {
            out.truncate(64 * 1024);
            out.push_str("\n… (truncated)");
        }
        out
    }

    /// Streaming XML: one section per element at depth <= 2, text nodes and
    /// attribute names preserved, nesting flattened. Entities: only the five
    /// predefined ones (unknown references stay literal - no external/DTD
    /// expansion is ever attempted).
    fn extract_xml(text: &str, sink: &mut Sink) -> Result<(), ExtractError> {
        let mut reader = Reader::from_str(text);
        let mut stack: Vec<String> = Vec::new();
        let mut buf = String::new();
        let mut section_title: Option<String> = None;
        let mut section_attrs = String::new();
        let mut elements = 0usize;
        let mut depth = 0usize;

        loop {
            if elements.is_multiple_of(512) {
                sink.tick()?;
            }
            if elements >= sink.limits().max_xml_elements {
                sink.mark_truncated("xml element cap reached");
                break;
            }
            let event = match reader.read_event() {
                Ok(Event::Eof) => break,
                Ok(event) => event,
                Err(_) => {
                    sink.warn("malformed XML: stopped at first parse error (partial text kept)");
                    break;
                }
            };
            match event {
                Event::Start(e) => {
                    elements += 1;
                    depth += 1;
                    let name = e.name().into_inner().to_string();
                    stack.push(name.clone());
                    if depth <= 2 {
                        // Close out the previous shallow section.
                        if let Some(title) = section_title.take() {
                            push_xml_section(sink, title, &section_attrs, &mut buf);
                        }
                        section_title = Some(name.clone());
                        section_attrs.clear();
                        buf.clear();
                        for attr in e.attributes().flatten() {
                            let key = attr.key.into_inner().to_string();
                            let raw = attr.value.into_owned();
                            let val = super::super::text::decode_entities(&raw);
                            section_attrs.push_str(&format!("{key}=\"{val}\" "));
                        }
                    }
                }
                Event::Empty(e) => {
                    elements += 1;
                    if depth <= 2 {
                        let name = e.name().into_inner().to_string();
                        if let Some(title) = section_title.take() {
                            push_xml_section(sink, title, &section_attrs, &mut buf);
                        }
                        let mut attrs = String::new();
                        for attr in e.attributes().flatten() {
                            let key = attr.key.into_inner().to_string();
                            let raw = attr.value.into_owned();
                            let val = super::super::text::decode_entities(&raw);
                            attrs.push_str(&format!("{key}=\"{val}\" "));
                        }
                        push_xml_section(sink, name, &attrs, &mut buf);
                    }
                }
                Event::GeneralRef(r) => {
                    // Only the five predefined names and numeric references
                    // resolve. An unknown entity - a DTD-declared one, say -
                    // stays literal, so no external entity is ever expanded.
                    if depth > 0 {
                        let text =
                            super::super::text::decode_entities(&format!("&{};", r.as_ref()));
                        buf.push_str(&text);
                    }
                }
                Event::Text(t) => {
                    if depth > 0 {
                        let raw = t.into_inner().into_owned();
                        let text = super::super::text::decode_entities(&raw);
                        if !text.trim().is_empty() {
                            buf.push_str(text.trim());
                            buf.push(' ');
                        }
                    }
                }
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    stack.pop();
                    if depth == 0 {
                        if let Some(title) = section_title.take() {
                            push_xml_section(sink, title, &section_attrs, &mut buf);
                        }
                    }
                    if sink.sections.len() >= sink.limits().max_sections {
                        sink.mark_truncated("section cap reached in XML");
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(title) = section_title.take() {
            push_xml_section(sink, title, &section_attrs, &mut buf);
        }
        Ok(())
    }

    fn push_xml_section(sink: &mut Sink, title: String, attrs: &str, buf: &mut String) {
        let mut text = String::new();
        if !attrs.trim().is_empty() {
            text.push_str(attrs.trim_end());
            text.push('\n');
        }
        text.push_str(buf.trim_end());
        if text.trim().is_empty() {
            buf.clear();
            return;
        }
        sink.push_section(Section {
            kind: SectionKind::Block,
            title: Some(title),
            level: None,
            locator: Locator::None,
            text,
        });
        buf.clear();
    }
}

#[cfg(feature = "format-structured")]
pub use structured::StructuredExtractor;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{ExtractionLimits, ExtractorRegistry, Format};

    fn extract(name: &str, bytes: &[u8]) -> crate::extract::Document {
        let dir =
            std::env::temp_dir().join(format!("nanomite-struct-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let doc = ExtractorRegistry::builtin()
            .extract_path(&path, &ExtractionLimits::default())
            .unwrap_or_else(|e| panic!("extract {name}: {e}"));
        let _ = std::fs::remove_dir_all(&dir);
        doc
    }

    #[test]
    fn json_sections_carry_key_paths() {
        let doc = extract(
            "cfg.json",
            br#"{"database": {"host": "db1", "port": 5432}, "name": "svc"}"#,
        );
        assert_eq!(doc.format, Format::Json);
        let titles: Vec<_> = doc
            .sections
            .iter()
            .filter_map(|s| s.title.clone())
            .collect();
        assert!(titles.iter().any(|t| t == "database.host"), "{titles:?}");
        assert!(titles.iter().any(|t| t == "name"));
        assert!(doc.text.contains("db1"));
    }

    #[test]
    fn jsonl_partial_extraction_skips_bad_lines() {
        let doc = extract(
            "log.jsonl",
            b"{\"id\":1}\nnot json\n{\"id\":2}\n{\"id\":3}\n",
        );
        assert_eq!(doc.format, Format::Jsonl);
        assert!(
            doc.warnings.iter().any(|w| w.contains("line 2")),
            "{:?}",
            doc.warnings
        );
        assert!(doc.text.contains("\"id\":1"));
        assert!(doc.text.contains("\"id\":3"));
        assert!(!doc.text.contains("not json"));
        assert!(doc
            .sections
            .iter()
            .any(|s| matches!(s.locator, Locator::Rows { start: 1, end: 1 })));
    }

    #[test]
    fn jsonl_fully_invalid_is_malformed() {
        let dir = std::env::temp_dir().join(format!("nanomite-struct-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.jsonl");
        std::fs::write(&path, b"nope\nalso nope\n").unwrap();
        let err = ExtractorRegistry::builtin()
            .extract_path(&path, &ExtractionLimits::default())
            .unwrap_err();
        assert_eq!(err.code(), "malformed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_json_is_malformed_with_path_in_detail() {
        let dir = std::env::temp_dir().join(format!("nanomite-struct-j-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken.json");
        std::fs::write(&path, b"{\"a\": }").unwrap();
        let err = ExtractorRegistry::builtin()
            .extract_path(&path, &ExtractionLimits::default())
            .unwrap_err();
        assert_eq!(err.code(), "malformed");
        assert!(err.to_string().contains("broken.json"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "format-structured")]
    #[test]
    fn yaml_sections_carry_top_level_keys() {
        let doc = extract(
            "cfg.yaml",
            b"database:\n  host: db1\nname: svc\nlist:\n  - a\n  - b\n",
        );
        assert_eq!(doc.format, Format::Yaml);
        let titles: Vec<_> = doc
            .sections
            .iter()
            .filter_map(|s| s.title.clone())
            .collect();
        assert!(titles.iter().any(|t| t == "database"), "{titles:?}");
        assert!(titles.iter().any(|t| t == "list"));
        assert!(doc.text.contains("db1"));
    }

    #[cfg(feature = "format-structured")]
    #[test]
    fn toml_tables_become_sections() {
        let doc = extract(
            "cfg.toml",
            b"title = \"app\"\n\n[database]\nhost = \"db1\"\n\n[database.replica]\nhost = \"db2\"\n",
        );
        assert_eq!(doc.format, Format::Toml);
        let titles: Vec<_> = doc
            .sections
            .iter()
            .filter_map(|s| s.title.clone())
            .collect();
        assert!(titles.iter().any(|t| t == "database"), "{titles:?}");
        assert!(doc.text.contains("db1"));
    }

    #[cfg(feature = "format-structured")]
    #[test]
    fn xml_shallow_elements_become_sections_with_attributes() {
        let doc = extract(
            "cfg.xml",
            br#"<?xml version="1.0"?><root><item id="1">hello</item><item id="2">world</item></root>"#,
        );
        assert_eq!(doc.format, Format::Xml);
        let titles: Vec<_> = doc
            .sections
            .iter()
            .filter_map(|s| s.title.clone())
            .collect();
        assert!(titles.iter().any(|t| t == "item"), "{titles:?}");
        assert!(doc.text.contains("hello"));
        assert!(doc.text.contains("id=\"1\""));
        assert!(doc.text.contains("world"));
    }

    #[cfg(feature = "format-structured")]
    #[test]
    fn xml_entity_expansion_is_not_possible() {
        // External entity declarations are ignored; the reference stays literal
        // instead of expanding to file contents.
        let doc = extract(
            "xxe.xml",
            br#"<?xml version="1.0"?><!DOCTYPE r [<!ENTITY x SYSTEM "file:///etc/passwd">]><r>&x;&amp;</r>"#,
        );
        assert!(!doc.text.contains("root:"), "XXE leaked: {}", doc.text);
        assert!(
            doc.text.contains("&x;"),
            "reference should stay literal: {}",
            doc.text
        );
        assert!(doc.text.contains('&'), "&amp; decodes to &");
    }

    #[cfg(feature = "format-structured")]
    #[test]
    fn malformed_xml_keeps_partial_text() {
        let doc = extract("partial.xml", b"<a>one</a><b>two<borken");
        assert!(doc.text.contains("one"));
        assert!(doc.warnings.iter().any(|w| w.contains("malformed")));
    }
}
