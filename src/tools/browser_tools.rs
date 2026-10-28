//! Browser automation: headless Chrome/Chromium driven through its CLI.
//!
//! Two tools for the agent:
//! - `browse` — render a URL in headless Chrome and return the readable
//!   text (JavaScript executed, unlike `web_fetch`'s plain HTTP fetch)
//! - `browse_screenshot` — render a URL and save a full-page PNG inside
//!   the workspace
//!
//! The browser binary is discovered from `NANOMITE_BROWSER`, then from the
//! PATH (google-chrome, chromium, brave, edge, …). No new dependencies: the
//! CLI surface (`--dump-dom`, `--screenshot`) is stable across Chromium forks.

use crate::extract::ExtractionLimits;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

const BROWSE_TIMEOUT: Duration = Duration::from_secs(90);
const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_TEXT: usize = 20_000;

#[derive(Clone)]
pub struct BrowserTools {
    workspace: PathBuf,
}

impl BrowserTools {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Locates a usable Chromium-family browser: `NANOMITE_BROWSER` wins,
    /// then the PATH is scanned for common binary names.
    pub fn find_browser() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("NANOMITE_BROWSER") {
            let p = PathBuf::from(path);
            if p.is_file() {
                return Some(p);
            }
        }
        const CANDIDATES: [&str; 7] = [
            "google-chrome-stable",
            "google-chrome",
            "chromium",
            "chromium-browser",
            "brave-browser",
            "microsoft-edge",
            "edge",
        ];
        let path_env = std::env::var("PATH").unwrap_or_default();
        for dir in path_env.split(':') {
            for name in CANDIDATES {
                let candidate = PathBuf::from(dir).join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    fn require_browser() -> Result<PathBuf, String> {
        Self::find_browser().ok_or_else(|| {
            "no chrome/chromium browser found - install one or set NANOMITE_BROWSER".into()
        })
    }

    fn validate_url(url: &str) -> Result<String, String> {
        let url = url.trim().to_string();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("refused: only http(s) URLs are allowed".into());
        }
        Ok(url)
    }

    /// Renders `url` headless and returns the page's readable text.
    pub fn browse(&self, args: BrowseArgs) -> Result<BrowseResult, String> {
        let browser = Self::require_browser()?;
        self.browse_with(browser, args)
    }

    /// Core of [`Self::browse`] with an explicit browser binary (tests).
    pub(crate) fn browse_with(
        &self,
        browser: PathBuf,
        args: BrowseArgs,
    ) -> Result<BrowseResult, String> {
        let url = Self::validate_url(&args.url)?;
        let wait = args.wait_ms.unwrap_or(3_000).clamp(200, 15_000);

        let mut cmd = std::process::Command::new(&browser);
        cmd.args([
            "--headless=new",
            "--disable-gpu",
            &format!("--virtual-time-budget={wait}"),
            "--dump-dom",
            &url,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
        let (out, err, code, timed_out) =
            super::exec_tools::run_with_timeout(&mut cmd, "browser", BROWSE_TIMEOUT, "browse")?;
        if timed_out {
            return Err(format!(
                "browse timed out after {}s",
                BROWSE_TIMEOUT.as_secs()
            ));
        }
        if code != 0 && out.trim().is_empty() {
            let detail = err.lines().last().unwrap_or("no output").trim();
            return Err(format!("browser exited {code}: {detail}"));
        }

        // The rendered DOM goes through the shared HTML extraction pipeline
        // (strip scripts/styles, keep readable text), via a temp file.
        let dom_path =
            std::env::temp_dir().join(format!("nanomite-dom-{}.html", std::process::id()));
        std::fs::write(&dom_path, out.as_bytes())
            .map_err(|e| format!("could not buffer the rendered page: {e}"))?;
        let extracted = crate::extract::extract_path(&dom_path, &ExtractionLimits::default())
            .map_err(|e| format!("could not read the rendered page: {e}"));
        let _ = std::fs::remove_file(&dom_path);
        let extracted = extracted?;

        let mut text = extracted.text;
        let mut truncated = false;
        if text.chars().count() > MAX_TEXT {
            text = text.chars().take(MAX_TEXT).collect();
            text.push_str("\n… (truncated)");
            truncated = true;
        }

        Ok(BrowseResult {
            url,
            text,
            truncated,
        })
    }

    /// Renders `url` headless and saves a PNG inside the workspace.
    pub fn screenshot(&self, args: BrowseScreenshotArgs) -> Result<ScreenshotResult, String> {
        let browser = Self::require_browser()?;
        self.screenshot_with(browser, args)
    }

    /// Core of [`Self::screenshot`] with an explicit browser binary (tests).
    pub(crate) fn screenshot_with(
        &self,
        browser: PathBuf,
        args: BrowseScreenshotArgs,
    ) -> Result<ScreenshotResult, String> {
        let url = Self::validate_url(&args.url)?;

        let output = match args
            .output
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(rel) => {
                if rel.contains("..") || rel.starts_with('/') {
                    return Err("refused: output must be a workspace-relative path".into());
                }
                let full = self.workspace.join(rel);
                if let Some(parent) = full.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                full
            }
            None => {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                self.workspace.join(format!("screenshot-{stamp}.png"))
            }
        };
        let output = output.canonicalize().unwrap_or_else(|_| output.clone());
        if !output.starts_with(&self.workspace) {
            return Err("refused: output escapes the workspace".into());
        }

        let mut cmd = std::process::Command::new(&browser);
        cmd.args([
            "--headless=new",
            "--disable-gpu",
            "--hide-scrollbars",
            "--window-size=1280,900",
            &format!("--screenshot={}", output.display()),
            &url,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
        let (_, err, code, timed_out) = super::exec_tools::run_with_timeout(
            &mut cmd,
            "browser",
            SCREENSHOT_TIMEOUT,
            "browse_screenshot",
        )?;
        if timed_out {
            return Err("screenshot timed out after 60s".into());
        }
        if code != 0 || !output.is_file() {
            let detail = err.lines().last().unwrap_or("no output").trim();
            return Err(format!("screenshot failed (code {code}): {detail}"));
        }
        let bytes = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);

        Ok(ScreenshotResult {
            url,
            path: output.display().to_string(),
            bytes,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseArgs {
    /// Absolute http(s) URL.
    pub url: String,
    /// Milliseconds of virtual time budget for JavaScript rendering.
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseResult {
    pub url: String,
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseScreenshotArgs {
    /// Absolute http(s) URL.
    pub url: String,
    /// Workspace-relative PNG path; a timestamped default when omitted.
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotResult {
    pub url: String,
    pub path: String,
    pub bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes an executable fake browser that prints a fixed HTML page and
    /// touches any `--screenshot=<path>` argument. Returns its path.
    fn fake_browser(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nanomite-browser-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let script = dir.join("fake-browser.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nfor a in \"$@\"; do\n  case \"$a\" in\n    --screenshot=*) touch \"${a#--screenshot=}\" ;;\n  esac\ndone\necho '<html><head><title>Fake</title></head><body><h1>Fake Heading</h1><p>browser text here</p></body></html>'\n",
        )
        .expect("write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }
        script
    }

    #[test]
    fn browse_extracts_readable_text_through_a_fake_browser() {
        let script = fake_browser("browse");
        let tools = BrowserTools::new(std::env::temp_dir());
        let result = tools
            .browse_with(
                script.clone(),
                BrowseArgs {
                    url: "https://example.test/page".into(),
                    wait_ms: None,
                },
            )
            .expect("browse");
        assert_eq!(result.url, "https://example.test/page");
        assert!(!result.truncated);
        let haystack = format!("{} {}", result.text, "Fake Heading");
        assert!(
            haystack.contains("Fake Heading"),
            "title missing: {haystack:?}"
        );
        assert!(
            haystack.contains("browser text here"),
            "body missing: {haystack:?}"
        );
        assert!(script.exists());
        let _ = std::fs::remove_dir_all(script.parent().unwrap());
    }

    #[test]
    fn screenshot_saves_a_png_in_the_workspace() {
        let workspace =
            std::env::temp_dir().join(format!("nanomite-browser-ws-{}", std::process::id()));
        std::fs::create_dir_all(&workspace).expect("workspace");
        let script = fake_browser("shot");
        let tools = BrowserTools::new(workspace.clone());
        let out_rel = "shots/one.png";
        let result = tools
            .screenshot_with(
                script.clone(),
                BrowseScreenshotArgs {
                    url: "https://example.test/page".into(),
                    output: Some(out_rel.into()),
                },
            )
            .expect("screenshot");
        assert!(
            result.path.ends_with("shots/one.png"),
            "path: {}",
            result.path
        );
        assert!(workspace.join(out_rel).is_file(), "png must exist");
        assert_eq!(result.bytes, 0);
        let _ = std::fs::remove_dir_all(&workspace);
        let _ = std::fs::remove_dir_all(script.parent().unwrap());
    }

    #[test]
    fn browse_refuses_non_http_urls_and_missing_browser() {
        let tools = BrowserTools::new(std::env::temp_dir());
        let err = tools
            .browse(BrowseArgs {
                url: "ftp://example.test".into(),
                wait_ms: None,
            })
            .expect_err("ftp refused");
        assert!(err.contains("http(s)"), "err: {err}");

        // Missing browser binary fails with a clean, actionable message.
        let err = tools
            .browse_with(
                PathBuf::from("/nonexistent/browser-bin"),
                BrowseArgs {
                    url: "https://example.test".into(),
                    wait_ms: None,
                },
            )
            .expect_err("missing browser refused");
        assert!(
            err.contains("browser exited") || err.contains("failed"),
            "err: {err}"
        );
    }
}
