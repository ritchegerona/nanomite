//! `read_document`: bounded text extraction from workspace documents.
//!
//! Backed by the [`crate::extract`] registry, so the tool understands every
//! format the index understands (pdf, docx, xlsx, ods, odt, rtf, csv and the
//! text family) without carrying any parsing of its own. Confinement is
//! identical to [`crate::tools::file_tools`]: a workspace-relative path is
//! resolved against the workspace root and anything that escapes is refused.

use crate::extract::{ExtractionLimits, ExtractorRegistry};
use crate::tools::file_tools::normalize;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

/// Extracted-text ceiling for one tool call (~16 KiB), well below the index's
/// on-demand cap: the payload goes straight into the model context.
pub const MAX_TOOL_TEXT_CHARS: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadDocumentArgs {
    /// Workspace-relative document path.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadDocumentResult {
    /// Detected format (`pdf`, `docx`, `csv`, …) as reported by the registry.
    pub format: String,
    /// Number of structural sections kept after extraction.
    pub sections: usize,
    /// True when a cap (file size, text size, section count) cut extraction
    /// short; the source file itself was never modified.
    pub truncated: bool,
    /// Extracted text, bounded by [`MAX_TOOL_TEXT_CHARS`].
    pub text: String,
}

pub struct DocumentTools {
    /// Workspace root, canonicalised once at construction (same as FileTools).
    root: PathBuf,
    max_file_bytes: usize,
    registry: ExtractorRegistry,
}

impl DocumentTools {
    pub fn new(workspace: impl Into<PathBuf>, max_file_bytes: usize) -> Self {
        let workspace = workspace.into();
        let root = fs::canonicalize(&workspace).unwrap_or(workspace);
        Self {
            root,
            max_file_bytes,
            registry: ExtractorRegistry::builtin(),
        }
    }

    /// Resolves a user-supplied path and refuses anything outside the
    /// workspace. Mirrors `FileTools::resolve`: absolute paths are taken as
    /// given, relative paths join the root, then lexical normalisation must
    /// land inside the root.
    fn resolve(&self, path: &str) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
        let joined = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.root.join(path)
        };
        let normalized = normalize(&joined);
        if !normalized.starts_with(&self.root) {
            return Err("refused: path escapes the workspace".into());
        }
        Ok(normalized)
    }

    /// Detects the format and extracts bounded text from one document.
    pub fn read_document(
        &self,
        args: ReadDocumentArgs,
    ) -> Result<ReadDocumentResult, Box<dyn Error + Send + Sync>> {
        let full_path = self.resolve(&args.path)?;

        // On-demand tool limits (bounded latency), with the payload-sized text
        // cap on top.
        let mut limits = ExtractionLimits::for_tool(self.max_file_bytes);
        limits.max_text_chars = MAX_TOOL_TEXT_CHARS;

        let doc = self
            .registry
            .extract_path(&full_path, &limits)
            .map_err(|e| e.to_string())?;

        Ok(ReadDocumentResult {
            format: doc.format.as_str().to_string(),
            sections: doc.sections.len(),
            truncated: doc.truncated,
            text: doc.text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique temp workspace per call so parallel tests never collide.
    fn temp_workspace(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nanomite-doc-tools-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).expect("create temp workspace");
        dir
    }

    #[test]
    fn reads_plain_text_markdown_json_and_csv() {
        let ws = temp_workspace("text-family");
        fs::write(ws.join("notes.txt"), "first line\nsecond line\n").expect("write txt");
        fs::write(
            ws.join("guide.md"),
            "# Guide\n\nIntro paragraph.\n\n## Details\n\nBody text.\n",
        )
        .expect("write md");
        fs::write(ws.join("data.json"), r#"{"name": "nanomite", "tools": 16}"#)
            .expect("write json");
        fs::write(ws.join("table.csv"), "id,label\n1,alpha\n2,beta\n").expect("write csv");

        let tools = DocumentTools::new(ws.clone(), 200_000);

        let txt = tools
            .read_document(ReadDocumentArgs {
                path: "notes.txt".into(),
            })
            .expect("txt");
        assert_eq!(txt.format, "text");
        assert!(txt.text.contains("second line"));
        assert!(!txt.truncated);

        let md = tools
            .read_document(ReadDocumentArgs {
                path: "guide.md".into(),
            })
            .expect("md");
        assert_eq!(md.format, "markdown");
        assert!(md.text.contains("Intro paragraph."));
        assert!(md.sections >= 1, "headings should become sections");

        let json = tools
            .read_document(ReadDocumentArgs {
                path: "data.json".into(),
            })
            .expect("json");
        assert_eq!(json.format, "json");
        assert!(json.text.contains("nanomite"));

        let csv = tools
            .read_document(ReadDocumentArgs {
                path: "table.csv".into(),
            })
            .expect("csv");
        assert_eq!(csv.format, "csv");
        assert!(csv.text.contains("alpha"), "csv text: {}", csv.text);

        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn refuses_paths_that_escape_the_workspace() {
        let ws = temp_workspace("escape");
        let outside =
            std::env::temp_dir().join(format!("nanomite-doc-outside-{}.txt", std::process::id()));
        fs::write(&outside, "secret").expect("write outside file");

        let tools = DocumentTools::new(ws.clone(), 200_000);

        let err = tools
            .read_document(ReadDocumentArgs {
                path: "../elsewhere.txt".into(),
            })
            .expect_err("relative escape refused");
        assert!(err.to_string().contains("escapes the workspace"));

        let err = tools
            .read_document(ReadDocumentArgs {
                path: outside.display().to_string(),
            })
            .expect_err("absolute path outside the workspace refused");
        assert!(err.to_string().contains("escapes the workspace"));

        let _ = fs::remove_dir_all(&ws);
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn reports_missing_files_as_errors() {
        let ws = temp_workspace("missing");
        let tools = DocumentTools::new(ws.clone(), 200_000);
        let err = tools
            .read_document(ReadDocumentArgs {
                path: "nope.txt".into(),
            })
            .expect_err("missing file");
        assert!(!err.to_string().is_empty());
        let _ = fs::remove_dir_all(&ws);
    }
}
