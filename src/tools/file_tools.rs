use memchr::memchr;
use memmap2::Mmap;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListFilesArgs {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListFilesResult {
    pub files: Vec<String>,
    pub directories: Vec<String>,
    pub truncated: bool,
}

impl ListFilesResult {
    pub fn total(&self) -> usize {
        self.files.len() + self.directories.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileArgs {
    pub path: String,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileResult {
    pub content: String,
    pub trimmed: bool,
    pub lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchArgs {
    pub query: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub files_scanned: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileArgs {
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub append: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileResult {
    pub bytes_written: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditFileArgs {
    pub path: String,
    pub old_string: String,
    pub new_string: String,
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditFileResult {
    pub replacements_made: usize,
}

const MAX_ENTRIES: usize = 400;
const MAX_HITS: usize = 60;
const MAX_SCAN_FILES: usize = 1_500;

#[derive(Clone)]
pub struct FileTools {
    /// Resolved once at construction: canonicalising on every call was pure syscall waste.
    root: PathBuf,
    max_file_bytes: usize,
}

impl FileTools {
    pub fn new(workspace: impl Into<PathBuf>, max_file_bytes: usize) -> Self {
        let workspace: PathBuf = workspace.into();
        let root = fs::canonicalize(&workspace).unwrap_or(workspace);
        Self {
            root,
            max_file_bytes,
        }
    }

    /// Resolves a user-supplied relative path and refuses anything outside the workspace.
    fn resolve(&self, path: Option<&str>) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
        let raw = path.unwrap_or(".");
        let joined = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.root.join(raw)
        };
        let normalized = normalize(&joined);
        if !normalized.starts_with(&self.root) {
            return Err("refused: path escapes the workspace".into());
        }
        Ok(normalized)
    }

    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    pub fn list_files(
        &self,
        args: ListFilesArgs,
    ) -> Result<ListFilesResult, Box<dyn Error + Send + Sync>> {
        let full_path = self.resolve(args.path.as_deref())?;

        let mut files = Vec::new();
        let mut directories = Vec::new();
        let mut truncated = false;

        for entry in fs::read_dir(&full_path)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                directories.push(format!("{name}/"));
            } else {
                files.push(name);
            }
            if files.len() + directories.len() >= MAX_ENTRIES {
                truncated = true;
                break;
            }
        }

        files.sort();
        directories.sort();

        Ok(ListFilesResult {
            files,
            directories,
            truncated,
        })
    }

    pub fn read_file(
        &self,
        args: ReadFileArgs,
    ) -> Result<ReadFileResult, Box<dyn Error + Send + Sync>> {
        use std::io::{BufReader, Read, Seek, SeekFrom};
        let full_path = self.resolve(Some(&args.path))?;
        let file_len = fs::metadata(&full_path)
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("{}: {e}", self.relative(&full_path)).into()
            })?
            .len() as usize;
        let offset = args.offset.unwrap_or(0).min(file_len);
        let budget = self.max_file_bytes.min(args.limit.unwrap_or(usize::MAX));

        // Stream only the requested window: a 4 KB slice of a 4 GB log must not
        // materialise the whole file. Behaviour matches the previous whole-file
        // read (same content, `trimmed`, `lines`; lossy UTF-8 at the edges).
        let window = budget.min(file_len - offset);
        let mut bytes = Vec::with_capacity(window.min(1024 * 1024));
        let mut file = fs::File::open(&full_path).map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("{}: {e}", self.relative(&full_path)).into()
        })?;
        file.seek(SeekFrom::Start(offset as u64))?;
        BufReader::new(file)
            .take(window as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("{}: {e}", self.relative(&full_path)).into()
            })?;

        let trimmed = offset + bytes.len() < file_len;

        let content = String::from_utf8_lossy(&bytes).into_owned();
        let lines = content.lines().count();

        Ok(ReadFileResult {
            content,
            trimmed,
            lines,
        })
    }

    pub fn search(&self, args: SearchArgs) -> Result<SearchResult, Box<dyn Error + Send + Sync>> {
        let start = self.resolve(args.path.as_deref())?;
        let mut hits = Vec::with_capacity(MAX_HITS);
        let mut files_scanned = 0usize;
        let mut truncated = false;
        let needle = args.query.to_ascii_lowercase();
        let needle_bytes = needle.as_bytes();
        let mut stack = vec![(start, 0usize)];

        while let Some((dir, depth)) = stack.pop() {
            if files_scanned >= MAX_SCAN_FILES || hits.len() >= MAX_HITS {
                truncated = true;
                break;
            }
            let Ok(read_dir) = fs::read_dir(&dir) else {
                continue;
            };
            let mut children: Vec<_> = read_dir.flatten().collect();
            children.sort_by_key(|e| e.file_name());

            for entry in children {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                let Ok(ft) = entry.file_type() else { continue };

                if ft.is_dir() {
                    if depth < 6 && !is_noise_dir(&name) {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                if is_noise_file(&name) {
                    continue;
                }
                files_scanned += 1;
                if files_scanned > MAX_SCAN_FILES {
                    truncated = true;
                    break;
                }

                // Use memory-mapped file for zero-copy access
                let Ok(file) = File::open(&path) else {
                    continue;
                };
                let Ok(metadata) = file.metadata() else {
                    continue;
                };
                if metadata.len() > self.max_file_bytes as u64 {
                    continue;
                }

                let bytes = unsafe { Mmap::map(&file) }.ok();
                let Some(bytes) = bytes else { continue };
                let bytes = &bytes[..];

                if bytes.is_empty() || looks_binary(bytes) {
                    continue;
                }

                // Quick rejection: use memmem to search for needle (case-insensitive via first bytes)
                // For miss case, this avoids full decode on non-matching files
                if needle_bytes.is_empty() {
                    continue;
                }

                // Fast two-byte check - if first two case-folded bytes don't appear in sequence, skip
                if needle_bytes.len() >= 2 {
                    let first = needle_bytes[0] | 32;
                    let second = needle_bytes[1] | 32;
                    let mut found = false;
                    for window in bytes.windows(2) {
                        if window[0] | 32 == first && window[1] | 32 == second {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        continue;
                    }
                } else {
                    // Single byte needle: check if it appears at all
                    let first = needle_bytes[0] | 32;
                    let mut found = false;
                    for &b in bytes {
                        if b | 32 == first {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        continue;
                    }
                }

                // Only decode if we found a candidate
                let text = String::from_utf8_lossy(bytes);
                let rel = self.relative(&path);

                // Line boundaries come from the DECODED string, never from the
                // raw bytes: lossy decoding expands invalid bytes into 3-byte
                // replacement characters, so raw-byte offsets drift and a
                // `&text[..]` slice can land mid-character (the abort this
                // fixes). '\n' is ASCII and cannot appear inside a multi-byte
                // sequence, so memchr on the decoded bytes finds the same
                // boundaries safely.
                let decoded = text.as_bytes();
                let mut line_start = 0usize;
                let mut line_no = 1usize;
                let mut search_pos = 0usize;

                while search_pos < decoded.len() {
                    // Find next newline
                    let line_end = memchr(b'\n', &decoded[search_pos..])
                        .map(|i| search_pos + i)
                        .unwrap_or(decoded.len());

                    let line_str = &text[line_start..line_end];
                    let matched = if !needle_bytes.is_empty() && line_str.is_ascii() {
                        contains_ci_ascii(line_str.as_bytes(), needle_bytes)
                    } else {
                        line_str.to_lowercase().contains(&needle)
                    };

                    if matched {
                        hits.push(SearchHit {
                            path: rel.clone(),
                            line: line_no,
                            text: line_str.trim_end().chars().take(200).collect(),
                        });
                        if hits.len() >= MAX_HITS {
                            truncated = true;
                            break;
                        }
                    }

                    line_start = line_end + 1;
                    search_pos = line_start;
                    line_no += 1
                }

                if hits.len() >= MAX_HITS {
                    truncated = true;
                    break;
                }
            }
        }

        hits.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        Ok(SearchResult {
            hits,
            files_scanned,
            truncated,
        })
    }

    pub fn write_file(
        &self,
        args: WriteFileArgs,
    ) -> Result<WriteFileResult, Box<dyn Error + Send + Sync>> {
        let full_path = self.resolve(Some(&args.path))?;

        // Open file with appropriate options (create if doesn't exist)
        let mut options = fs::OpenOptions::new();
        options.create(true);
        if args.append {
            options.append(true);
        } else {
            options.write(true).truncate(true);
        }

        let mut file = options.open(&full_path)?;
        let bytes = args.content.as_bytes();
        file.write_all(bytes)?;

        Ok(WriteFileResult {
            bytes_written: bytes.len(),
            truncated: false, // We don't truncate the content we write
        })
    }

    pub fn edit_file(
        &self,
        args: EditFileArgs,
    ) -> Result<EditFileResult, Box<dyn Error + Send + Sync>> {
        let full_path = self.resolve(Some(&args.path))?;
        let mut content = fs::read_to_string(&full_path)?;

        let mut replacements_made = 0;
        if args.replace_all {
            // Replace all occurrences
            while let Some(pos) = content.find(&args.old_string) {
                content.replace_range(pos..pos + args.old_string.len(), &args.new_string);
                replacements_made += 1;
            }
        } else {
            // Replace only first occurrence
            if let Some(pos) = content.find(&args.old_string) {
                content.replace_range(pos..pos + args.old_string.len(), &args.new_string);
                replacements_made = 1;
            }
        }

        fs::write(&full_path, content)?;

        Ok(EditFileResult { replacements_made })
    }
}

/// Lexical normalisation: no filesystem access, so it is cheap and total.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn is_noise_dir(name: &str) -> bool {
    matches!(
        name,
        "target" | "node_modules" | ".git" | "dist" | "build" | "vendor" | ".next"
    )
}

fn is_noise_file(name: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    matches!(
        ext,
        "lock" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "zip" | "gz" | "pdf"
    ) || name.starts_with('.')
}

fn looks_binary(bytes: &[u8]) -> bool {
    let probe = &bytes[..bytes.len().min(1024)];
    probe.contains(&0)
        || probe
            .iter()
            .filter(|b| **b < 9 || (**b > 13 && **b < 32))
            .count()
            > probe.len() / 10
}

/// Allocation-free case-insensitive substring search for ASCII haystacks.
///
/// Semantics are byte-identical to `hay.to_lowercase().contains(needle)` when
/// `hay` is pure ASCII (callers gate on `line.is_ascii()`): ASCII-fold equals
/// Unicode lowercase for ASCII, and a non-ASCII needle can never match an
/// ASCII haystack under either formulation. The probe byte uses `| 32`, which
/// can only produce false positives (never false negatives); the verify uses
/// an exact A–Z fold so control characters (`\r` vs `-`, `0x10` vs `0`) are
/// never confused.
#[inline]
fn fold_ascii(b: u8) -> u8 {
    if b.wrapping_sub(b'A') < 26 {
        b | 32
    } else {
        b
    }
}

fn contains_ci_ascii(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    let n = needle.len();
    if n > hay.len() {
        return false;
    }

    // Pre-fold the needle for comparison
    let folded_needle: Vec<u8> = needle.iter().map(|&b| fold_ascii(b)).collect();

    let mut pos = 0;
    let last = hay.len() - n;

    while pos <= last {
        // Find next candidate by manually scanning (memchr can't do case-insensitive)
        // We use a quick check: look for any byte whose folded value matches folded_needle[0]
        let target = folded_needle[0];
        let mut found = false;
        let mut i = pos;
        while i <= last {
            if fold_ascii(hay[i]) == target {
                pos = i;
                found = true;
                break;
            }
            i += 1;
        }
        if !found {
            break;
        }

        // Verify the rest of the needle
        let window = &hay[pos..pos + n];
        if window
            .iter()
            .zip(&folded_needle)
            .all(|(h, w)| fold_ascii(*h) == *w)
        {
            return true;
        }
        pos += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_removes_traversal() {
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(normalize(Path::new("/a/../../etc")), PathBuf::from("/etc"));
    }

    #[test]
    fn rejects_paths_outside_workspace() {
        let tools = FileTools::new(std::env::temp_dir(), 1024);
        let err = tools.resolve(Some("../etc/passwd")).unwrap_err();
        assert!(err.to_string().contains("escapes the workspace"));
    }

    /// The allocation-free ASCII matcher must agree with the old
    /// `hay.to_lowercase().contains(&needle.to_lowercase())` semantics.
    #[test]
    fn contains_ci_ascii_matches_reference() {
        let cases: [(&str, &str); 15] = [
            ("The Model Registry", "model"),
            ("MODEL registry", "model"),
            ("nothing here", "model"),
            ("mo", "model"),
            ("model", "model"),
            ("a model", ""),
            ("123 models!", "MODEL"),
            ("", "model"),
            ("", ""),
            ("mod el", "model"),
            ("mode", "model"),
            ("MODELLING", "model"),
            ("m0del", "model"),
            ("a\r-b", "-b"), // control char must not fold onto '-'
            ("\x10 x", "0"), // 0x10|32 == '0'|32: probe hit, verify rejects
        ];
        for (hay, needle) in &cases {
            let fast = contains_ci_ascii(hay.as_bytes(), needle.as_bytes());
            let reference = hay.to_lowercase().contains(&needle.to_lowercase());
            assert_eq!(
                fast, reference,
                "fast path diverges from reference for {hay:?} / {needle:?}"
            );
        }
    }

    /// End-to-end: case-insensitive hits still found through the public tool,
    /// with the hit text carrying the file's original casing.
    #[test]
    fn search_survives_invalid_utf8_and_multibyte_files() {
        // Regression: the search walk sliced the lossy-decoded string with
        // raw-byte offsets - invalid bytes expand into 3-byte replacement
        // characters, the offsets drifted mid-glyph, and the abort took the
        // whole app down.
        let dir = std::env::temp_dir().join(format!("nanomite-search-utf8-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut broken = b"ok ascii line\nbroken \xff\xfe marker\n".to_vec();
        broken.extend_from_slice("needle here\n".as_bytes());
        broken.extend_from_slice(b"more \x80 stuff\n");
        fs::write(dir.join("broken.txt"), &broken).unwrap();
        fs::write(
            dir.join("cjk.txt"),
            "\u{65e5}\u{672c}\u{8a9e} \u{30c6}\u{30b9}\u{30c8} needle\n".as_bytes(),
        )
        .unwrap();
        let tools = FileTools::new(dir.clone(), 1024 * 1024);
        let result = tools
            .search(SearchArgs {
                query: "needle".into(),
                path: None,
            })
            .expect("search must not panic on non-utf8 files");
        assert_eq!(result.hits.len(), 2);
        assert_eq!(result.hits[0].line, 3, "ascii needle line");
        assert_eq!(result.hits[1].line, 1, "cjk needle line");

        // Non-ASCII needle exercises the lowercase fallback path.
        let result = tools
            .search(SearchArgs {
                query: "\u{30c6}\u{30b9}\u{30c8}".into(),
                path: None,
            })
            .expect("search");
        assert_eq!(result.hits.len(), 1);
        assert!(result.hits[0].text.contains("\u{30c6}\u{30b9}\u{30c8}"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_finds_case_insensitive_hits() {
        let dir = std::env::temp_dir().join(format!("nanomite-search-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("a.txt"),
            "The QUICK brown Fox\nnothing to see\nQUOKKA\n",
        )
        .unwrap();
        let tools = FileTools::new(dir.clone(), 1024 * 1024);
        let result = tools
            .search(SearchArgs {
                query: "qu".into(),
                path: None,
            })
            .unwrap();
        assert_eq!(result.hits.len(), 2);
        assert_eq!(result.hits[0].line, 1);
        assert_eq!(result.hits[0].text, "The QUICK brown Fox");
        assert_eq!(result.hits[1].line, 3);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Whole-file lowercasing keeps line structure: every line of the lowered
    /// text pairs with the same-index original line (the invariant the search
    /// fast path relies on).
    #[test]
    fn lowercasing_preserves_line_structure() {
        let cases = [
            "The QUICK brown Fox\nnothing to see\nQUOKKA\n",
            "«» une ligne UNICODE ÜBER Alles — идёт дождь\nİstanbul ΣΣΣ end\nplain\n",
            "Σ at start\nmid ΣΣ mid\nend Σ",
            "",
            "\n\n\n",
        ];
        for case in &cases {
            let lower = case.to_lowercase();
            assert_eq!(
                case.lines().count(),
                lower.lines().count(),
                "line count diverged for {case:?}"
            );
            for (orig, low) in case.lines().zip(lower.lines()) {
                assert_eq!(
                    orig.to_lowercase(),
                    low,
                    "per-line lowercase diverged from whole-text lowercase for {case:?}"
                );
            }
        }
    }
}
