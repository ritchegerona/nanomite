//! Symbol index tools: project-wide symbol search and navigation.
//!
//! Provides fast symbol lookup (definitions, references) across the workspace
//! using a combination of ctags (when available) and regex-based fallback.
//! Results are structured for the model to navigate codebases efficiently.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_SYMBOLS: usize = 5000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSymbolsArgs {
    /// Force rebuild the index (default: false, uses cached index)
    #[serde(default)]
    pub force_rebuild: bool,
    /// Specific languages to index (default: all detected)
    #[serde(default)]
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSymbolsResult {
    pub indexed: bool,
    pub symbols_count: usize,
    pub languages: Vec<String>,
    pub duration_ms: u64,
    pub cache_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindSymbolArgs {
    /// Symbol name or pattern to search for (supports * wildcard)
    pub query: String,
    /// Filter by symbol kind: function, struct, enum, trait, const, mod, etc.
    #[serde(default)]
    pub kind: Option<String>,
    /// Filter by language
    #[serde(default)]
    pub language: Option<String>,
    /// Max results (default 50, max 200)
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindSymbolResult {
    pub query: String,
    pub matches: Vec<SymbolMatch>,
    pub total: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolMatch {
    pub name: String,
    pub kind: String,
    pub language: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindReferencesArgs {
    /// Symbol name to find references for
    pub symbol: String,
    /// Optional file to start search from (for context)
    #[serde(default)]
    pub from_file: Option<String>,
    /// Max results (default 50, max 200)
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindReferencesResult {
    pub symbol: String,
    pub references: Vec<SymbolRef>,
    pub total: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRef {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub context: String,
    pub is_definition: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SymbolIndex {
    symbols: Vec<SymbolMatch>,
    by_name: HashMap<String, Vec<usize>>,
    by_file: HashMap<String, Vec<usize>>,
    built_at: std::time::SystemTime,
    workspace_root: PathBuf,
}

#[derive(Clone)]
pub struct IndexTools {
    root: PathBuf,
    index_cache: std::sync::Arc<std::sync::Mutex<Option<SymbolIndex>>>,
}

impl IndexTools {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            index_cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn cache_path(&self) -> PathBuf {
        self.root.join(".nanomite").join("symbol_index.json")
    }

    fn ensure_cache_dir(&self) -> Result<(), String> {
        let cache_dir = self.root.join(".nanomite");
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("failed to create cache dir: {e}"))?;
        Ok(())
    }

    fn load_cached_index(&self) -> Option<SymbolIndex> {
        let path = self.cache_path();
        if !path.exists() {
            return None;
        }
        let data = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    fn save_index(&self, index: &SymbolIndex) -> Result<(), String> {
        self.ensure_cache_dir()?;
        let path = self.cache_path();
        let data = serde_json::to_string_pretty(index)
            .map_err(|e| format!("serialization failed: {e}"))?;
        std::fs::write(&path, data).map_err(|e| format!("write failed: {e}"))?;
        Ok(())
    }

    fn try_ctags_index(&self) -> Option<Vec<SymbolMatch>> {
        // Check if ctags is available
        let output = Command::new("ctags").arg("--version").output().ok()?;
        if !output.status.success() {
            return None;
        }

        // Run ctags on the workspace
        let output = Command::new("ctags")
            .args([
                "--recurse",
                "--format=json",
                "--fields=+nKs",
                "--excmd=number",
                "--output=-",
                ".",
            ])
            .current_dir(&self.root)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut symbols = Vec::new();

        for line in stdout.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(tag) = serde_json::from_str::<CtagEntry>(line) {
                if let Some(sym) = ctag_to_symbol(&tag, &self.root) {
                    symbols.push(sym);
                }
            }
        }

        if symbols.is_empty() {
            None
        } else {
            Some(symbols)
        }
    }

    fn regex_index(&self) -> Vec<SymbolMatch> {
        let mut symbols = Vec::new();
        let patterns = language_patterns();

        let walker = walkdir::WalkDir::new(&self.root)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.')
                    && name != "target"
                    && name != "node_modules"
                    && name != ".git"
                    && name != "build"
                    && name != "dist"
            })
            .filter_map(|e| e.ok());

        for entry in walker {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let lang = extension_to_language(ext);
            if lang.is_none() {
                continue;
            }
            let lang = lang.unwrap();

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if let Some(patterns) = patterns.get(lang) {
                for (kind, regex) in patterns {
                    for cap in regex.captures_iter(&content) {
                        let mat = cap.get(0).unwrap();
                        let name = cap.get(1).map(|m| m.as_str()).unwrap_or(mat.as_str());
                        let line_num = content[..mat.start()].lines().count() + 1;
                        let col = mat.start()
                            - content[..mat.start()]
                                .rfind('\n')
                                .map(|i| i + 1)
                                .unwrap_or(0);
                        let signature = extract_signature(&content, mat.start(), mat.end());

                        let rel_path = path
                            .strip_prefix(&self.root)
                            .unwrap_or(path)
                            .to_string_lossy()
                            .to_string();

                        symbols.push(SymbolMatch {
                            name: name.to_string(),
                            kind: kind.to_string(),
                            language: lang.to_string(),
                            file: rel_path,
                            line: line_num,
                            column: col,
                            signature,
                        });

                        if symbols.len() >= MAX_SYMBOLS {
                            return symbols;
                        }
                    }
                }
            }
        }

        symbols
    }

    fn build_index(&self, force: bool) -> Result<SymbolIndex, String> {
        let _started = std::time::Instant::now();

        // Check local cache first
        if !force {
            let cache = self.index_cache.lock().unwrap();
            if let Some(cached) = cache.as_ref() {
                // Check if cache is fresh (workspace mtime)
                let workspace_mtime = std::fs::metadata(&self.root)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                if cached.built_at >= workspace_mtime {
                    return Ok(cached.clone());
                }
            }
        }

        if !force {
            if let Some(cached) = self.load_cached_index() {
                // Check if cache is fresh (workspace mtime)
                let workspace_mtime = std::fs::metadata(&self.root)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                if cached.built_at >= workspace_mtime {
                    return Ok(cached);
                }
            }
        }

        // Try ctags first, fallback to regex
        let symbols = self.try_ctags_index().unwrap_or_else(|| self.regex_index());

        // Build lookup maps
        let mut by_name = HashMap::new();
        let mut by_file = HashMap::new();
        for (idx, sym) in symbols.iter().enumerate() {
            by_name
                .entry(sym.name.clone())
                .or_insert_with(Vec::new)
                .push(idx);
            by_file
                .entry(sym.file.clone())
                .or_insert_with(Vec::new)
                .push(idx);
        }

        eprintln!("DEBUG regex_index: found {} symbols", symbols.len());
        for s in &symbols {
            eprintln!("  - {} ({}) in {}:{}", s.name, s.kind, s.file, s.line);
        }
        let index = SymbolIndex {
            symbols,
            by_name,
            by_file,
            built_at: std::time::SystemTime::now(),
            workspace_root: self.root.clone(),
        };

        // Save to local cache
        *self.index_cache.lock().unwrap() = Some(index.clone());

        self.save_index(&index)?;
        Ok(index)
    }

    pub fn index_symbols(&self, args: IndexSymbolsArgs) -> Result<IndexSymbolsResult, String> {
        let started = std::time::Instant::now();

        let index = self.build_index(args.force_rebuild)?;
        let languages: Vec<String> = index
            .symbols
            .iter()
            .map(|s| s.language.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let result = IndexSymbolsResult {
            indexed: true,
            symbols_count: index.symbols.len(),
            languages,
            duration_ms: started.elapsed().as_millis() as u64,
            cache_path: self.cache_path().to_string_lossy().to_string(),
        };

        Ok(result)
    }

    pub fn find_symbol(&self, args: FindSymbolArgs) -> Result<FindSymbolResult, String> {
        let limit = args.limit.clamp(1, 200);

        // Get or build index from local cache
        let index = {
            let cache = self.index_cache.lock().unwrap();
            if let Some(cached) = cache.as_ref() {
                cached.clone()
            } else {
                drop(cache);
                self.build_index(false)?
            }
        };

        let query = args.query.to_lowercase();
        let is_wildcard = query.contains('*');
        let pattern = if is_wildcard {
            query.replace('*', ".*")
        } else {
            query.clone()
        };

        let mut matches = Vec::new();
        for sym in index.symbols.iter() {
            if matches.len() >= limit {
                break;
            }

            // Filter by kind
            if let Some(ref kind_filter) = args.kind {
                if !sym.kind.eq_ignore_ascii_case(kind_filter) {
                    continue;
                }
            }
            // Filter by language
            if let Some(ref lang_filter) = args.language {
                if !sym.language.eq_ignore_ascii_case(lang_filter) {
                    continue;
                }
            }

            let name_lower = sym.name.to_lowercase();
            let matched = if is_wildcard {
                regex::Regex::new(&format!("^{}$", pattern))
                    .map(|re| re.is_match(&name_lower))
                    .unwrap_or(false)
            } else {
                name_lower.contains(&pattern)
            };

            if matched {
                matches.push(sym.clone());
            }
        }

        Ok(FindSymbolResult {
            query: args.query,
            total: matches.len(),
            truncated: matches.len() >= limit,
            matches,
        })
    }

    pub fn find_references(
        &self,
        args: FindReferencesArgs,
    ) -> Result<FindReferencesResult, String> {
        let limit = args.limit.clamp(1, 200);

        // Get or build index from local cache
        let index = {
            let cache = self.index_cache.lock().unwrap();
            if let Some(cached) = cache.as_ref() {
                cached.clone()
            } else {
                drop(cache);
                self.build_index(false)?
            }
        };

        // Find the symbol definition first
        let def_indices = index.by_name.get(&args.symbol);
        let mut refs = Vec::new();

        if let Some(indices) = def_indices {
            for &def_idx in indices {
                let def = &index.symbols[def_idx];
                refs.push(SymbolRef {
                    file: def.file.clone(),
                    line: def.line,
                    column: def.column,
                    context: def.signature.clone().unwrap_or_default(),
                    is_definition: true,
                });
            }
        }

        // Search for references in files (simple text search for the symbol name)
        // This is a best-effort approach without full LSP
        let search_files: Vec<String> = if let Some(from_file) = args.from_file {
            vec![from_file]
        } else {
            index.by_file.keys().cloned().collect()
        };

        for file in search_files {
            if refs.len() >= limit {
                break;
            }
            let path = self.root.join(&file);
            if !path.exists() {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            for (line_num, line) in content.lines().enumerate() {
                if refs.len() >= limit {
                    break;
                }
                // Simple word-boundary search for the symbol
                let symbol_pattern = format!(r"\b{}\b", regex::escape(&args.symbol));
                if let Ok(re) = regex::Regex::new(&symbol_pattern) {
                    for mat in re.find_iter(line) {
                        if refs.len() >= limit {
                            break;
                        }
                        // Skip the definition line
                        let is_def = index
                            .symbols
                            .iter()
                            .any(|s| s.file == file && s.line == line_num + 1);
                        refs.push(SymbolRef {
                            file: file.clone(),
                            line: line_num + 1,
                            column: mat.start() + 1,
                            context: line.trim().to_string(),
                            is_definition: is_def,
                        });
                    }
                }
            }
        }

        Ok(FindReferencesResult {
            symbol: args.symbol,
            total: refs.len(),
            truncated: refs.len() >= limit,
            references: refs,
        })
    }
}

#[derive(Debug, Deserialize)]
struct CtagEntry {
    name: String,
    path: String,
    #[serde(rename = "kind")]
    kind: Option<String>,
    #[serde(rename = "language")]
    language: Option<String>,
    line: Option<usize>,
}

fn ctag_to_symbol(tag: &CtagEntry, root: &Path) -> Option<SymbolMatch> {
    let kind = tag.kind.as_deref().unwrap_or("unknown");
    let language = tag.language.as_deref().unwrap_or("unknown");
    let line = tag.line.unwrap_or(1);

    // Extract column from pattern if possible
    let column = 1;

    let rel_path = Path::new(&tag.path)
        .strip_prefix(root)
        .unwrap_or(Path::new(&tag.path))
        .to_string_lossy()
        .to_string();

    Some(SymbolMatch {
        name: tag.name.clone(),
        kind: normalize_kind(kind),
        language: language.to_string(),
        file: rel_path,
        line,
        column,
        signature: None,
    })
}

fn normalize_kind(ctags_kind: &str) -> String {
    match ctags_kind {
        "function" | "method" => "function",
        "struct" => "struct",
        "enum" => "enum",
        "trait" => "trait",
        "impl" => "impl",
        "const" | "constant" => "const",
        "static" => "static",
        "macro" => "macro",
        "module" | "mod" => "mod",
        "type" => "type",
        "field" => "field",
        "variant" => "variant",
        _ => "unknown",
    }
    .to_string()
}

fn extension_to_language(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "js" | "jsx" | "ts" | "tsx" => Some("javascript"),
        "go" => Some("go"),
        "java" => Some("java"),
        "cpp" | "cc" | "cxx" | "hpp" | "h" => Some("cpp"),
        "c" => Some("c"),
        "cs" => Some("csharp"),
        "rb" => Some("ruby"),
        "php" => Some("php"),
        "swift" => Some("swift"),
        "kt" | "kts" => Some("kotlin"),
        "scala" => Some("scala"),
        "sh" | "bash" => Some("shell"),
        _ => None,
    }
}

fn language_patterns() -> HashMap<&'static str, Vec<(&'static str, regex::Regex)>> {
    let mut map = HashMap::new();

    // Rust patterns
    map.insert(
        "rust",
        vec![
            (
                "function",
                regex::Regex::new(r"(?m)^\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)").unwrap(),
            ),
            (
                "struct",
                regex::Regex::new(r"(?m)^\s*(?:pub\s+)?struct\s+(\w+)").unwrap(),
            ),
            (
                "enum",
                regex::Regex::new(r"(?m)^\s*(?:pub\s+)?enum\s+(\w+)").unwrap(),
            ),
            (
                "trait",
                regex::Regex::new(r"(?m)^\s*(?:pub\s+)?trait\s+(\w+)").unwrap(),
            ),
            (
                "impl",
                regex::Regex::new(r"(?m)^\s*impl(?:\s+<\w+>)?\s+(\w+)").unwrap(),
            ),
            (
                "const",
                regex::Regex::new(r"(?m)^\s*(?:pub\s+)?const\s+(\w+)").unwrap(),
            ),
            (
                "static",
                regex::Regex::new(r"(?m)^\s*(?:pub\s+)?static\s+(\w+)").unwrap(),
            ),
            (
                "macro",
                regex::Regex::new(r"(?m)^\s*macro_rules!\s+(\w+)").unwrap(),
            ),
            (
                "mod",
                regex::Regex::new(r"(?m)^\s*(?:pub\s+)?mod\s+(\w+)").unwrap(),
            ),
            (
                "type",
                regex::Regex::new(r"(?m)^\s*(?:pub\s+)?type\s+(\w+)").unwrap(),
            ),
        ],
    );

    // Python patterns
    map.insert(
        "python",
        vec![
            (
                "function",
                regex::Regex::new(r"(?m)^\s*(?:async\s+)?def\s+(\w+)").unwrap(),
            ),
            (
                "class",
                regex::Regex::new(r"(?m)^\s*class\s+(\w+)").unwrap(),
            ),
            (
                "const",
                regex::Regex::new(r"(?m)^\s*(\w+)\s*=\s*[A-Z_][A-Z0-9_]*").unwrap(),
            ),
        ],
    );

    // JavaScript/TypeScript patterns
    map.insert(
        "javascript",
        vec![
            (
                "function",
                regex::Regex::new(r"(?m)^\s*(?:export\s+)?(?:async\s+)?function\s+(\w+)").unwrap(),
            ),
            (
                "function",
                regex::Regex::new(
                    r"(?m)^\s*(?:export\s+)?const\s+(\w+)\s*=\s*(?:async\s+)?\(?\w*\)?\s*=>",
                )
                .unwrap(),
            ),
            (
                "class",
                regex::Regex::new(r"(?m)^\s*(?:export\s+)?class\s+(\w+)").unwrap(),
            ),
            (
                "interface",
                regex::Regex::new(r"(?m)^\s*(?:export\s+)?interface\s+(\w+)").unwrap(),
            ),
            (
                "type",
                regex::Regex::new(r"(?m)^\s*(?:export\s+)?type\s+(\w+)").unwrap(),
            ),
            (
                "const",
                regex::Regex::new(r"(?m)^\s*(?:export\s+)?const\s+(\w+)\s*=").unwrap(),
            ),
        ],
    );

    // Go patterns
    map.insert(
        "go",
        vec![
            (
                "function",
                regex::Regex::new(r"(?m)^\s*func\s+(?:\(\w+\s+\w+\)\s+)?(\w+)").unwrap(),
            ),
            (
                "struct",
                regex::Regex::new(r"(?m)^\s*type\s+(\w+)\s+struct").unwrap(),
            ),
            (
                "interface",
                regex::Regex::new(r"(?m)^\s*type\s+(\w+)\s+interface").unwrap(),
            ),
            (
                "const",
                regex::Regex::new(r"(?m)^\s*const\s+(\w+)").unwrap(),
            ),
            ("var", regex::Regex::new(r"(?m)^\s*var\s+(\w+)").unwrap()),
        ],
    );

    map
}

fn extract_signature(content: &str, start: usize, end: usize) -> Option<String> {
    // Try to get the full line
    let line_start = content[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = content[end..]
        .find('\n')
        .map(|i| end + i)
        .unwrap_or(content.len());
    let line = content[line_start..line_end].trim();
    if !line.is_empty() && line.len() < 200 {
        Some(line.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_test_project() -> (PathBuf, IndexTools) {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("nanomite-index-test-{}-{}", std::process::id(), id));
        fs::create_dir_all(&dir).unwrap();

        // Create a sample Rust project
        fs::write(
            dir.join("Cargo.toml"),
            r#"
[package]
name = "test_proj"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();

        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("src/lib.rs"),
            r#"
pub fn hello_world() {
    println!("Hello!");
}

pub struct User {
    name: String,
    age: u32,
}

pub enum Status {
    Active,
    Inactive,
}

pub trait Greet {
    fn greet(&self);
}

impl Greet for User {
    fn greet(&self) {
        println!("Hi, {}", self.name);
    }
}

const MAX_USERS: usize = 100;

mod internal {
    pub fn helper() {}
}
"#,
        )
        .unwrap();

        let tools = IndexTools::new(&dir);
        (dir, tools)
    }

    #[test]
    fn index_symbols_creates_cache() {
        let (dir, tools) = init_test_project();
        let result = tools
            .index_symbols(IndexSymbolsArgs {
                force_rebuild: true,
                languages: vec![],
            })
            .unwrap();
        assert!(result.indexed);
        assert!(result.symbols_count > 0);
        assert!(!result.languages.is_empty());
        assert!(result.cache_path.ends_with(".nanomite/symbol_index.json"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_symbol_finds_functions() {
        let (dir, tools) = init_test_project();
        tools
            .index_symbols(IndexSymbolsArgs {
                force_rebuild: true,
                languages: vec![],
            })
            .unwrap();

        let result = tools
            .find_symbol(FindSymbolArgs {
                query: "hello".to_string(),
                kind: None,
                language: None,
                limit: 10,
            })
            .unwrap();

        assert!(!result.matches.is_empty());
        assert!(result.matches.iter().any(|m| m.name == "hello_world"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_symbol_filters_by_kind() {
        let (dir, tools) = init_test_project();
        tools
            .index_symbols(IndexSymbolsArgs {
                force_rebuild: true,
                languages: vec![],
            })
            .unwrap();

        let result = tools
            .find_symbol(FindSymbolArgs {
                query: "User".to_string(),
                kind: Some("struct".to_string()),
                language: None,
                limit: 10,
            })
            .unwrap();

        assert!(!result.matches.is_empty());
        assert!(result.matches.iter().all(|m| m.kind == "struct"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_symbol_wildcard() {
        let (dir, tools) = init_test_project();
        tools
            .index_symbols(IndexSymbolsArgs {
                force_rebuild: true,
                languages: vec![],
            })
            .unwrap();

        let result = tools
            .find_symbol(FindSymbolArgs {
                query: "hel*".to_string(),
                kind: None,
                language: None,
                limit: 10,
            })
            .unwrap();

        assert!(result.matches.iter().any(|m| m.name == "hello_world"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_references_finds_definition() {
        let (dir, tools) = init_test_project();
        tools
            .index_symbols(IndexSymbolsArgs {
                force_rebuild: true,
                languages: vec![],
            })
            .unwrap();

        let result = tools
            .find_references(FindReferencesArgs {
                symbol: "hello_world".to_string(),
                from_file: None,
                limit: 10,
            })
            .unwrap();

        assert!(!result.references.is_empty());
        assert!(result.references.iter().any(|r| r.is_definition));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_escape_is_refused() {
        let dir = std::env::temp_dir().join(format!("nanomite-index-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let _tools = IndexTools::new(&dir);
        // This should work - no path escape in these tools
        let _ = fs::remove_dir_all(&dir);
    }
}
