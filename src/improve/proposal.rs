//! Proposal data model and validation rules for the self-improvement engine.
//!
//! Every rule is enforced against caller-supplied [`Roots`]; nothing here
//! consults the environment, so tests can point the roots anywhere.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::ImproveError;

/// Hard cap on the size of a proposed replacement (`new_content`): 256 KiB.
pub const MAX_PROPOSAL_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Config,
    Prompt,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Agent,
    User,
}

impl Actor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Approved,
    Rejected,
    Applied,
    RolledBack,
    Failed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Applied => "applied",
            Self::RolledBack => "rolled_back",
            Self::Failed => "failed",
        }
    }
}

/// Caller-supplied anchor paths. The engine never discovers these from the
/// environment; the UI layer builds them once and passes them everywhere.
#[derive(Debug, Clone)]
pub struct Roots {
    pub config_path: PathBuf,
    pub prompt_path: PathBuf,
    pub repo_root: PathBuf,
}

/// A staged self-edit: the full replacement content plus provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub created_at: String,
    pub actor: Actor,
    pub target: Target,
    pub path: PathBuf,
    pub new_content: String,
    pub rationale: String,
    pub status: Status,
}

impl Proposal {
    /// Validates the proposal against caller-supplied roots, in check order:
    ///
    /// (a) the target file must already exist — v1 supports edits only, never
    ///     create/delete;
    /// (b) `new_content` is capped at [`MAX_PROPOSAL_BYTES`];
    /// (c) the extension must match the target kind (Config → `.toml`,
    ///     Prompt → `.md`, Source → `.rs`/`.toml`/`.md`);
    /// (d) the new content must differ from the current file content;
    /// (e) Config/Prompt paths must be exactly the root-supplied anchor paths,
    ///     and Source paths must resolve inside `repo_root` (lexical check
    ///     plus canonicalization, mirroring `memory::security`).
    pub fn validate(&self, roots: &Roots) -> Result<(), ImproveError> {
        if self.new_content.len() > MAX_PROPOSAL_BYTES {
            return Err(ImproveError::invalid(format!(
                "new content is {} bytes, over the {} byte cap",
                self.new_content.len(),
                MAX_PROPOSAL_BYTES
            )));
        }
        if !self.path.is_file() {
            return Err(ImproveError::invalid(format!(
                "target `{}` does not exist (v1 edits existing files only)",
                self.path.display()
            )));
        }
        if !extension_allowed(self.target, &self.path) {
            return Err(ImproveError::invalid(format!(
                "extension of `{}` is not allowed for a {} target",
                self.path.display(),
                self.target.as_str()
            )));
        }
        match self.target {
            Target::Config | Target::Prompt => {
                let anchor = match self.target {
                    Target::Config => &roots.config_path,
                    _ => &roots.prompt_path,
                };
                if lexical_normalize(&self.path) != lexical_normalize(anchor) {
                    return Err(ImproveError::policy(format!(
                        "{} target must be the caller-supplied path `{}`",
                        self.target.as_str(),
                        anchor.display()
                    )));
                }
            }
            Target::Source => ensure_within_repo(&self.path, &roots.repo_root)?,
        }
        let current = fs::read(&self.path).map_err(ImproveError::Io)?;
        if current == self.new_content.as_bytes() {
            return Err(ImproveError::invalid(
                "new content is identical to the current file content",
            ));
        }
        Ok(())
    }
}

impl Target {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Prompt => "prompt",
            Self::Source => "source",
        }
    }
}

fn extension_allowed(target: Target, path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    match target {
        Target::Config => ext == "toml",
        Target::Prompt => ext == "md",
        Target::Source => matches!(ext.as_str(), "rs" | "toml" | "md"),
    }
}

/// Confinement for Source targets: lexical containment first, then
/// canonicalization as the final authority against symlink escapes
/// (mirrors the layering of `memory::security::resolve_within`).
fn ensure_within_repo(path: &Path, repo_root: &Path) -> Result<(), ImproveError> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    };
    if !lexical_normalize(&joined).starts_with(lexical_normalize(repo_root)) {
        return Err(ImproveError::policy(format!(
            "source path `{}` escapes the repository root `{}`",
            path.display(),
            repo_root.display()
        )));
    }
    // The file exists (checked by the caller), so canonicalize must succeed.
    let real = joined.canonicalize().map_err(|e| {
        ImproveError::invalid(format!("cannot resolve `{}`: {e}", joined.display()))
    })?;
    let real_root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    if !real.starts_with(&real_root) {
        return Err(ImproveError::policy(format!(
            "resolved path of `{}` escapes the repository root (symlink escape)",
            path.display()
        )));
    }
    Ok(())
}

/// Lexical normalization without filesystem access (mirrors `memory::security`).
fn lexical_normalize(path: &Path) -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::{Actor, Proposal, Roots, Status, Target, MAX_PROPOSAL_BYTES};
    use crate::improve::testing::temp_dir;
    use crate::improve::ImproveError;
    use std::fs;
    use std::path::Path;

    fn roots(root: &Path) -> Roots {
        Roots {
            config_path: root.join("config.toml"),
            prompt_path: root.join("prompt.md"),
            repo_root: root.to_path_buf(),
        }
    }

    fn proposal(target: Target, path: &Path, new_content: &str) -> Proposal {
        Proposal {
            id: "test".to_string(),
            created_at: "1970-01-01T00:00:00.000Z".to_string(),
            actor: Actor::Agent,
            target,
            path: path.to_path_buf(),
            new_content: new_content.to_string(),
            rationale: "test".to_string(),
            status: Status::Pending,
        }
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn valid_source_edit_passes() {
        let temp = temp_dir("valid-source");
        let root = &temp.path;
        write(&root.join("src/lib.rs"), "pub fn a() {}\n");
        let p = proposal(Target::Source, &root.join("src/lib.rs"), "pub fn b() {}\n");
        assert!(p.validate(&roots(root)).is_ok());
    }

    #[test]
    fn missing_target_file_is_rejected() {
        let temp = temp_dir("missing-file");
        let root = &temp.path;
        let p = proposal(Target::Source, &root.join("src/missing.rs"), "x\n");
        let err = p.validate(&roots(root)).unwrap_err();
        assert!(matches!(err, ImproveError::Invalid(_)), "{err:?}");
    }

    #[test]
    fn oversized_content_is_rejected() {
        let temp = temp_dir("too-big");
        let root = &temp.path;
        write(&root.join("src/big.rs"), "x\n");
        let huge = "x".repeat(MAX_PROPOSAL_BYTES + 1);
        let p = proposal(Target::Source, &root.join("src/big.rs"), &huge);
        let err = p.validate(&roots(root)).unwrap_err();
        assert!(matches!(err, ImproveError::Invalid(_)), "{err:?}");
    }

    #[test]
    fn content_at_the_cap_is_allowed() {
        let temp = temp_dir("cap-ok");
        let root = &temp.path;
        let old = "a".repeat(MAX_PROPOSAL_BYTES);
        let new = "b".repeat(MAX_PROPOSAL_BYTES);
        write(&root.join("src/big.rs"), &old);
        let p = proposal(Target::Source, &root.join("src/big.rs"), &new);
        assert!(p.validate(&roots(root)).is_ok());
    }

    #[test]
    fn extensions_are_enforced_per_target() {
        let temp = temp_dir("extensions");
        let root = &temp.path;
        write(&root.join("notes.md"), "x\n");
        write(&root.join("config.md"), "x\n");
        write(&root.join("prompt.txt"), "x\n");
        write(&root.join("src/code.txt"), "x\n");
        let r = roots(root);
        // Config wants .toml
        assert!(proposal(Target::Config, &root.join("config.md"), "y\n")
            .validate(&r)
            .is_err());
        // Prompt wants .md
        assert!(proposal(Target::Prompt, &root.join("prompt.txt"), "y\n")
            .validate(&r)
            .is_err());
        // Source wants .rs/.toml/.md
        assert!(proposal(Target::Source, &root.join("src/code.txt"), "y\n")
            .validate(&r)
            .is_err());
        // (the .md Source case is covered by valid_source_edit_passes)
    }

    #[test]
    fn config_and_prompt_targets_must_use_the_root_paths() {
        let temp = temp_dir("binding");
        let root = &temp.path;
        write(&root.join("other.toml"), "x = \"1\"\n");
        write(&root.join("other.md"), "x\n");
        write(&root.join("config.toml"), "x = \"1\"\n");
        write(&root.join("prompt.md"), "x\n");
        let r = roots(root);
        assert!(
            proposal(Target::Config, &root.join("other.toml"), "y = \"2\"\n")
                .validate(&r)
                .is_err()
        );
        assert!(proposal(Target::Prompt, &root.join("other.md"), "y\n")
            .validate(&r)
            .is_err());
        // The anchored paths themselves validate.
        assert!(
            proposal(Target::Config, &root.join("config.toml"), "y = \"2\"\n")
                .validate(&r)
                .is_ok()
        );
        assert!(proposal(Target::Prompt, &root.join("prompt.md"), "y\n")
            .validate(&r)
            .is_ok());
    }

    #[test]
    fn safety_layer_paths_validate_for_source_targets() {
        // The self-edit denylist was removed by owner decision (2026-10-30):
        // proposals against the former safety-layer files validate like any
        // other Source target and reach the /improve approval gate.
        let temp = temp_dir("safety-stageable");
        let root = &temp.path;
        for rel in [
            "src/memory/security.rs",
            "src/memory/policy.rs",
            "src/main.rs",
            "src/memory/nested/security.rs",
            "src/memory/deeper/policy.rs",
        ] {
            write(&root.join(rel), "pub fn never() {}\n");
            let p = proposal(Target::Source, &root.join(rel), "pub fn edited() {}\n");
            assert!(
                p.validate(&roots(root)).is_ok(),
                "{rel} should validate without the denylist"
            );
        }
    }

    #[test]
    fn identical_content_is_rejected() {
        let temp = temp_dir("identical");
        let root = &temp.path;
        write(&root.join("src/lib.rs"), "pub fn a() {}\n");
        let p = proposal(Target::Source, &root.join("src/lib.rs"), "pub fn a() {}\n");
        let err = p.validate(&roots(root)).unwrap_err();
        assert!(format!("{err}").contains("identical"), "{err}");
    }

    #[test]
    fn source_paths_must_stay_inside_the_repo_root() {
        let temp = temp_dir("outside");
        let root = &temp.path;
        // A sibling of the fake repo root, reachable via `..`.
        let sibling = temp
            .path
            .parent()
            .unwrap()
            .join(format!("outside-{}.toml", std::process::id()));
        write(&sibling, "x = \"1\"\n");
        let p = proposal(Target::Source, &sibling, "x = \"2\"\n");
        let err = p.validate(&roots(root)).unwrap_err();
        assert!(matches!(err, ImproveError::Policy(_)), "{err:?}");
        let _ = fs::remove_file(&sibling);
    }

    #[test]
    fn symlink_escape_is_rejected() {
        let temp = temp_dir("symlink");
        let root = &temp.path;
        let sibling = temp
            .path
            .parent()
            .unwrap()
            .join(format!("outside-{}.toml", std::process::id()));
        write(&sibling, "x = \"1\"\n");
        write(&root.join("src/inner.toml"), "y = \"1\"\n");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&sibling, root.join("src/link.toml")).unwrap();
        #[cfg(unix)]
        {
            let p = proposal(Target::Source, &root.join("src/link.toml"), "x = \"2\"\n");
            let err = p.validate(&roots(root)).unwrap_err();
            assert!(matches!(err, ImproveError::Policy(_)), "{err:?}");
        }
        let _ = fs::remove_file(&sibling);
    }
}
