//! Parsing of compiler errors, test failures, and runtime panics
//! from plain text build/test output.
//!
//! Uses regex-free string scanning so it compiles everywhere with zero
//! external dependencies. Handles three source formats:
//! - `rustc` / `cargo` compiler output (`error[E0xxx]: ... --> src/file.rs:10:5`)
//! - `cargo test --lib` output (`test result: FAILED`, `panicked at '...'`, `src/file.rs:25:12`)
//! - Runtime panic output (`thread 'main' panicked at '...'`, `src/file.rs:42:9`)

use super::diagnostics::{DebugDiagnostic, DiagnosticKind, Severity};

/// Parses raw output text into a vector of structured diagnostics.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticsParser;

impl DiagnosticsParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse `output` and return all diagnostics found.
    pub fn parse(&self, output: &str) -> Vec<DebugDiagnostic> {
        let mut diagnostics = Vec::new();

        // Compiler errors: `error[E0xxx]: message` followed by `--> path:line:col`
        diagnostics.extend(self.parse_compiler_errors(output));

        // Test failures: `panicked at 'message', src/file.rs:line:col`
        diagnostics.extend(self.parse_test_panics(output));

        // Test result lines: `test result: FAILED. N passed; M failed; ...`
        diagnostics.extend(self.parse_test_results(output));

        diagnostics
    }

    // --- compiler error parsing ----------------------------------------------

    fn parse_compiler_errors(&self, output: &str) -> Vec<DebugDiagnostic> {
        let mut out = Vec::new();
        let lines: Vec<&str> = output.lines().collect();

        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            // Match `error[E0382]: borrow of moved value: ...`
            if let Some((code, message)) = self.extract_compiler_error(line) {
                // Look ahead for the `--> path:line:col` span
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
                    } else if l.starts_with("error[") || l.starts_with("warning:") {
                        break;
                    }
                    j += 1;
                }

                let id = if !file.is_empty() && line_no > 0 {
                    format!("{}-{}:{}:{}", code, file, line_no, col)
                } else {
                    format!("{}-global", code)
                };

                let kind = self.classify_compiler_error(code.as_str(), message.as_str());
                let severity = Severity::Error;

                out.push(DebugDiagnostic::new(
                    id, kind, severity, file, line_no, col, message, context,
                ));
                i = j;
            } else {
                i += 1;
            }
        }

        out
    }

    /// Extract `error[E0xxx]: message` from a compiler error line.
    fn extract_compiler_error(&self, line: &str) -> Option<(String, String)> {
        let line = line.trim();
        if !line.starts_with("error[") {
            return None;
        }
        // Find the closing `]` of the error code
        let end_code = line.find(']')?;
        let code = line[6..end_code].to_string(); // skip "error["
        let rest = line[end_code + 1..].trim();
        // Skip `: ` or `:`
        let message = rest
            .strip_prefix(": ")
            .unwrap_or(rest.strip_prefix(':').unwrap_or(rest))
            .trim()
            .to_string();
        Some((code, message))
    }

    /// Extract `--> path/to/file.rs:line:col` from a span line.
    fn extract_span(&self, line: &str) -> Option<(String, usize, usize)> {
        let line = line.trim();
        let arrow = line.find("--> ")?;
        let after = line[arrow + 4..].trim();
        // Format: `path.rs:10:5` or `path.rs:10`
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

    /// Classify a rustc error code into a DiagnosticKind.
    fn classify_compiler_error(&self, code: &str, _message: &str) -> DiagnosticKind {
        match code {
            "E0433" | "E0432" | "E0601" | "E0602" | "E0603" | "E0604" => {
                DiagnosticKind::UnresolvedImport
            }
            "E0308" | "E0277" | "E0631" => DiagnosticKind::TypeMismatch,
            "E0499" | "E0502" | "E0507" | "E0597" => DiagnosticKind::Lifetime,
            "E0463" | "E0554" | "E0555" | "E0556" => DiagnosticKind::MissingDependency,
            _ => DiagnosticKind::Compiler,
        }
    }

    // --- test panic parsing --------------------------------------------------

    fn parse_test_panics(&self, output: &str) -> Vec<DebugDiagnostic> {
        let mut out = Vec::new();
        let lines: Vec<&str> = output.lines().collect();

        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            // Match `panicked at 'assertion failed', src/file.rs:25:12`
            if line.contains("panicked at ") {
                if let Some((message, file, line_no, col)) = self.extract_panic_location(line) {
                    let id = format!("panic-{}:{}:{}", file, line_no, col);
                    out.push(DebugDiagnostic::new(
                        id,
                        DiagnosticKind::Panic,
                        Severity::Error,
                        file,
                        line_no,
                        col,
                        message,
                        Vec::new(),
                    ));
                }
            }
            i += 1;
        }

        out
    }

    /// Extract `panicked at 'message', path.rs:line:col` from a panic line.
    fn extract_panic_location(&self, line: &str) -> Option<(String, String, usize, usize)> {
        let marker = "panicked at '";
        let start = line.find(marker)?;
        let after_msg = &line[start + marker.len()..];
        let end_quote = after_msg.find('\'')?;
        let message = after_msg[..end_quote].to_string();

        // After the quote: `, src/file.rs:25:12`
        let rest = after_msg[end_quote + 1..].trim();
        let rest = rest.strip_prefix(',').unwrap_or(rest).trim();

        // Find the file:line:col part
        let colon_idx = rest.rfind(':')?;
        let file_part = rest[..colon_idx].trim();
        let col_part = rest[colon_idx + 1..].trim();
        let col = col_part.parse().unwrap_or(0);

        let line_colon = file_part.rfind(':')?;
        let file = file_part[..line_colon].trim().to_string();
        let line_no = file_part[line_colon + 1..].trim().parse().unwrap_or(0);

        Some((message, file, line_no, col))
    }

    // --- test result parsing -------------------------------------------------

    fn parse_test_results(&self, output: &str) -> Vec<DebugDiagnostic> {
        let mut out = Vec::new();
        for line in output.lines() {
            let line = line.trim();
            if line.starts_with("test result: FAILED") {
                // Extract counts
                let message = line.to_string();
                out.push(DebugDiagnostic::new(
                    format!("test-result-{}", hash_string(&message)),
                    DiagnosticKind::TestFailure,
                    Severity::Error,
                    String::new(),
                    0,
                    0,
                    message,
                    Vec::new(),
                ));
            }
        }
        out
    }
}

/// Deterministic FNV-1a hash (shared with neural module but re-implemented here
/// to keep `debug` dependency-free).
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
    fn parses_compiler_error_with_span() {
        let output = "\
error[E0382]: borrow of moved value: `x` used here after move
  --> src/lib.rs:10:5
   |
10 |     let y = x;
   |     ^ value borrowed here after move
";
        let parser = DiagnosticsParser::new();
        let diags = parser.parse(output);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.kind, DiagnosticKind::Compiler);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.file, "src/lib.rs");
        assert_eq!(d.line, 10);
        assert_eq!(d.col, 5);
        assert!(d.message.contains("borrow of moved value"));
    }

    #[test]
    fn parses_test_panic() {
        let output = "\
panicked at 'assertion failed: left == right', src/lib.rs:25:12
";
        let parser = DiagnosticsParser::new();
        let diags = parser.parse(output);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.kind, DiagnosticKind::Panic);
        assert_eq!(d.file, "src/lib.rs");
        assert_eq!(d.line, 25);
        assert_eq!(d.col, 12);
        assert!(d.message.contains("assertion failed"));
    }

    #[test]
    fn parses_test_result_failed() {
        let output = "\
test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
";
        let parser = DiagnosticsParser::new();
        let diags = parser.parse(output);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, DiagnosticKind::TestFailure);
    }

    #[test]
    fn empty_output_yields_no_diagnostics() {
        let parser = DiagnosticsParser::new();
        let diags = parser.parse("");
        assert!(diags.is_empty());
    }

    #[test]
    fn unresolved_import_is_flagged_as_known_fix() {
        let output = "\
error[E0433]: failed to resolve: could not find `Foo` in scope
  --> src/lib.rs:3:5
";
        let parser = DiagnosticsParser::new();
        let diags = parser.parse(output);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].has_known_fix);
        assert_eq!(diags[0].kind, DiagnosticKind::UnresolvedImport);
    }
}
