//! Structured diagnostic types for parsed errors and failures.
//!
//! Each diagnostic carries a stable id, severity, kind, file/line/column
//! location, the raw error message, and optional fix hints extracted from
//! the source output.

use serde::{Deserialize, Serialize};

/// Severity of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
        }
    }
}

/// Category of the diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticKind {
    /// Rust compiler error (E0xxx)
    Compiler,
    /// Test failure (test result: FAILED, panicked, assertion)
    TestFailure,
    /// Runtime panic at a specific location
    Panic,
    /// Missing symbol / unresolved import
    UnresolvedImport,
    /// Type mismatch
    TypeMismatch,
    /// Lifetime / borrow checker issue
    Lifetime,
    /// Missing feature or dependency
    MissingDependency,
    /// Other / unclassified
    Other,
}

impl DiagnosticKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compiler => "compiler",
            Self::TestFailure => "test_failure",
            Self::Panic => "panic",
            Self::UnresolvedImport => "unresolved_import",
            Self::TypeMismatch => "type_mismatch",
            Self::Lifetime => "lifetime",
            Self::MissingDependency => "missing_dependency",
            Self::Other => "other",
        }
    }
}

/// A single parsed diagnostic from compiler/test/runtime output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugDiagnostic {
    /// Stable id: `<kind>-<file>:<line>:<col>` or a hash for location-less errors.
    pub id: String,
    pub kind: DiagnosticKind,
    pub severity: Severity,
    /// File path (may be empty for global errors like linker errors).
    pub file: String,
    /// 1-based line number (0 if unknown).
    pub line: usize,
    /// 1-based column number (0 if unknown).
    pub col: usize,
    /// The primary error message.
    pub message: String,
    /// Secondary notes/span context from the compiler.
    pub context: Vec<String>,
    /// Whether a known fix pattern exists for this kind of error.
    pub has_known_fix: bool,
}

impl DebugDiagnostic {
    pub fn new(
        id: String,
        kind: DiagnosticKind,
        severity: Severity,
        file: String,
        line: usize,
        col: usize,
        message: String,
        context: Vec<String>,
    ) -> Self {
        let has_known_fix = matches!(
            kind,
            DiagnosticKind::UnresolvedImport
                | DiagnosticKind::TypeMismatch
                | DiagnosticKind::MissingDependency
        );
        Self {
            id,
            kind,
            severity,
            file,
            line,
            col,
            message,
            context,
            has_known_fix,
        }
    }
}
