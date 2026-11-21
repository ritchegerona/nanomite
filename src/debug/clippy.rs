//! Parses `cargo clippy` warnings into structured diagnostics.
//!
//! Clippy output looks like:
//! ```text
//! warning: foo is unused
//!   --> src/lib.rs:10:5
//!    |
//! 10 |     let foo = 1;
//!    |     ^^^ help: remove this
//!
//! warning: clippy::unwrap_used
//!   --> src/main.rs:3:9
//!    |
//!  3 |     let x = y.unwrap();
//!    |         ^^^^^^^^^^^^^ help: consider using .map_err() or ?
//! ```
//!
//! Produces `DebugDiagnostic`s with `kind = Compiler` and `has_known_fix`
//! set for patterns the fix generator knows how to handle.

use super::diagnostics::{DebugDiagnostic, DiagnosticKind, Severity};

/// Parses `cargo clippy` output into diagnostics.
#[derive(Debug, Clone, Default)]
pub struct ClippyParser;

impl ClippyParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse clippy output text into diagnostics.
    pub fn parse(&self, output: &str) -> Vec<DebugDiagnostic> {
        let mut diagnostics = Vec::new();
        let lines: Vec<&str> = output.lines().collect();

        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];

            // Match `warning: message` or `warning: clippy::lint_name`
            if line.starts_with("warning:") {
                let message = line.trim_start_matches("warning:").trim().to_string();
                let mut file = String::new();
                let mut line_no = 0usize;
                let mut col = 0usize;
                let mut context = Vec::new();

                let mut j = i + 1;
                while j < lines.len() && j < i + 12 {
                    let l = lines[j];
                    if let Some((f, ln, c)) = self.extract_span(l) {
                        file = f;
                        line_no = ln;
                        col = c;
                    } else if l.starts_with("   |") || l.starts_with("    |") {
                        context.push(l.to_string());
                    } else if l.starts_with("warning:") || l.starts_with("error:") {
                        break;
                    }
                    j += 1;
                }

                let id = if !file.is_empty() && line_no > 0 {
                    format!("clippy-{}:{}:{}", file, line_no, col)
                } else {
                    format!("clippy-global-{}", hash_string(message.as_str()))
                };

                let kind = self.classify_clippy(message.as_str());
                let has_known_fix = message.as_str().contains("unused import")
                    || message.as_str().contains("dead_code")
                    || message.as_str().contains("needless")
                    || message.as_str().contains("redundant")
                    || message.as_str().contains("unwrap_used")
                    || message.as_str().contains("expect_used")
                        && matches!(
                            kind,
                            DiagnosticKind::UnresolvedImport | DiagnosticKind::Other
                        );

                diagnostics.push(DebugDiagnostic::new(
                    id,
                    kind,
                    Severity::Warning,
                    file,
                    line_no,
                    col,
                    message,
                    context,
                ));

                if let Some(d) = diagnostics.last_mut() {
                    d.has_known_fix = has_known_fix;
                }

                i = j;
            } else {
                i += 1;
            }
        }

        diagnostics
    }

    /// Extract `--> path.rs:line:col` from a span line.
    fn extract_span(&self, line: &str) -> Option<(String, usize, usize)> {
        let line = line.trim();
        let arrow = line.find("--> ")?;
        let after = line[arrow + 4..].trim();
        let parts: Vec<&str> = after.splitn(3, ':').collect();
        if parts.is_empty() {
            return None;
        }
        let file = parts[0].trim().to_string();
        let line_no = parts
            .get(1)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let col = parts
            .get(2)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        Some((file, line_no, col))
    }

    /// Classify a clippy warning message into a DiagnosticKind.
    fn classify_clippy(&self, message: &str) -> DiagnosticKind {
        if message.contains("unused_imports") || message.contains("unused import") {
            DiagnosticKind::UnresolvedImport
        } else if message.contains("dead_code")
            || message.contains("dead code")
            || message.contains("unwrap_used")
            || message.contains("unwrap()")
            || message.contains("expect_used")
            || message.contains("expect(")
            || message.contains("needless")
            || message.contains("redundant")
        {
            DiagnosticKind::Other
        } else {
            DiagnosticKind::Compiler
        }
    }
}

/// Deterministic FNV-1a hash.
fn hash_string(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unused_import() {
        let output = "\
warning: unused import: `foo`
  --> src/lib.rs:5:1
   |
5  |     use foo;
   |     ^^^^^ help: remove this
";
        let parser = ClippyParser::new();
        let diags = parser.parse(output);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].file, "src/lib.rs");
        assert_eq!(diags[0].line, 5);
        assert!(diags[0].has_known_fix);
        assert!(diags[0].message.contains("unused import"));
    }

    #[test]
    fn parses_unwrap_used() {
        let output = "\
warning: clippy::unwrap_used
  --> src/main.rs:3:9
   |
3  |     let x = y.unwrap();
   |         ^^^^^^^^^^^^^ help: consider using `.map_err()` or `?`
";
        let parser = ClippyParser::new();
        let diags = parser.parse(output);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].file, "src/main.rs");
        assert_eq!(diags[0].line, 3);
        assert!(diags[0].message.contains("unwrap_used"));
    }

    #[test]
    fn parses_dead_code() {
        let output = "\
warning: dead_code
  --> src/lib.rs:10:5
   |
10 |     fn never_used() {}
   |     ^^^^^^^^^^^^^^^ help: this function is never used
";
        let parser = ClippyParser::new();
        let diags = parser.parse(output);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, DiagnosticKind::Other);
        assert!(diags[0].has_known_fix);
    }

    #[test]
    fn empty_output_yields_none() {
        let parser = ClippyParser::new();
        assert!(parser.parse("").is_empty());
    }
}
