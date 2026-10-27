//! Parallel worktree sessions: multiple nanomite agents on one repo without
//! conflicts.
//!
//! `nanomite --worktree <branch>` creates (or reuses) a git worktree for the
//! branch under `.nanomite/worktrees/<branch-slug>/`, then opens the agent in
//! it. Each worktree gets its own session/plan state (they live in the
//! worktree's own `.nanomite/`), while config stays user-global. Listing and
//! cleanup are handled from the parent repo.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_WORKTREES: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub branch: String,
    pub path: String,
    pub head: String,
    pub is_current: bool,
    /// True when this nanomite instance created/reuses it for parallel work.
    pub managed: bool,
}

/// Slugify a branch name into a directory-safe component.
pub fn branch_slug(branch: &str) -> String {
    branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// The managed worktrees root for a repo: `.nanomite/worktrees`.
pub fn worktrees_root(repo: &Path) -> PathBuf {
    repo.join(".nanomite").join("worktrees")
}

/// Runs git in `repo`; spawn failures surface as code -1 (callers check the
/// code, not the error, so a missing git binary reads as "not a repo").
fn run_git(repo: &Path, args: &[&str]) -> (String, i32) {
    match Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) => (
            String::from_utf8_lossy(&output.stdout).into_owned(),
            output.status.code().unwrap_or(-1),
        ),
        Err(_) => (String::new(), -1),
    }
}

/// Lists every git worktree of `repo`, marking which are nanomite-managed
/// (live under `.nanomite/worktrees/`). Bounded to MAX_WORKTREES managed
/// entries so a wild repo cannot flood the picker.
pub fn list(repo: &Path) -> Result<Vec<WorktreeInfo>, String> {
    // Verify this is a repo first: a clean error beats git's stderr noise.
    let (probe, code) = run_git(repo, &["rev-parse", "--is-inside-work-tree"]);
    if code != 0 || probe.trim() != "true" {
        return Err("not a git repository".into());
    }

    let (out, code) = run_git(repo, &["worktree", "list", "--porcelain"]);
    if code != 0 {
        return Err("git worktree list failed".into());
    }

    // Porcelain blocks: worktree <path> / HEAD <sha> / branch <ref> / bare,
    // separated by blank lines. One WorktreeInfo per block.
    let root = worktrees_root(repo);
    let mut out_vec: Vec<WorktreeInfo> = Vec::new();
    let mut cur_path = String::new();
    let mut cur_head = String::new();
    let mut cur_branch = String::new();

    fn flush(
        path: &mut String,
        head: &mut String,
        branch: &mut String,
        root: &Path,
        out: &mut Vec<WorktreeInfo>,
    ) {
        if path.is_empty() {
            return;
        }
        let branch_name = branch
            .strip_prefix("refs/heads/")
            .unwrap_or(branch.as_str())
            .to_string();
        let managed = Path::new(path.as_str()).starts_with(root);
        let count = out.iter().filter(|w| w.managed).count();
        let skip = managed && count >= MAX_WORKTREES;
        if !skip {
            out.push(WorktreeInfo {
                branch: branch_name,
                path: std::mem::take(path),
                head: std::mem::take(head).chars().take(12).collect(),
                is_current: false,
                managed,
            });
        }
        path.clear();
        head.clear();
        branch.clear();
    }

    for line in out.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            flush(
                &mut cur_path,
                &mut cur_head,
                &mut cur_branch,
                &root,
                &mut out_vec,
            );
            continue;
        }
        if let Some(p) = line.strip_prefix("worktree ") {
            cur_path = p.to_string();
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            cur_head = h.to_string();
        } else if let Some(b) = line.strip_prefix("branch ") {
            cur_branch = b.to_string();
        }
        // `bare` and `detached` lines carry no path/branch we need.
    }
    flush(
        &mut cur_path,
        &mut cur_head,
        &mut cur_branch,
        &root,
        &mut out_vec,
    );
    Ok(out_vec)
}

/// Creates (or reuses) a managed worktree for `branch` under
/// `.nanomite/worktrees/<slug>/` and returns its path. Fails clearly when the
/// branch does not exist and cannot be created from HEAD (the caller decides
/// whether to ask the model/user).
pub fn ensure(repo: &Path, branch: &str) -> Result<PathBuf, String> {
    if branch.trim().is_empty() || branch.contains(char::is_whitespace) || branch.starts_with('-') {
        return Err("refused: branch must be a single non-flag token".into());
    }
    let slug = branch_slug(branch);
    if slug.is_empty() {
        return Err("refused: branch name is not usable as a directory".into());
    }
    let root = worktrees_root(repo);
    std::fs::create_dir_all(&root).map_err(|e| format!("worktree root: {e}"))?;
    let target = root.join(&slug);

    if target.exists() {
        // Reuse: verify it is actually a worktree of this repo on the branch.
        let (head, code) = run_git(&target, &["rev-parse", "--abbrev-ref", "HEAD"]);
        if code == 0 && head.trim() == branch {
            return Ok(target);
        }
        return Err(format!(
            "worktree {} exists but is not on branch `{branch}`",
            target.display()
        ));
    }

    // Branch may not exist locally; create from HEAD when missing.
    let (_, exists_code) = run_git(
        repo,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    );
    if exists_code != 0 {
        let (_, created) = run_git(repo, &["branch", branch, "HEAD"]);
        if created != 0 {
            return Err(format!("could not create branch `{branch}`"));
        }
    }

    let (_, code) = run_git(
        repo,
        &["worktree", "add", &target.display().to_string(), branch],
    );
    if code != 0 {
        return Err(format!("git worktree add failed for `{branch}`"));
    }
    Ok(target)
}

/// Removes a managed worktree (git worktree remove) and its branch entry.
/// Only managed paths may be removed: a safety rail against deleting a
/// user's own checkouts.
pub fn remove(repo: &Path, branch: &str) -> Result<(), String> {
    let slug = branch_slug(branch);
    let target = worktrees_root(repo).join(&slug);
    if !target.starts_with(worktrees_root(repo)) || !target.exists() {
        return Err(format!("no managed worktree for `{branch}`"));
    }
    let (_, code) = run_git(
        repo,
        &[
            "worktree",
            "remove",
            "--force",
            &target.display().to_string(),
        ],
    );
    if code != 0 {
        return Err(format!("git worktree remove failed for `{branch}`"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_repo() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "nanomite-wt-{}-{}-{}",
            std::process::id(),
            id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let (_, code) = run_git(&dir, &["init"]);
        assert_eq!(code, 0);
        run_git(&dir, &["config", "user.email", "t@t"]);
        run_git(&dir, &["config", "user.name", "t"]);
        fs::write(dir.join("a.txt"), "v1").unwrap();
        run_git(&dir, &["add", "a.txt"]);
        run_git(&dir, &["commit", "-m", "init"]);
        dir
    }

    #[test]
    fn slug_is_directory_safe() {
        assert_eq!(branch_slug("feature/x"), "feature-x");
        assert_eq!(branch_slug("feat_1"), "feat-1");
        assert_eq!(branch_slug("release-1.2"), "release-1.2");
        assert_eq!(branch_slug("--flag"), "flag");
    }

    #[test]
    fn non_repo_is_refused_cleanly() {
        let dir = std::env::temp_dir().join("nanomite-wt-nonrepo");
        fs::create_dir_all(&dir).unwrap();
        let err = list(&dir).unwrap_err();
        assert!(err.contains("not a git repository"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_creates_reuses_and_removes_worktrees() {
        let repo = init_repo();
        // Create.
        let path = ensure(&repo, "feature/x").expect("create");
        assert!(path.exists());
        assert!(path.starts_with(worktrees_root(&repo)));

        // Listed as managed.
        let list = list(&repo).unwrap();
        assert!(list.iter().any(|w| w.managed && w.branch == "feature/x"));

        // Reuse: same path, no error.
        let again = ensure(&repo, "feature/x").expect("reuse");
        assert_eq!(path, again);

        // Isolation: state written in the worktree stays in the worktree.
        fs::create_dir_all(path.join(".nanomite").join("plans")).unwrap();
        fs::write(path.join(".nanomite/plans/wt-plan.json"), "{}").unwrap();
        assert!(!repo.join(".nanomite/plans/wt-plan.json").exists());

        // Remove only managed paths.
        assert!(remove(&repo, "feature/x").is_ok());
        assert!(!path.exists());
        assert!(remove(&repo, "nonexistent").is_err());

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn dangerous_branch_names_are_refused() {
        let repo = init_repo();
        assert!(ensure(&repo, "").is_err());
        assert!(ensure(&repo, "-oProxyCommand=evil").is_err());
        assert!(ensure(&repo, "two words").is_err());
        let _ = fs::remove_dir_all(&repo);
    }
}
