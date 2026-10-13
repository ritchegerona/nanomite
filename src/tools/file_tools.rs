use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
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
        let full_path = self.resolve(Some(&args.path))?;
        let offset = args.offset.unwrap_or(0);
        let budget = self.max_file_bytes.min(args.limit.unwrap_or(usize::MAX));

        let bytes = fs::read(&full_path).map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("{}: {e}", self.relative(&full_path)).into()
        })?;

        let slice = &bytes[offset.min(bytes.len())..];
        let slice = &slice[..slice.len().min(budget)];
        let trimmed = offset + slice.len() < bytes.len();

        let content = String::from_utf8_lossy(slice).into_owned();
        let lines = content.lines().count();

        Ok(ReadFileResult {
            content,
            trimmed,
            lines,
        })
    }

    pub fn search(&self, args: SearchArgs) -> Result<SearchResult, Box<dyn Error + Send + Sync>> {
        let start = self.resolve(args.path.as_deref())?;
        let mut hits = Vec::new();
        let mut files_scanned = 0usize;
        let mut truncated = false;
        let needle = args.query.to_lowercase();
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

                let Ok(bytes) = fs::read(&path) else { continue };
                if bytes.len() > self.max_file_bytes || looks_binary(&bytes) {
                    continue;
                }
                let text = String::from_utf8_lossy(&bytes);
                let rel = self.relative(&path);
                for (idx, line) in text.lines().enumerate() {
                    if line.to_lowercase().contains(&needle) {
                        hits.push(SearchHit {
                            path: rel.clone(),
                            line: idx + 1,
                            text: line.trim_end().chars().take(200).collect(),
                        });
                        if hits.len() >= MAX_HITS {
                            truncated = true;
                            break;
                        }
                    }
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
        
        Ok(EditFileResult {
            replacements_made,
        })
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
}
