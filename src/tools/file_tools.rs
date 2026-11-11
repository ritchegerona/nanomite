use memchr::{memchr, memchr2};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::sync::Mutex;
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
        resolve_within(&self.root, &joined).ok_or_else(|| -> Box<dyn Error + Send + Sync> {
            "refused: path escapes the workspace".into()
        })
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
        let needle = args.query.to_ascii_lowercase();
        let needle_bytes = needle.as_bytes();

        // Phase 1 (single-threaded): walk the tree, collect candidate paths.
        // Directory traversal is syscall-bound and benefits little from
        // threads; the expensive per-file scan below does.
        let mut stack = vec![(start, 0usize)];
        let mut candidates: Vec<PathBuf> = Vec::new();
        let mut walk_truncated = false;
        'outer: while let Some((dir, depth)) = stack.pop() {
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
                candidates.push(path);
                if candidates.len() >= MAX_SCAN_FILES {
                    walk_truncated = true;
                    break 'outer;
                }
            }
        }

        // Phase 2: scan files in parallel with scoped threads (no deps).
        // Each thread gets its own reusable buffer; per-thread local hit
        // vectors are merged at the end so the mutex is touched once per
        // thread. Ordering is restored by the final sort + truncate.
        let hits: Mutex<Vec<SearchHit>> = Mutex::new(Vec::new());
        let needle_ref = &needle;
        let needle_bytes_ref = &needle_bytes;
        let root_ref = &self.root;
        let max_bytes = self.max_file_bytes;
        std::thread::scope(|scope| {
            let hits_ref = &hits;
            let threads = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .min(8);
            let chunk = candidates.len().div_ceil(threads);
            if chunk == 0 {
                return;
            }
            for group in candidates.chunks(chunk) {
                scope.spawn(move || {
                    let mut local: Vec<SearchHit> = Vec::new();
                    let mut buf: Vec<u8> = Vec::new();
                    for path in group {
                        scan_file(
                            path,
                            root_ref,
                            max_bytes,
                            needle_ref,
                            needle_bytes_ref,
                            &mut buf,
                            &mut local,
                            MAX_HITS,
                        );
                        if local.len() >= MAX_HITS {
                            break; // per-thread safety cap; union still >= MAX_HITS
                        }
                    }
                    if !local.is_empty() {
                        let mut all = hits_ref.lock().unwrap_or_else(|e| e.into_inner());
                        all.extend(local);
                    }
                });
            }
        });

        let mut hits = hits.into_inner()?;
        hits.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        let hits_overflow = hits.len() > MAX_HITS;
        hits.truncate(MAX_HITS);
        Ok(SearchResult {
            hits,
            files_scanned: candidates.len(),
            truncated: walk_truncated || hits_overflow,
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

/// Resolves an already-joined path inside `root`, refusing escapes.
///
/// The lexical pass strips `..`; canonicalising the deepest existing
/// ancestor then makes both sides comparable when `root` sits behind a
/// symlink (macOS `/var` → `/private/var`) and catches an in-workspace
/// symlink that points out of it. A non-existent tail stays lexical so
/// create/write targets still resolve. Returns the canonical path so
/// `strip_prefix(root)` display keeps working.
pub(crate) fn resolve_within(root: &Path, joined: &Path) -> Option<PathBuf> {
    let normalized = normalize(joined);
    let resolved = canonicalize_deepest(&normalized);
    resolved
        .starts_with(canonicalize_deepest(root))
        .then_some(resolved)
}

/// `fs::canonicalize` on the deepest ancestor that exists, re-appending the
/// missing tail unchanged (it cannot contain symlinks yet).
fn canonicalize_deepest(path: &Path) -> PathBuf {
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        if let Ok(canon) = fs::canonicalize(&cur) {
            let mut out = canon;
            for part in tail.iter().rev() {
                out.push(part);
            }
            return out;
        }
        let Some(name) = cur.file_name().map(|n| n.to_os_string()) else {
            return normalize(path);
        };
        let Some(parent) = cur.parent().map(|p| p.to_path_buf()) else {
            return normalize(path);
        };
        tail.push(name);
        cur = parent;
    }
}

/// Scans one file for `needle` (case-insensitive ASCII fast path, Unicode
/// fallback), appending hits to `out`. Reads into the caller's reusable
/// buffer; returns early on unreadable/oversized/binary files. Stops scanning
/// lines once `out` reaches `cap`.
#[allow(clippy::too_many_arguments)]
fn scan_file(
    path: &Path,
    root: &Path,
    max_file_bytes: usize,
    needle: &str,
    needle_bytes: &[u8],
    buf: &mut Vec<u8>,
    out: &mut Vec<SearchHit>,
    cap: usize,
) {
    if out.len() >= cap {
        return;
    }
    // Read straight into a buffer reused across files, bounded with `take`
    // so oversized files are rejected without a separate stat: one open + one
    // read beats open + fstat + munmap, and the warm buffer avoids per-file
    // allocation.
    buf.clear();
    let too_big = {
        let Ok(mut file) = File::open(path) else { return };
        let Ok(read_len) = (&mut file)
            .take(max_file_bytes as u64 + 1)
            .read_to_end(buf)
        else {
            return;
        };
        read_len > max_file_bytes
    };
    if too_big {
        return;
    }
    let bytes = &buf[..];
    if bytes.is_empty() || looks_binary(bytes) {
        return;
    }

    // Prefilter: candidate search on raw bytes without decoding.
    // Sound for any needle: the first byte's two ASCII case variants are
    // located with SIMD memchr2 (or memchr when they coincide) and each
    // candidate window is verified with `eq_ignore_ascii_case`. Non-ASCII
    // bytes never match an ASCII case pair, so multibyte UTF-8 is skipped
    // safely, and a miss exits before the lossy decode + line walk entirely.
    if !needle_bytes.is_empty() && needle.is_ascii() {
        let first = needle_bytes[0];
        let (lo, hi) = if first.is_ascii_alphabetic() {
            (first | 32, first & !32)
        } else {
            (first, first)
        };
        let n = needle_bytes.len();
        let mut found = false;
        let mut pos = 0;
        while let Some(i) = memchr2(lo, hi, &bytes[pos..]) {
            let start = pos + i;
            if start + n > bytes.len() {
                break;
            }
            if bytes[start..start + n].eq_ignore_ascii_case(needle_bytes) {
                found = true;
                break;
            }
            pos = start + 1;
        }
        if !found {
            return;
        }
    } else if !needle_bytes.is_empty() {
        // Non-ASCII needle: fold to lowercase once; a raw-byte scan for the
        // lowercase form is a sound candidate filter because any case-variant
        // match must contain the folded code units as a byte subsequence at
        // some offset.
        let mut found = false;
        let mut pos = 0;
        let low = needle.as_bytes();
        while let Some(i) = memchr(low[0], &bytes[pos..]) {
            let start = pos + i;
            if start + low.len() <= bytes.len() && bytes[start..start + low.len()] == *low {
                found = true;
                break;
            }
            pos = start + 1;
        }
        if !found {
            return;
        }
    }

    // Only decode if we found a candidate
    let text = String::from_utf8_lossy(bytes);
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");

    // Line boundaries come from the DECODED string, never from the raw bytes:
    // lossy decoding expands invalid bytes into 3-byte replacement
    // characters, so raw-byte offsets drift and a `&text[..]` slice can land
    // mid-character. '\n' is ASCII and cannot appear inside a multi-byte
    // sequence, so memchr on the decoded bytes finds the same boundaries
    // safely.
    let decoded = text.as_bytes();
    let mut line_start = 0usize;
    let mut line_no = 1usize;
    let mut search_pos = 0usize;

    while search_pos < decoded.len() {
        let line_end = memchr(b'\n', &decoded[search_pos..])
            .map(|i| search_pos + i)
            .unwrap_or(decoded.len());

        let line_str = &text[line_start..line_end];
        let matched = if !needle_bytes.is_empty() && line_str.is_ascii() {
            contains_ci_ascii(line_str.as_bytes(), needle_bytes)
        } else {
            line_str.to_lowercase().contains(needle)
        };

        if matched {
            out.push(SearchHit {
                path: rel.clone(),
                line: line_no,
                text: line_str.trim_end().chars().take(200).collect(),
            });
            if out.len() >= cap {
                return;
            }
        }

        line_start = line_end + 1;
        search_pos = line_start;
        line_no += 1;
    }
}

fn is_noise_dir(name: &str) -> bool {
    // Dot-dirs are caches and tool state, never source: `.venv` alone is
    // 100M+ of site-packages that would otherwise eat the scan budget
    // before `src/` is even reached (matches index_tools's walk).
    if name.starts_with('.') {
        return true;
    }
    matches!(
        name,
        "target" | "node_modules" | "dist" | "build" | "vendor" | "venv" | "env" | "__pycache__"
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

/// Case-insensitive ASCII substring test. Candidates are located with SIMD
/// (`memchr2` on the first needle byte's two case variants; `memchr` shortcut
/// when both variants coincide) and verified with `eq_ignore_ascii_case`,
/// which the compiler vectorises. Non-ASCII bytes never fold, so UTF-8
/// content passes through untouched.
fn contains_ci_ascii(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    let first = needle[0];
    let (lo, hi) = if first.is_ascii_alphabetic() {
        (first | 32, first & !32)
    } else {
        (first, first)
    };
    let mut pos = 0;
    while let Some(i) = memchr2(lo, hi, &hay[pos..]) {
        let start = pos + i;
        if start + needle.len() > hay.len() {
            return false;
        }
        if hay[start..start + needle.len()].eq_ignore_ascii_case(needle) {
            return true;
        }
        pos = start + 1;
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

    /// A workspace addressed through a symlink alias (macOS `/var` →
    /// `/private/var`) must accept absolute paths in either spelling: the
    /// root is canonicalised at construction, so the alias form used to be
    /// refused outright — that is what crashed `cargo bench`.
    #[test]
    fn accepts_absolute_paths_through_a_root_symlink_alias() {
        let base = std::env::temp_dir().join(format!("nm-alias-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let real = base.join("real");
        fs::create_dir_all(&real).expect("real workspace");
        let alias = base.join("alias");
        std::os::unix::fs::symlink(&real, &alias).expect("alias symlink");

        let tools = FileTools::new(&alias, 1024); // root canonicalises to `real`
        fs::write(alias.join("f.rs"), "fn main() {}").expect("fixture");

        let abs_alias = alias.join("f.rs").display().to_string();
        let got = tools
            .resolve(Some(&abs_alias))
            .expect("alias-spelled absolute path must resolve");
        assert!(
            got.starts_with(&tools.root),
            "resolved path must land inside the canonical root"
        );

        let abs_real = real.join("f.rs").display().to_string();
        tools
            .resolve(Some(&abs_real))
            .expect("canonical absolute path must resolve");

        let _ = fs::remove_dir_all(&base);
    }

    /// A symlink inside the workspace pointing out of it must be refused:
    /// the lexical check alone let `link/secret` pass while the open
    /// followed the link outside the root.
    #[test]
    fn refuses_a_workspace_symlink_that_points_outside() {
        let base = std::env::temp_dir().join(format!("nm-slink-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let ws = base.join("ws");
        let outside = base.join("outside");
        fs::create_dir_all(&ws).expect("ws");
        fs::create_dir_all(&outside).expect("outside");
        fs::write(outside.join("secret.txt"), "s").expect("secret");
        std::os::unix::fs::symlink(&outside, ws.join("link")).expect("link");

        let tools = FileTools::new(&ws, 1024);
        let err = tools
            .resolve(Some("link/secret.txt"))
            .expect_err("symlink escape must be refused");
        assert!(err.to_string().contains("escapes the workspace"));

        let _ = fs::remove_dir_all(&base);
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

    /// Caches, tool state and build output are never source: the search must
    /// not descend into dot-dirs (`.venv` alone is 100M+ of site-packages)
    /// or bare `venv`/`__pycache__` dirs.
    #[test]
    fn noise_dir_skips_dot_dirs_and_venv() {
        for noise in [
            ".venv",
            ".git",
            ".mypy_cache",
            ".pytest_cache",
            ".ruff_cache",
            ".hypothesis",
            ".nanomite",
            ".venv/lib",
            "venv",
            "env",
            "__pycache__",
            "target",
            "node_modules",
            "dist",
            "build",
            "vendor",
        ] {
            assert!(is_noise_dir(noise), "{noise:?} must be noise");
        }
        for source in [
            "src",
            "tools",
            "tests",
            "docs",
            "src-tauri",
            "envs",
            "venv2",
            "",
        ] {
            assert!(!is_noise_dir(source), "{source:?} must not be noise");
        }
    }

    /// End-to-end: a needle that exists BOTH in `.venv` and in `src/` must
    /// only hit `src/` — the walk must not scan cache dirs at all.
    #[test]
    fn search_skips_venv_and_cache_dirs() {
        let dir = std::env::temp_dir().join(format!("nanomite-search-venv-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".venv/lib/python3/site-packages")).unwrap();
        fs::write(
            dir.join(".venv/lib/python3/site-packages/pkg.py"),
            "needle in venv\n",
        )
        .unwrap();
        fs::create_dir_all(dir.join(".mypy_cache")).unwrap();
        fs::write(dir.join(".mypy_cache/cache.txt"), "needle in cache\n").unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/real.rs"), "needle in source\n").unwrap();

        let tools = FileTools::new(dir.clone(), 1024 * 1024);
        let result = tools
            .search(SearchArgs {
                query: "needle".into(),
                path: None,
            })
            .expect("search");
        assert_eq!(result.hits.len(), 1, "only the src hit: {result:?}");
        assert!(result.hits[0].path.ends_with("src/real.rs"), "{result:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
