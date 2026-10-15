//! Source-code extractor. Always compiled (no parser dependency): source is
//! already text, so extraction is language labelling plus structure-light
//! sectioning (blank-line paragraphs and `//`-style banner comments become
//! boundaries so function/doc blocks stay findable).
//!
//! The extractor is deliberately **not** a parser: it never executes or
//! evaluates anything, it only reads bytes.

use super::{ExtractError, Extractor, Format, Input, Section, SectionKind, Sink};

/// Extension -> language name for the formats the indexer labels.
const LANGUAGES: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("py", "python"),
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("ts", "typescript"),
    ("tsx", "typescript"),
    ("jsx", "javascript"),
    ("go", "go"),
    ("c", "c"),
    ("h", "c"),
    ("cc", "cpp"),
    ("cpp", "cpp"),
    ("cxx", "cpp"),
    ("hpp", "cpp"),
    ("java", "java"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("scala", "scala"),
    ("rb", "ruby"),
    ("php", "php"),
    ("sh", "shell"),
    ("bash", "shell"),
    ("zsh", "shell"),
    ("fish", "shell"),
    ("ps1", "powershell"),
    ("sql", "sql"),
    ("cs", "csharp"),
    ("swift", "swift"),
    ("lua", "lua"),
    ("pl", "perl"),
    ("r", "r"),
    ("dart", "dart"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("erl", "erlang"),
    ("hs", "haskell"),
    ("clj", "clojure"),
    ("vue", "vue"),
    ("svelte", "svelte"),
    ("css", "css"),
    ("scss", "scss"),
    ("dockerfile", "dockerfile"),
    ("makefile", "makefile"),
    ("cmake", "cmake"),
    ("gradle", "gradle"),
    ("tf", "terraform"),
    ("proto", "protobuf"),
    ("zig", "zig"),
    ("nim", "nim"),
    ("jl", "julia"),
    ("ml", "ocaml"),
    ("fs", "fsharp"),
    ("vim", "vimscript"),
    ("el", "emacs-lisp"),
    ("lisp", "lisp"),
    ("mk", "makefile"),
];

/// Well-known filenames whose language cannot be derived from an extension.
const FILENAMES: &[(&str, &str)] = &[
    ("dockerfile", "dockerfile"),
    ("makefile", "makefile"),
    ("gnumakefile", "makefile"),
    ("cmakelists.txt", "cmake"),
    ("rakefile", "ruby"),
    ("gemfile", "ruby"),
    ("vagrantfile", "ruby"),
    ("procfile", "text"),
    ("go.mod", "go"),
    ("go.sum", "go"),
];

/// Language for a lowercase extension, if it is a source language.
pub fn language_for_extension(ext: &str) -> Option<String> {
    LANGUAGES
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, l)| l.to_string())
}

/// Language for a file name (handles `Makefile`, `Dockerfile`, ...).
pub fn language_for_filename(name: &str) -> Option<String> {
    FILENAMES
        .iter()
        .find(|(n, _)| *n == name.to_ascii_lowercase().as_str())
        .map(|(_, l)| l.to_string())
}

/// Picks the source language from the extension, the file name, or a shebang.
pub fn detect_language(path: &std::path::Path, head: &[u8]) -> Option<String> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if let Some(lang) = language_for_filename(name) {
        return Some(lang);
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if let Some(lang) = language_for_extension(&ext) {
        return Some(lang);
    }
    // Shebang fallback: `#!/usr/bin/env python3` or `#!/bin/bash`.
    if head.starts_with(b"#!") {
        let head_text = String::from_utf8_lossy(&head[..head.len().min(120)]);
        // Only the first line is the shebang. After `#!` the first token is the
        // interpreter path; `env` is a launcher, so the token after it names the
        // real interpreter.
        let line = head_text.lines().next().unwrap_or("");
        let mut tokens = line.trim_start_matches("#!").split_whitespace();
        let mut interp = tokens.next().unwrap_or_default();
        if interp == "env" || interp.ends_with("/env") {
            interp = tokens.next().unwrap_or_default();
        }
        let base = interp.rsplit('/').next().unwrap_or_default();
        // Version suffixes (`python3`, `perl5.36`) are not part of the name.
        let base = base.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
        if base.is_empty() {
            return None;
        }
        // Interp aliases collapse onto one name so `#!/bin/sh`, `bash` and
        // `zsh` all index as the same language.
        return Some(match base {
            "sh" | "bash" | "zsh" | "dash" | "ksh" | "ash" => "shell".to_string(),
            "py" => "python".to_string(),
            "rb" => "ruby".to_string(),
            "node" | "nodejs" => "javascript".to_string(),
            other => other.to_string(),
        });
    }
    None
}

/// True when this extension is claimed as source code by the indexer.
pub fn is_code_extension(ext: &str) -> bool {
    language_for_extension(&ext.to_ascii_lowercase()).is_some()
}

pub struct CodeExtractor;

impl Extractor for CodeExtractor {
    fn name(&self) -> &'static str {
        "code"
    }

    fn supports(&self, format: Format) -> bool {
        matches!(format, Format::Code)
    }

    fn extract(&self, input: &Input<'_>, sink: &mut Sink) -> Result<(), ExtractError> {
        let text = match std::str::from_utf8(input.bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                // Source with a stray invalid byte: recover, do not fail.
                String::from_utf8_lossy(input.bytes).into_owned()
            }
        };
        let max = sink.limits().max_sections;
        // Section at blank-line paragraph boundaries; a block comment opener at
        // the start of a line also breaks the block so docs stay attached to
        // the code that follows them less often.
        let mut sections: Vec<Section> = Vec::new();
        let mut block = String::new();
        let mut start_line = 1usize;
        let mut line_no = 0usize;
        let mut prev_blank = true;
        for line in text.split_inclusive('\n') {
            line_no += 1;
            if line_no.is_multiple_of(1024) {
                sink.tick()?;
            }
            let trimmed = line.trim_start();
            let is_boundary = !prev_blank
                && (trimmed.starts_with("/*")
                    || trimmed.starts_with("///")
                    || trimmed.starts_with("#[")
                    || trimmed.starts_with("@decorator"));
            if line.trim().is_empty() || is_boundary {
                if !block.trim().is_empty() {
                    sections.push(Section {
                        kind: SectionKind::CodeBlock,
                        title: block_title(&block),
                        level: None,
                        locator: super::Locator::Lines {
                            start: start_line,
                            end: line_no.saturating_sub(1).max(start_line),
                        },
                        text: block.trim_end().to_string(),
                    });
                    block.clear();
                    if sections.len() >= max {
                        break;
                    }
                }
                start_line = if line.trim().is_empty() {
                    line_no + 1
                } else {
                    line_no
                };
                prev_blank = line.trim().is_empty();
                if !line.trim().is_empty() {
                    block.push_str(line);
                    prev_blank = false;
                }
                continue;
            }
            if block.is_empty() {
                start_line = line_no;
            }
            block.push_str(line);
            prev_blank = false;
        }
        if !block.trim().is_empty() && sections.len() < max {
            sections.push(Section {
                kind: SectionKind::CodeBlock,
                title: block_title(&block),
                level: None,
                locator: super::Locator::Lines {
                    start: start_line,
                    end: line_no.max(start_line),
                },
                text: block.trim_end().to_string(),
            });
        }
        if sections.len() >= max && line_no > 0 {
            sink.mark_truncated(format!("section cap of {max} reached"));
        }
        for section in sections {
            sink.tick()?;
            if !sink.push_section(section) {
                break;
            }
        }
        Ok(())
    }
}

/// Best-effort definition name for a code block: first `fn`/`def`/`class`/
/// `func` style line. Used only as a display hint, never as structure.
fn block_title(block: &str) -> Option<String> {
    // Modifiers are stripped so the title is the same whether or not the
    // definition is `pub`, `async` or `const`: `fn compute`, not
    // `pub fn compute`. Index titles have to be stable to be searchable.
    const MODIFIERS: &[&str] = &[
        "pub ",
        "pub(crate) ",
        "pub(super) ",
        "pub(in) ",
        "async ",
        "const ",
        "unsafe ",
        "export default ",
        "export ",
        "static ",
        "final ",
        "abstract ",
    ];

    for line in block.lines().take(12) {
        let mut t = line.trim();
        while let Some(m) = MODIFIERS.iter().find(|m| t.starts_with(**m)) {
            t = t[m.len()..].trim_start();
        }
        for keyword in [
            "fn ", "func ", "def ", "class ", "struct ", "enum ", "impl ", "trait ", "module ",
        ] {
            if let Some(rest) = t.strip_prefix(keyword) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
                    .collect();
                if !name.is_empty() {
                    return Some(format!("{keyword}{}", name.trim()));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{ExtractionLimits, ExtractorRegistry, Format, Locator};

    fn extract(name: &str, bytes: &[u8]) -> crate::extract::Document {
        let dir =
            std::env::temp_dir().join(format!("nanomite-code-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let doc = ExtractorRegistry::builtin()
            .extract_path(&path, &ExtractionLimits::default())
            .expect("extract");
        let _ = std::fs::remove_dir_all(&dir);
        doc
    }

    #[test]
    fn rust_source_is_labelled_and_sectioned() {
        let src =
            b"//! crate doc\n\nuse std::path::Path;\n\nfn main() {\n    println!(\"hi\");\n}\n";
        let doc = extract("main.rs", src);
        assert_eq!(doc.format, Format::Code);
        assert_eq!(doc.language.as_deref(), Some("rust"));
        assert!(!doc.sections.is_empty());
        assert!(doc.text.contains("println!"));
        assert!(doc
            .sections
            .iter()
            .any(|s| matches!(s.locator, Locator::Lines { .. })));
    }

    #[test]
    fn shebang_detects_interpreter_language() {
        assert_eq!(
            detect_language(
                std::path::Path::new("tool"),
                b"#!/usr/bin/env python3\nprint(1)\n"
            )
            .as_deref(),
            Some("python")
        );
        assert_eq!(
            detect_language(std::path::Path::new("run"), b"#!/bin/bash\necho hi\n").as_deref(),
            Some("shell")
        );
    }

    #[test]
    fn well_known_filenames_map_to_languages() {
        assert_eq!(
            language_for_filename("Dockerfile").as_deref(),
            Some("dockerfile")
        );
        assert_eq!(
            language_for_filename("Makefile").as_deref(),
            Some("makefile")
        );
        assert!(language_for_extension("rs").is_some());
        assert!(!is_code_extension("png"));
    }

    #[test]
    fn block_titles_surface_definitions() {
        let title = block_title("pub fn compute_total(items: &[u32]) -> u32 {\n    0\n}");
        assert_eq!(title.as_deref(), Some("fn compute_total"));
    }

    #[test]
    fn invalid_utf8_source_still_extract() {
        let doc = extract("bad.rs", &[b'f', b'n', b' ', 0xff, b'(']);
        assert!(!doc.text.is_empty());
    }

    #[test]
    fn code_is_never_evaluated_only_read() {
        // A file containing something executable must extract verbatim as data.
        let src = b"def run():\n    import os; os.system('true')\n";
        let doc = extract("evil.py", src);
        assert!(doc.text.contains("os.system"));
        assert_eq!(doc.language.as_deref(), Some("python"));
    }
}
