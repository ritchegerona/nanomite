//! Safe reading of zip-based containers (XLSX, ODS, DOCX, ODT).
//!
//! Untrusted archives get three independent checks before any bytes are used:
//!
//! 1. **Entry-name validation** - no absolute paths, no `..`, no drive letters,
//!    no backslash separators, no NUL. Names are used only as *keys into the
//!    archive*, never joined onto the filesystem, but we validate anyway so a
//!    caller cannot be tricked into treating an entry as a real path.
//! 2. **Expansion accounting** - every byte read counts against
//!    `max_expanded_bytes`, every entry against `max_archive_entries`, and each
//!    single entry against `max_file_bytes`. A 4 GB bomb hits the cap after
//!    the limit, not after the allocation.
//! 3. **Deadline ticks** - entry loops check the shared deadline.
//!
//! Nesting (a zip inside a zip) is not performed: `max_nesting = 0` by
//! default, and no extractor recurses into nested archives.

use super::{Deadline, ExtractError, ExtractionLimits, Format};
use std::io::{Cursor, Read};

pub type ZipArchive<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

/// Extension-based format claim for a zip container, confirmed later by the
/// owning extractor when it looks for its real entries.
pub fn format_from_entry_names(ext: &str) -> Option<Format> {
    match ext {
        "xlsx" | "xlsm" | "xltx" => Some(Format::Xlsx),
        "ods" | "ots" => Some(Format::Ods),
        "docx" | "docm" | "dotx" => Some(Format::Docx),
        "odt" | "ott" => Some(Format::Odt),
        // A bare `.zip` is not a document we claim; the caller treats it as
        // unsupported rather than unpacking arbitrary archives.
        _ => None,
    }
}

/// Opens a capped byte slice as a zip archive. The slice must outlive the
/// archive, so extractors hold both on the stack together.
pub fn open(bytes: &[u8]) -> Result<zip::ZipArchive<Cursor<&[u8]>>, ExtractError> {
    zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ExtractError::Malformed(format!("not a readable zip container: {e}")))
}

/// Validates an entry name before it is trusted as an archive key.
pub fn validate_entry_name(name: &str) -> Result<(), ExtractError> {
    if name.is_empty() {
        return Err(ExtractError::Malformed("zip entry with empty name".into()));
    }
    if name.contains('\0') {
        return Err(ExtractError::PathEscape(format!(
            "zip entry name contains NUL: {name:?}"
        )));
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(ExtractError::PathEscape(format!(
            "zip entry with absolute path: {name:?}"
        )));
    }
    if name.as_bytes().get(1) == Some(&b':') {
        return Err(ExtractError::PathEscape(format!(
            "zip entry with drive letter: {name:?}"
        )));
    }
    for component in name.split(['/', '\\']) {
        if component == ".." {
            return Err(ExtractError::PathEscape(format!(
                "zip entry with traversal component: {name:?}"
            )));
        }
    }
    Ok(())
}

/// Running accounting of one archive's read budget.
pub struct Budget {
    entries: usize,
    bytes: u64,
    limits: ExtractionLimits,
    deadline: Deadline,
}

impl Budget {
    pub fn new(limits: &ExtractionLimits) -> Self {
        Self {
            entries: 0,
            bytes: 0,
            limits: limits.clone(),
            deadline: Deadline::new(limits.max_duration),
        }
    }

    /// Charges one entry slot; errors once `max_archive_entries` is reached.
    pub fn charge_entry(&mut self) -> Result<(), ExtractError> {
        if self.deadline.expired() {
            return Err(ExtractError::Timeout("zip read timed out".into()));
        }
        if self.entries >= self.limits.max_archive_entries {
            return Err(ExtractError::Limit(format!(
                "archive read stopped after {} entries (max_archive_entries)",
                self.limits.max_archive_entries
            )));
        }
        self.entries += 1;
        Ok(())
    }

    /// Charges `n` uncompressed bytes against both the per-archive expansion
    /// cap and the per-file cap.
    pub fn charge_bytes(&mut self, n: u64) -> Result<(), ExtractError> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > self.limits.max_expanded_bytes {
            return Err(ExtractError::TooLarge(format!(
                "archive expansion of {} bytes exceeds the {} byte budget (zip bomb guard)",
                self.bytes, self.limits.max_expanded_bytes
            )));
        }
        if n > self.limits.max_file_bytes {
            return Err(ExtractError::TooLarge(format!(
                "zip entry is {} bytes, above the {} byte read cap",
                n, self.limits.max_file_bytes
            )));
        }
        Ok(())
    }

    pub fn entries_read(&self) -> usize {
        self.entries
    }

    pub fn bytes_read(&self) -> u64 {
        self.bytes
    }
}

/// Names of every entry in the archive, validated and bounded. Listing reads
/// only the central directory (no decompression), so it costs no byte budget;
/// it is still capped so a hostile directory cannot exhaust memory.
pub fn entry_names(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    budget: &mut Budget,
) -> Result<Vec<String>, ExtractError> {
    let scan_cap = 4096.min(archive.len());
    let mut names = Vec::with_capacity(scan_cap.min(1024));
    for i in 0..scan_cap {
        if i % 256 == 0 && budget.deadline.expired() {
            return Err(ExtractError::Timeout("zip directory scan timed out".into()));
        }
        let name = entry_name(archive, i)?;
        validate_entry_name(&name)?;
        names.push(name);
    }
    Ok(names)
}

/// Reads one entry by name into memory, enforcing every budget.
pub fn read_entry(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    budget: &mut Budget,
) -> Result<Vec<u8>, ExtractError> {
    validate_entry_name(name)?;
    budget.charge_entry()?;
    let declared = archive
        .by_name(name)
        .map_err(|e| ExtractError::Malformed(format!("zip entry {name:?}: {e}")))?
        .size();
    if declared > budget.limits.max_file_bytes {
        return Err(ExtractError::TooLarge(format!(
            "zip entry {name:?} declares {declared} bytes, above the {} byte cap",
            budget.limits.max_file_bytes
        )));
    }
    let mut buf = Vec::with_capacity(declared.min(64 * 1024) as usize);
    let mut entry = archive
        .by_name(name)
        .map_err(|e| ExtractError::Malformed(format!("zip entry {name:?}: {e}")))?;
    entry
        .by_ref()
        .take(budget.limits.max_file_bytes)
        .read_to_end(&mut buf)?;
    budget.charge_bytes(buf.len() as u64)?;
    Ok(buf)
}

/// Reads `name` when the archive has it. A missing *optional* part is not an
/// error, but budget and I/O failures stay fatal: a partial read of a required
/// part is not a document.
pub fn read_optional(
    archive: &mut ZipArchive<'_>,
    name: &str,
    budget: &mut Budget,
) -> Result<Option<Vec<u8>>, ExtractError> {
    match read_entry(archive, name, budget) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(ExtractError::Malformed(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

/// Reads the raw bytes of an entry addressed by index (used when iterating).
pub fn read_entry_at(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    index: usize,
    budget: &mut Budget,
) -> Result<(String, Vec<u8>), ExtractError> {
    budget.charge_entry()?;
    let name = entry_name(archive, index)?;
    validate_entry_name(&name)?;
    let declared = archive
        .by_index(index)
        .map_err(|e| ExtractError::Malformed(format!("zip entry #{index}: {e}")))?
        .size();
    if declared > budget.limits.max_file_bytes {
        return Err(ExtractError::TooLarge(format!(
            "zip entry {name:?} declares {declared} bytes, above the {} byte cap",
            budget.limits.max_file_bytes
        )));
    }
    let mut buf = Vec::with_capacity(declared.min(64 * 1024) as usize);
    let mut entry = archive
        .by_index(index)
        .map_err(|e| ExtractError::Malformed(format!("zip entry #{index}: {e}")))?;
    entry
        .by_ref()
        .take(budget.limits.max_file_bytes)
        .read_to_end(&mut buf)?;
    budget.charge_bytes(buf.len() as u64)?;
    Ok((name, buf))
}

/// First existing name among `candidates`, or `None`.
pub fn first_existing<S: AsRef<str>>(
    names: impl IntoIterator<Item = S>,
    candidates: &[&str],
) -> Option<String> {
    let names: Vec<String> = names.into_iter().map(|n| n.as_ref().to_string()).collect();
    for candidate in candidates {
        if names.iter().any(|n| n == candidate) {
            return Some((*candidate).to_string());
        }
    }
    None
}

fn entry_name(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    index: usize,
) -> Result<String, ExtractError> {
    // zip 9 returns a Result (non-UTF8 names); older versions returned &str.
    // Going through the API once here keeps the difference contained.
    let file = archive
        .by_index(index)
        .map_err(|e| ExtractError::Malformed(format!("zip entry #{index}: {e}")))?;
    let name: String = file
        .name()
        .map_err(|e| ExtractError::Malformed(format!("zip entry #{index}: {e}")))?
        .trim_end_matches('/')
        .to_string();
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::ExtractionLimits;

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, data) in entries {
                writer.start_file(*name, opts).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn scratch_zip(name: &str, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nanomite-zip-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, zip_with(entries)).unwrap();
        path
    }

    #[test]
    fn rejects_traversal_and_absolute_names() {
        assert!(validate_entry_name("../evil").is_err());
        assert!(validate_entry_name("a/../../b").is_err());
        assert!(validate_entry_name("/etc/passwd").is_err());
        assert!(validate_entry_name("C:\\windows").is_err());
        assert!(validate_entry_name("ok/name.txt").is_ok());
    }

    #[test]
    fn entry_names_and_reads_work() {
        let bytes = zip_with(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("xl/workbook.xml", b"<wb/>"),
        ]);
        let mut archive = open(&bytes).unwrap();
        let mut budget = Budget::new(&ExtractionLimits::default());
        let names = entry_names(&mut archive, &mut budget).unwrap();
        assert_eq!(names.len(), 2);
        let data = read_entry(&mut archive, "xl/workbook.xml", &mut budget).unwrap();
        assert_eq!(data, b"<wb/>");
    }

    #[test]
    fn expansion_budget_stops_bombs() {
        // 300 entries, each under the per-file cap, but over the archive cap.
        let big = vec![b'x'; 10_000];
        let names: Vec<&'static str> = (0..300)
            .map(|i| Box::leak(format!("f{i}.bin").into_boxed_str()) as &'static str)
            .collect();
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (*n, big.as_slice())).collect();
        let bytes = zip_with(&entries);
        let mut archive = open(&bytes).unwrap();
        let limits = ExtractionLimits {
            max_expanded_bytes: 100_000,
            max_archive_entries: 1_000,
            ..ExtractionLimits::default()
        };
        let mut budget = Budget::new(&limits);
        let mut err = None;
        for name in entry_names(&mut archive, &mut budget).unwrap_or_default() {
            if let Err(e) = read_entry(&mut archive, &name, &mut budget) {
                err = Some(e);
                break;
            }
        }
        let err = err.expect("bomb must hit a budget");
        assert!(matches!(err, ExtractError::TooLarge(_)), "{err}");
    }

    #[test]
    fn entry_count_budget_stops_deep_archives() {
        let entries: Vec<(&str, &[u8])> = (0..40)
            .map(|i| {
                (
                    Box::leak(format!("e{i}").into_boxed_str()) as &str,
                    b"x" as &[u8],
                )
            })
            .collect();
        let bytes = zip_with(&entries);
        let mut archive = open(&bytes).unwrap();
        let limits = ExtractionLimits {
            max_archive_entries: 5,
            ..ExtractionLimits::default()
        };
        let mut budget = Budget::new(&limits);
        // Listing the directory stays bounded but non-fatal...
        assert_eq!(entry_names(&mut archive, &mut budget).unwrap().len(), 40);
        // ...while actually *reading* more than the entry budget fails.
        let mut err = None;
        for i in 0..40 {
            if let Err(e) = read_entry(&mut archive, &format!("e{i}"), &mut budget) {
                err = Some(e);
                break;
            }
        }
        let err = err.expect("reading past the entry cap must fail");
        assert_eq!(err.code(), "limit", "{err}");
    }

    #[test]
    fn reads_a_real_file_end_to_end() {
        let path = scratch_zip("t.xlsx", &[("xl/workbook.xml", b"<workbook/>")]);
        let bytes = std::fs::read(&path).unwrap();
        let mut archive = open(&bytes).unwrap();
        let mut budget = Budget::new(&ExtractionLimits::default());
        assert!(first_existing(
            entry_names(&mut archive, &mut budget).unwrap(),
            &["xl/workbook.xml"]
        )
        .is_some());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
