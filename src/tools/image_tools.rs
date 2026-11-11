//! `ocr_image`: text extraction from images in the workspace via tesseract.
//!
//! The binary is discovered like the browser tools: `NANOMITE_TESSERACT` wins,
//! then the PATH is scanned. No new dependencies - tesseract is driven over
//! its CLI and the result is bounded text for the model context. Confinement
//! is identical to [`crate::tools::document_tools`]: a workspace-relative path
//! is resolved against the workspace root and anything that escapes is
//! refused.

use crate::tools::exec_tools::run_with_timeout;
use crate::tools::file_tools::resolve_within;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Hard cap for one tesseract run; OCR latency is image-size dependent, so a
/// pathological input must not park a turn forever.
const OCR_TIMEOUT: Duration = Duration::from_secs(120);

/// Extracted-text ceiling for one tool call, matching `read_document` (~16
/// KiB): the payload goes straight into the model context.
pub const MAX_TOOL_TEXT_CHARS: usize = 16 * 1024;

/// Extensions tesseract (via leptonica) reads reliably. Anything else is
/// refused up front with a clean message instead of a tesseract internals
/// dump.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "tif", "tiff", "bmp", "pnm", "webp"];

/// True when `path` looks like a bitmap image tesseract can read. Shared with
/// the `read_document` dispatch so image paths route to OCR automatically.
pub fn is_image_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Locates a usable tesseract binary: `NANOMITE_TESSERACT` wins, then the
/// PATH is scanned. Testable core takes both inputs explicitly.
pub(crate) fn find_tesseract_in(env_value: Option<&str>, path_env: &str) -> Option<PathBuf> {
    if let Some(value) = env_value.map(str::trim).filter(|v| !v.is_empty()) {
        let candidate = PathBuf::from(value);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    for dir in path_env.split(':').filter(|d| !d.is_empty()) {
        let candidate = PathBuf::from(dir).join("tesseract");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Reads `NANOMITE_TESSERACT` and `PATH` from the environment.
pub fn find_tesseract() -> Option<PathBuf> {
    find_tesseract_in(
        std::env::var("NANOMITE_TESSERACT").ok().as_deref(),
        &std::env::var("PATH").unwrap_or_default(),
    )
}

/// Default OCR language: `NANOMITE_OCR_LANG` when set, else `eng`.
fn default_lang() -> String {
    std::env::var("NANOMITE_OCR_LANG")
        .ok()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "eng".into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrImageArgs {
    /// Workspace-relative image path.
    pub path: String,
    /// Tesseract language(s), e.g. `eng`, `deu+eng`. Defaults to
    /// `NANOMITE_OCR_LANG` or `eng`.
    #[serde(default)]
    pub lang: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrImageResult {
    /// Workspace path that was read.
    pub path: String,
    /// Language pack(s) actually requested.
    pub language: String,
    /// Recognized character count (after bounding).
    pub chars: usize,
    /// Recognized text, bounded by [`MAX_TOOL_TEXT_CHARS`].
    pub text: String,
    /// True when the text cap cut extraction short; the image was never
    /// modified.
    pub truncated: bool,
}

#[derive(Clone)]
pub struct ImageTools {
    /// Workspace root, canonicalised once at construction (same as FileTools).
    root: PathBuf,
}

impl ImageTools {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        let root = std::fs::canonicalize(&workspace).unwrap_or(workspace);
        Self { root }
    }

    /// Resolves a user-supplied path and refuses anything outside the
    /// workspace. Mirrors `DocumentTools::resolve` (absolute paths are taken
    /// as given, relative paths join the root, then [`resolve_within`] must
    /// land inside the canonicalised root).
    fn resolve(&self, path: &str) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
        let joined = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.root.join(path)
        };
        resolve_within(&self.root, &joined).ok_or_else(
            || -> Box<dyn std::error::Error + Send + Sync> {
                "refused: path escapes the workspace".into()
            },
        )
    }

    /// Recognizes text in one workspace image with tesseract.
    pub fn ocr_image(
        &self,
        args: OcrImageArgs,
    ) -> Result<OcrImageResult, Box<dyn std::error::Error + Send + Sync>> {
        let binary =
            find_tesseract().ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
                "tesseract not found - install it or set NANOMITE_TESSERACT to the binary".into()
            })?;
        self.ocr_with(&binary, args)
    }

    /// Core of [`Self::ocr_image`] with an explicit binary (tests, and the
    /// `read_document` image routing shares the same path).
    pub(crate) fn ocr_with(
        &self,
        binary: &Path,
        args: OcrImageArgs,
    ) -> Result<OcrImageResult, Box<dyn std::error::Error + Send + Sync>> {
        if !is_image_path(&args.path) {
            return Err(format!(
                "not an image path (supported: {}): {}",
                IMAGE_EXTENSIONS.join(", "),
                args.path
            )
            .into());
        }
        let full_path = self.resolve(&args.path)?;
        let lang = args
            .lang
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .unwrap_or(&default_lang())
            .to_string();
        if !lang
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '+')
        {
            return Err(format!("invalid language pack name: {lang}").into());
        }

        let mut cmd = std::process::Command::new(binary);
        cmd.arg(&full_path)
            .arg("stdout")
            .arg("-l")
            .arg(&lang)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let (out, err, code, timed_out) =
            run_with_timeout(&mut cmd, "tesseract", OCR_TIMEOUT, "ocr_image")?;
        if timed_out {
            return Err(format!("ocr_image timed out after {}s", OCR_TIMEOUT.as_secs()).into());
        }
        if code != 0 && out.trim().is_empty() {
            let detail = err.lines().last().unwrap_or("no output").trim();
            return Err(format!("tesseract exited {code}: {detail}").into());
        }
        // Tesseract chatter (warnings, progress) goes to stderr; the text is
        // stdout only, so no scrubbing is needed.
        let mut text = out;
        let mut truncated = false;
        if text.chars().count() > MAX_TOOL_TEXT_CHARS {
            text = text.chars().take(MAX_TOOL_TEXT_CHARS).collect();
            text.push_str("\n… (truncated)");
            truncated = true;
        }
        Ok(OcrImageResult {
            path: args.path,
            language: lang,
            chars: text.chars().count(),
            text,
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nanomite-ocr-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("create temp workspace");
        dir
    }

    /// A stand-in tesseract: echoes a fixed line and exits 0, so the full
    /// tool path (resolve, arg building, output bounding) runs without the
    /// real binary.
    fn fake_tesseract(dir: &Path) -> PathBuf {
        let path = dir.join("tesseract");
        std::fs::write(
            &path,
            "#!/bin/sh\necho 'recognized text from fake tesseract'\n",
        )
        .expect("write fake tesseract");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake tesseract");
        }
        path
    }

    #[test]
    fn image_extension_check() {
        for path in ["a.png", "b.JPG", "c.jpeg", "d.tif", "e.webp", "f.bmp"] {
            assert!(is_image_path(path), "{path} should be an image");
        }
        for path in ["a.pdf", "b.txt", "c.docx", "noext", "d.png.bak"] {
            assert!(!is_image_path(path), "{path} is not an image");
        }
    }

    #[test]
    fn tesseract_discovery_prefers_env_then_path() {
        let dir = temp_workspace("discovery");
        let binary = fake_tesseract(&dir);
        // NANOMITE_TESSERACT wins even when it points somewhere unusual.
        assert_eq!(
            find_tesseract_in(Some(binary.to_str().unwrap()), "/nonexistent"),
            Some(binary.clone())
        );
        // An env value that does not exist falls through to the PATH scan.
        assert_eq!(
            find_tesseract_in(Some("/missing/tesseract"), dir.to_str().unwrap()),
            Some(binary.clone())
        );
        // Neither present: None.
        assert_eq!(find_tesseract_in(None, "/nonexistent"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ocr_runs_the_binary_and_bounds_output() {
        let ws = temp_workspace("run");
        std::fs::write(ws.join("scan.png"), b"fake image bytes").expect("write image");
        let binary = fake_tesseract(&ws);
        let tools = ImageTools::new(ws.clone());

        let result = tools
            .ocr_with(
                &binary,
                OcrImageArgs {
                    path: "scan.png".into(),
                    lang: Some("eng".into()),
                },
            )
            .expect("ocr succeeds");

        assert_eq!(result.path, "scan.png");
        assert_eq!(result.language, "eng");
        assert!(
            result.text.contains("fake tesseract"),
            "text: {}",
            result.text
        );
        assert!(!result.truncated);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn ocr_refuses_non_images_and_workspace_escapes() {
        let ws = temp_workspace("refuse");
        std::fs::write(ws.join("doc.pdf"), b"%PDF").expect("write pdf");
        let binary = fake_tesseract(&ws);
        let tools = ImageTools::new(ws.clone());

        let err = tools
            .ocr_with(
                &binary,
                OcrImageArgs {
                    path: "doc.pdf".into(),
                    lang: None,
                },
            )
            .expect_err("non-image refused");
        assert!(err.to_string().contains("not an image path"));

        let err = tools
            .ocr_with(
                &binary,
                OcrImageArgs {
                    path: "../outside.png".into(),
                    lang: None,
                },
            )
            .expect_err("relative escape refused");
        assert!(err.to_string().contains("escapes the workspace"));

        let err = tools
            .ocr_with(
                &binary,
                OcrImageArgs {
                    path: "/etc/hosts.png".into(),
                    lang: None,
                },
            )
            .expect_err("absolute path outside the workspace refused");
        assert!(err.to_string().contains("escapes the workspace"));
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn missing_binary_has_a_clean_message() {
        // Discovery returning None is what triggers the tool error; assert
        // the trigger condition directly instead of mutating the environment
        // (parallel tests read the same vars).
        assert_eq!(find_tesseract_in(None, "/nonexistent"), None);
    }

    #[test]
    fn ocr_rejects_bad_language_packs() {
        let ws = temp_workspace("lang");
        std::fs::write(ws.join("scan.png"), b"fake").expect("write image");
        let binary = fake_tesseract(&ws);
        let tools = ImageTools::new(ws.clone());
        let err = tools
            .ocr_with(
                &binary,
                OcrImageArgs {
                    path: "scan.png".into(),
                    lang: Some("eng; rm -rf /".into()),
                },
            )
            .expect_err("bad lang refused");
        assert!(err.to_string().contains("invalid language pack"));
        let _ = std::fs::remove_dir_all(&ws);
    }
}
