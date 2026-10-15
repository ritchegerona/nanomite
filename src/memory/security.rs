//! Security helpers: credential detection, content hashing and path confinement.
//!
//! Two invariants live here:
//!   1. credentials never enter a memory record or a markdown document, and
//!   2. no caller-supplied path can escape the memory root, through `..` or a symlink.

use super::model::{MemoryError, MemoryRecord};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// Where a credential was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretMatch {
    /// Byte offsets of the offending span.
    pub start: usize,
    pub end: usize,
    /// Detector name, e.g. `openai_key`.
    pub kind: &'static str,
}

/// What a redacted span is replaced with: `[redacted:openai_key]`.
pub const REDACT_PREFIX: &str = "[redacted:";

// -- hashing / identifiers ---------------------------------------------------

/// FNV-1a, 64 bit. Fast, dependency-free and good enough for duplicate
/// detection and cache keys. **Not** a cryptographic digest and never used as one.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Duplicate-detection digest: case-folded, whitespace-collapsed content.
pub fn content_hash(content: &str) -> String {
    let mut normalized = String::with_capacity(content.len());
    let mut pending_space = false;
    for ch in content.trim().chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !normalized.is_empty() {
            normalized.push(' ');
        }
        pending_space = false;
        for lower in ch.to_lowercase() {
            normalized.push(lower);
        }
    }
    format!("{:016x}", fnv1a64(normalized.as_bytes()))
}

/// Deterministic identifier: `mem_<16 hex>`.
///
/// Uniqueness is enforced by the primary key, not by this function: the caller
/// retries with a bumped `salt` on conflict.
pub fn new_id(now: i64, seq: u64, salt: &str) -> String {
    let mix = fnv1a64(format!("{now}:{seq}:{}", content_hash(salt)).as_bytes());
    format!("mem_{mix:016x}")
}

// -- credential detection ----------------------------------------------------

/// Named detectors, most specific first. Each returns every match it finds.
type Detector = (&'static str, fn(&str) -> Vec<(usize, usize)>);

const DETECTORS: &[Detector] = &[
    ("private_key_block", private_key_block),
    ("putty_private_key", putty_private_key),
    ("aws_access_key", aws_access_key),
    ("google_api_key", google_api_key),
    ("openai_key", openai_key),
    ("stripe_key", stripe_key),
    ("github_token", github_token),
    ("slack_token", slack_token),
    ("huggingface_token", huggingface_token),
    ("jwt", jwt),
    ("bearer_token", bearer_token),
    ("ssh_private_blob", ssh_private_blob),
    ("credential_assignment", credential_assignment),
];

/// First credential found, scanning detectors in a fixed order (deterministic).
pub fn find_secret(text: &str) -> Option<SecretMatch> {
    all_secrets(text).into_iter().next()
}

/// Every credential found, ordered by position.
pub fn all_secrets(text: &str) -> Vec<SecretMatch> {
    let mut found = Vec::new();
    for (kind, detector) in DETECTORS {
        for (start, end) in detector(text) {
            found.push(SecretMatch { start, end, kind });
        }
    }
    found.sort_by_key(|m| (m.start, m.end));
    // Overlapping detections (an `sk-ant-` hit inside a generic `sk-` hit) collapse to one.
    let mut merged: Vec<SecretMatch> = Vec::with_capacity(found.len());
    for candidate in found {
        if candidate.start >= candidate.end {
            continue;
        }
        match merged.last_mut() {
            Some(last) if candidate.start < last.end => {
                last.end = last.end.max(candidate.end);
            }
            _ => merged.push(candidate),
        }
    }
    merged
}

/// `Ok(())` when `text` is safe to persist, otherwise an actionable refusal.
pub fn reject_secrets(field: &str, text: &str) -> Result<(), MemoryError> {
    match find_secret(text) {
        Some(found) => Err(MemoryError::Sensitive(format!(
            "{field} looks like a {}. Memory never stores credentials - remove it, \
             or keep the value in an environment variable or secret manager.",
            humanize(found.kind)
        ))),
        None => Ok(()),
    }
}

/// Replaces every credential with `[redacted:<kind>]`.
///
/// Used on the way *out* (prompt injection, exports) as defence in depth, in
/// case a record predates a detector or was written by another tool.
pub fn redact(text: &str) -> String {
    let matches = all_secrets(text);
    if matches.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for found in matches {
        out.push_str(&text[cursor..found.start]);
        out.push_str(REDACT_PREFIX);
        out.push_str(found.kind);
        out.push(']');
        cursor = found.end;
    }
    out.push_str(&text[cursor..]);
    out
}

fn humanize(kind: &str) -> String {
    format!("{} ({})", kind.replace('_', " "), "credential")
}

fn private_key_block(text: &str) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find("-----BEGIN") {
        let start = cursor + rel;
        let Some(rel_end) = text[start..].find("PRIVATE KEY-----") else {
            break;
        };
        let end = start + rel_end + "PRIVATE KEY-----".len();
        hits.push((start, end));
        cursor = end;
    }
    hits
}

fn putty_private_key(text: &str) -> Vec<(usize, usize)> {
    find_all(text, "PuTTY-User-Key-File", 0)
}

fn aws_access_key(text: &str) -> Vec<(usize, usize)> {
    prefixed_runs(text, &["AKIA", "ASIA", "ABIA", "ACCA"], 16, |c| {
        c.is_ascii_uppercase() || c.is_ascii_digit()
    })
}

fn google_api_key(text: &str) -> Vec<(usize, usize)> {
    prefixed_runs(text, &["AIza"], 35, |c| {
        c.is_ascii_alphanumeric() || c == '_' || c == '-'
    })
}

fn openai_key(text: &str) -> Vec<(usize, usize)> {
    prefixed_runs(text, &["sk-"], 20, |c| {
        c.is_ascii_alphanumeric() || c == '_' || c == '-'
    })
}

fn stripe_key(text: &str) -> Vec<(usize, usize)> {
    prefixed_runs(text, &["sk_live_", "rk_live_", "pk_live_"], 10, |c| {
        c.is_ascii_alphanumeric()
    })
}

fn github_token(text: &str) -> Vec<(usize, usize)> {
    prefixed_runs(
        text,
        &["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"],
        22,
        |c| c.is_ascii_alphanumeric() || c == '_',
    )
}

fn slack_token(text: &str) -> Vec<(usize, usize)> {
    prefixed_runs(
        text,
        &["xoxb-", "xoxp-", "xoxa-", "xoxr-", "xoxs-"],
        10,
        |c| c.is_ascii_alphanumeric() || c == '-',
    )
}

fn huggingface_token(text: &str) -> Vec<(usize, usize)> {
    prefixed_runs(text, &["hf_"], 24, |c| c.is_ascii_alphanumeric())
}

fn jwt(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut hits = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find("eyJ") {
        let start = cursor + rel;
        let body: Vec<usize> = bytes
            .iter()
            .enumerate()
            .skip(start)
            .take_while(|(_, b)| {
                let c = **b as char;
                c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'
            })
            .map(|(i, _)| i)
            .collect();
        let end = body.last().copied().unwrap_or(start + 3);
        let token = &text[start..end];
        if end - start >= 40 && token.matches('.').count() == 2 {
            hits.push((start, end));
        }
        cursor = (start + 3).max(end.min(text.len()));
    }
    hits
}

fn bearer_token(text: &str) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    let lower = text.to_lowercase();
    let mut cursor = 0usize;
    while let Some(rel) = lower[cursor..].find("bearer ") {
        let start = cursor + rel + "bearer ".len();
        let bytes = text.as_bytes();
        let mut end = start;
        while end < text.len() {
            let c = bytes[end] as char;
            if !(c.is_ascii_alphanumeric()
                || c == '-'
                || c == '_'
                || c == '.'
                || c == '+'
                || c == '/'
                || c == '=')
            {
                break;
            }
            end += 1;
        }
        if end - start >= 20 {
            hits.push((start, end));
        }
        cursor = end.max(start);
    }
    hits
}

fn ssh_private_blob(text: &str) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    for marker in ["ssh-rsa ", "ssh-ed25519 ", "ecdsa-sha2-nistp256 "] {
        let mut cursor = 0usize;
        while let Some(rel) = text[cursor..].find(marker) {
            let start = cursor + rel;
            let body_start = start + marker.len();
            let bytes = text.as_bytes();
            let mut end = body_start;
            while end < text.len()
                && ((bytes[end] as char).is_ascii_alphanumeric()
                    || matches!(bytes[end], b'+' | b'/' | b'='))
            {
                end += 1;
            }
            if end - body_start >= 80 {
                hits.push((body_start, end));
            }
            cursor = end.max(start + 1);
        }
    }
    hits
}

/// `key = value` / `"password": "value"` forms, skipping obvious placeholders.
fn credential_assignment(text: &str) -> Vec<(usize, usize)> {
    const KEYS: &[&str] = &[
        "api_key",
        "apikey",
        "api-key",
        "access_key",
        "access_token",
        "auth_token",
        "client_secret",
        "password",
        "passwd",
        "private_key",
        "secret_key",
        "secret",
        "token",
    ];
    let lower = text.to_lowercase();
    let bytes = text.as_bytes();
    let mut hits = Vec::new();

    for key in KEYS {
        let mut cursor = 0usize;
        while let Some(rel) = lower[cursor..].find(key) {
            let key_start = cursor + rel;
            let mut at = key_start + key.len();
            // Require a word boundary so `tokenizer` or `secretary` do not match.
            if key_start > 0 && is_word_byte(bytes[key_start - 1]) {
                cursor = at;
                continue;
            }
            // Skip optional quoting and spaces, then require ':' or '='.
            while at < bytes.len() && matches!(bytes[at], b' ' | b'\t' | b'"' | b'\'') {
                at += 1;
            }
            if at >= bytes.len() || !matches!(bytes[at], b':' | b'=') {
                cursor = key_start + key.len();
                continue;
            }
            at += 1;
            while at < bytes.len() && matches!(bytes[at], b' ' | b'\t' | b'"' | b'\'') {
                at += 1;
            }
            let value_start = at;
            while at < bytes.len() && !(bytes[at] as char).is_whitespace() {
                at += 1;
            }
            let value = text[value_start..at].trim_matches(['"', '\'', ',', ';']);
            if value.len() >= 8 && !is_placeholder(value) {
                hits.push((value_start, at));
            }
            cursor = at.max(key_start + key.len());
        }
    }
    hits
}

/// Documentation samples ("api_key = your-key-here") must not trip the filter.
fn is_placeholder(value: &str) -> bool {
    let lower = value.to_lowercase();
    if lower.contains('<') || lower.contains('{') || lower.contains("${") {
        return true;
    }
    const FILLERS: &[&str] = &[
        "your",
        "example",
        "changeme",
        "change_me",
        "placeholder",
        "redacted",
        "dummy",
        "xxxxx",
        "todo",
        "none",
        "null",
        "unset",
        "sample",
        "replace",
        "abcdef",
    ];
    if FILLERS.iter().any(|f| lower.contains(f)) {
        return true;
    }
    // A value that is one repeated character carries no entropy: not a credential.
    let mut chars = value.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) => value.chars().all(|c| c == first) && first == second,
        _ => false,
    }
}

fn is_word_byte(byte: u8) -> bool {
    (byte as char).is_ascii_alphanumeric() || byte == b'_'
}

fn find_all(text: &str, needle: &str, extra: usize) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find(needle) {
        let start = cursor + rel;
        let end = start + needle.len() + extra;
        hits.push((start, end.min(text.len())));
        cursor = start + needle.len();
    }
    hits
}

/// Finds `<prefix><run>` where `run` is at least `min` accepted characters.
fn prefixed_runs(
    text: &str,
    prefixes: &[&str],
    min: usize,
    accept: impl Fn(char) -> bool,
) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    for prefix in prefixes {
        let mut cursor = 0usize;
        while let Some(rel) = text[cursor..].find(prefix) {
            let start = cursor + rel;
            let body = start + prefix.len();
            let mut end = body;
            for ch in text[body..].chars() {
                if accept(ch) {
                    end += ch.len_utf8();
                } else {
                    break;
                }
            }
            if end - body >= min {
                hits.push((start, end));
                cursor = end;
            } else {
                cursor = start + prefix.len();
            }
        }
    }
    hits
}

// -- path confinement --------------------------------------------------------

/// Owner-only permissions for memory directories and databases.
#[cfg(unix)]
pub const DIR_MODE: u32 = 0o700;
#[cfg(unix)]
pub const FILE_MODE: u32 = 0o600;

/// Resolves `relative` inside `root`, refusing traversal and symlink escapes.
///
/// The check is three-layered because each catches what the others miss:
/// lexical normalization catches `..`, component inspection catches embedded
/// symlinks, and canonicalization catches a root that is itself a symlink.
pub fn resolve_within(root: &Path, relative: &str) -> Result<PathBuf, MemoryError> {
    let trimmed = relative.trim();
    if trimmed.is_empty() {
        return Err(MemoryError::invalid("path must not be empty"));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(MemoryError::invalid(
            "path must not contain control characters",
        ));
    }
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() || trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(MemoryError::Policy(
            "refused: absolute paths are not allowed in the memory store".into(),
        ));
    }
    if candidate
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(MemoryError::Policy(
            "refused: path traversal (`..`) is not allowed".into(),
        ));
    }
    if candidate
        .components()
        .any(|c| matches!(c, Component::RootDir | Component::Prefix(_)))
    {
        return Err(MemoryError::Policy(
            "refused: non-relative path component".into(),
        ));
    }

    let root = root.to_path_buf();
    let joined = lexical_normalize(&root.join(candidate));
    if !joined.starts_with(&root) {
        return Err(MemoryError::Policy(
            "refused: path escapes the memory root".into(),
        ));
    }

    // Every existing ancestor must be a real directory, not a symlink out of the root.
    let mut probe = joined.as_path();
    loop {
        if let Ok(meta) = std::fs::symlink_metadata(probe) {
            if meta.file_type().is_symlink() {
                return Err(MemoryError::Policy(format!(
                    "refused: `{}` is a symlink; the memory store never follows links",
                    relative
                )));
            }
            if !meta.is_dir() && probe != joined {
                return Err(MemoryError::Policy(format!(
                    "refused: `{}` traverses a non-directory",
                    relative
                )));
            }
        }
        // Missing components are fine for a create; the deepest existing
        // ancestor has already been checked.
        if probe == root || !probe.starts_with(&root) {
            break;
        }
        match probe.parent() {
            Some(parent) => probe = parent,
            None => break,
        }
    }

    // If the target exists, canonicalization is the final authority.
    if let Ok(real) = joined.canonicalize() {
        let real_root = root.canonicalize().unwrap_or_else(|_| root.clone());
        if !real.starts_with(&real_root) {
            return Err(MemoryError::Policy(
                "refused: resolved path escapes the memory root".into(),
            ));
        }
    }
    Ok(joined)
}

/// Lexical normalization, no filesystem access (mirrors `tools::file_tools::normalize`).
pub fn lexical_normalize(path: &Path) -> PathBuf {
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

/// Creates `dir` (and parents) with owner-only permissions.
pub fn ensure_dir(dir: &Path) -> Result<(), MemoryError> {
    if let Err(e) = std::fs::create_dir_all(dir) {
        return Err(MemoryError::io(&format!("create {}", dir.display()), e));
    }
    restrict(dir, true);
    Ok(())
}

/// Best-effort permission tightening; ignored where the platform forbids it.
pub fn restrict(path: &Path, dir: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if dir { DIR_MODE } else { FILE_MODE };
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, dir);
    }
}

/// Atomic replace: write to a sibling temp file, fsync, rename, fsync the directory.
///
/// Either the old file or the new file is visible after a crash — never a
/// half-written document.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), MemoryError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    ensure_dir(parent)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "memory".to_string());
    let tmp = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));

    let write = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(e) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(MemoryError::io(&format!("write {}", tmp.display()), e));
    }
    restrict(&tmp, false);
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(MemoryError::io(&format!("replace {}", path.display()), e));
    }
    // Durability of the rename itself.
    if let Ok(dir_handle) = std::fs::File::open(parent) {
        let _ = dir_handle.sync_all();
    }
    restrict(path, false);
    Ok(())
}

/// Convenience wrapper used by the store and the markdown layer before persisting.
pub fn assert_record_safe(record: &MemoryRecord) -> Result<(), MemoryError> {
    reject_secrets("memory content", &record.content)?;
    if let Some(summary) = &record.summary {
        reject_secrets("memory summary", summary)?;
    }
    for tag in &record.tags {
        reject_secrets("memory tag", tag)?;
    }
    if let Some(detail) = &record.source.detail {
        reject_secrets("memory source", detail)?;
    }
    if let Some(project) = &record.project {
        reject_secrets("project key", project)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_credential_shapes() {
        let cases: &[(&str, &str)] = &[
            ("sk-abcdefghijklmnopqrstuvwxyz0123456789", "openai_key"),
            ("AKIAIOSFODNN7EXAMPLE", "aws_access_key"),
            ("AIzaSyA1234567890abcdefghijklmnopqrstuvw", "google_api_key"),
            ("ghp_abcdefghijklmnopqrstuvwxyz0123456789", "github_token"),
            ("xoxb-123456789012-abcdefghij", "slack_token"),
            (
                "-----BEGIN RSA PRIVATE KEY-----\nMIIEow==\n-----END RSA PRIVATE KEY-----",
                "private_key_block",
            ),
            (
                "Authorization: Bearer abcdefghijklmnopqrstuvwxyz",
                "bearer_token",
            ),
            ("password = hunter2secret", "credential_assignment"),
            (
                "api_key: \"AKIA-not-real-but-long-enough\"",
                "credential_assignment",
            ),
        ];
        for (text, expected) in cases {
            let found = find_secret(text).unwrap_or_else(|| panic!("missed {text:?}"));
            assert_eq!(found.kind, *expected, "wrong detector for {text:?}");
        }
    }

    #[test]
    fn documentation_samples_are_not_treated_as_secrets() {
        for text in [
            "api_key = your-key-here",
            "password: <redacted>",
            "token: ${GITHUB_TOKEN}",
            "secret = changeme",
            "the tokenizer splits text into tokens",
            "password = \"xxxxxxxx\"",
            "sk-",
        ] {
            assert!(find_secret(text).is_none(), "false positive on {text:?}");
        }
    }

    #[test]
    fn redact_removes_every_credential() {
        let text = "key sk-abcdefghijklmnopqrstuvwxyz0123456789 and AKIAIOSFODNN7EXAMPLE end";
        let out = redact(text);
        assert!(!out.contains("sk-abcdefghijklmnopqrstu"));
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(out.contains("[redacted:"));
        assert!(out.ends_with(" end"));
    }

    #[test]
    fn content_hash_ignores_case_and_whitespace() {
        assert_eq!(
            content_hash("Deploy  on\n fridays"),
            content_hash("deploy on fridays")
        );
        assert_ne!(content_hash("a"), content_hash("b"));
    }

    #[test]
    fn ids_are_prefixed_and_distinct() {
        let a = new_id(100, 1, "alpha");
        let b = new_id(100, 2, "alpha");
        assert!(a.starts_with("mem_") && a.len() == 20);
        assert_ne!(a, b);
    }

    #[test]
    fn resolve_within_blocks_traversal_and_absolute_paths() {
        let root = std::env::temp_dir();
        assert!(resolve_within(&root, "../etc/passwd").is_err());
        assert!(resolve_within(&root, "/etc/passwd").is_err());
        assert!(resolve_within(&root, "a/../../b").is_err());
        assert!(resolve_within(&root, "").is_err());
        assert!(resolve_within(&root, "projects/nanomite.md").is_ok());
    }

    #[test]
    fn resolve_within_rejects_symlinks() {
        let base = std::env::temp_dir().join(format!("nanomite-sec-{}", std::process::id()));
        let root = base.join("memory");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.md"), "leak").unwrap();

        #[cfg(unix)]
        {
            let link = root.join("escape.md");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(outside.join("secret.md"), &link).unwrap();
            let err = resolve_within(&root, "escape.md").unwrap_err();
            assert!(err.to_string().contains("symlink"), "{err}");
        }

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn atomic_write_replaces_content() {
        let dir = std::env::temp_dir().join(format!("nanomite-atomic-{}", std::process::id()));
        ensure_dir(&dir).unwrap();
        let file = dir.join("doc.md");
        write_atomic(&file, b"one").unwrap();
        write_atomic(&file, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "two");
        // No temp files left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
