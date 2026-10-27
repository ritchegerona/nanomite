//! Git workspace tools: status, diff, log, commit, branch.
//!
//! All commands run `git` directly (no shell) inside the workspace, with the
//same timeout and output bounds as `run_command`. Inspection tools are
//! `Safe`; history-mutating tools (`commit`, `branch`) are `Write` and
//! controlled by the central ToolPolicy (see `agent/policy.rs`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Hard cap for one git command.
const MAX_TIMEOUT_MS: u64 = 120_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatusArgs {
    /// Workspace-relative path to a file or directory. Empty means the whole workspace.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatusResult {
    pub path: String,
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub staged: Vec<String>,
    pub unstaged: Vec<String>,
    pub untracked: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDiffArgs {
    /// Workspace-relative file or directory to diff. Empty means all changes.
    #[serde(default)]
    pub path: Option<String>,
    /// Show staged (index) instead of working-tree changes.
    #[serde(default)]
    pub staged: bool,
    /// Include untracked files as new content.
    #[serde(default)]
    pub include_untracked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDiffResult {
    pub path: String,
    pub diff: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLogArgs {
    /// Number of commits to show (default 20, max 200).
    #[serde(default = "default_log_limit")]
    pub limit: usize,
    /// Optional path to limit history to.
    #[serde(default)]
    pub path: Option<String>,
    /// Include full diff per commit (bounded by output cap).
    #[serde(default)]
    pub show_diff: bool,
}

fn default_log_limit() -> usize {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLogResult {
    pub commits: Vec<GitCommit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommit {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    pub diff: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitArgs {
    /// Commit message.
    pub message: String,
    /// Files to stage before commit (relative to workspace). Empty = all tracked changes.
    #[serde(default)]
    pub files: Vec<String>,
    /// Allow empty commit (no changes staged).
    #[serde(default)]
    pub allow_empty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitResult {
    pub hash: String,
    pub short_hash: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitBranchArgs {
    /// Name of the new branch. If empty, lists all branches.
    #[serde(default)]
    pub name: Option<String>,
    /// Start point (commit-ish). Defaults to HEAD.
    #[serde(default)]
    pub start_point: Option<String>,
    /// Delete the branch instead of creating it.
    #[serde(default)]
    pub delete: bool,
    /// Force delete (required if branch has unmerged changes).
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitBranchResult {
    pub branches: Vec<String>,
    pub current: String,
    pub action: String, // "listed" | "created" | "deleted"
}

#[derive(Clone)]
pub struct GitTools {
    root: PathBuf,
}

impl GitTools {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolves a workspace-relative path; refuses anything that escapes it.
    fn resolve_path(&self, path: Option<&str>) -> Result<PathBuf, String> {
        match path {
            None => Ok(self.root.clone()),
            Some(raw) => {
                let raw = raw.trim();
                if raw.is_empty() || raw == "." {
                    return Ok(self.root.clone());
                }
                let p = Path::new(raw);
                let joined = if p.is_absolute() {
                    PathBuf::from(raw)
                } else {
                    self.root.join(p)
                };
                let normalized = crate::tools::file_tools::normalize(&joined);
                if !normalized.starts_with(&self.root) {
                    return Err("refused: path escapes the workspace".into());
                }
                Ok(normalized)
            }
        }
    }

    /// Runs `git <args...>` in the workspace with timeout and output bounds.
    fn run_git(
        &self,
        args: &[&str],
        timeout_ms: u64,
    ) -> Result<(String, String, i32, bool), String> {
        let timeout = Duration::from_millis(timeout_ms.clamp(1_000, MAX_TIMEOUT_MS));
        let mut cmd = Command::new("git");
        cmd.args(args)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::tools::exec_tools::run_with_timeout(&mut cmd, "git", timeout, &args.join(" "))
    }

    pub fn status(&self, args: GitStatusArgs) -> Result<GitStatusResult, String> {
        let path = self.resolve_path(args.path.as_deref())?;
        let rel = path.strip_prefix(&self.root).unwrap_or(&path);
        let rel_str = rel.to_string_lossy().to_string();
        let path_arg = if rel_str.is_empty() { "." } else { &rel_str };

        // Get branch + upstream tracking
        let (out, _, code, _) = self.run_git(&["rev-parse", "--abbrev-ref", "HEAD"], 5_000)?;
        let branch = if code == 0 {
            out.trim().to_string()
        } else {
            "HEAD (detached)".to_string()
        };

        // Upstream ahead/behind
        let mut ahead = 0;
        let mut behind = 0;
        if code == 0 {
            let (out, _, code, _) = self.run_git(
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("{}...@{{u}}", branch),
                ],
                5_000,
            )?;
            if code == 0 {
                for part in out.trim().split('\t') {
                    if let Ok(n) = part.parse::<usize>() {
                        if ahead == 0 && behind == 0 {
                            ahead = n;
                        } else {
                            behind = n;
                        }
                    }
                }
            }
        }

        // Status --porcelain=v2 for parseable output
        let (out, _, code, _) = self.run_git(
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "--untracked-files=all",
                path_arg,
            ],
            10_000,
        )?;

        let mut staged = Vec::new();
        let mut unstaged = Vec::new();
        let mut untracked = Vec::new();

        if code == 0 {
            for line in out.lines() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                // v2 format: "1 XY PATH" or "? PATH"
                if line.starts_with('?') {
                    // untracked
                    let p = line[2..].trim();
                    if !p.is_empty() {
                        untracked.push(p.to_string());
                    }
                } else if line.len() >= 4 {
                    let xy = &line[1..3];
                    let p = line[4..].trim();
                    if !xy.starts_with(' ') && !xy.starts_with('?') {
                        staged.push(p.to_string());
                    }
                    if !xy[1..].starts_with(' ') {
                        unstaged.push(p.to_string());
                    }
                }
            }
        }

        Ok(GitStatusResult {
            path: rel_str,
            branch,
            ahead,
            behind,
            staged,
            unstaged,
            untracked,
        })
    }

    pub fn diff(&self, args: GitDiffArgs) -> Result<GitDiffResult, String> {
        let path = self.resolve_path(args.path.as_deref())?;
        let rel = path.strip_prefix(&self.root).unwrap_or(&path);
        let rel_str = rel.to_string_lossy().to_string();
        let path_arg = if rel_str.is_empty() { "." } else { &rel_str };

        let mut git_args = vec!["diff"];
        if args.staged {
            git_args.push("--cached");
        }
        if args.include_untracked {
            git_args.push("--no-ext-diff");
        }
        git_args.push(path_arg);

        let (out, _, _code, _timed_out) = self.run_git(&git_args, 30_000)?;

        let (diff, truncated) = if out.chars().count() > crate::tools::exec_tools::MAX_OUTPUT {
            let capped: String = out
                .chars()
                .take(crate::tools::exec_tools::MAX_OUTPUT)
                .collect();
            (
                format!(
                    "{capped}\n… +{} more chars",
                    out.len() - crate::tools::exec_tools::MAX_OUTPUT
                ),
                true,
            )
        } else {
            (out, false)
        };

        Ok(GitDiffResult {
            path: rel_str,
            diff,
            truncated,
        })
    }

    pub fn log(&self, args: GitLogArgs) -> Result<GitLogResult, String> {
        let limit = args.limit.clamp(1, 200);
        let path_arg = args.path.as_deref().unwrap_or(".");

        let path = self.resolve_path(Some(path_arg))?;
        let rel = path.strip_prefix(&self.root).unwrap_or(&path);
        let rel_str = rel.to_string_lossy().to_string();
        let rel_arg = if rel_str.is_empty() { "." } else { &rel_str };

        let format = "%H|%h|%an|%ad|%s";
        let format_arg = format!("--format={}", format);
        let limit_arg = format!("-n{}", limit);
        let git_args = vec!["log", "--date=short", &format_arg, &limit_arg, rel_arg];
        let (out, _, code, _) = self.run_git(&git_args, 10_000)?;

        let mut commits = Vec::new();
        if code == 0 {
            for line in out.lines() {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() == 5 {
                    let (hash, short_hash, author, date, message) =
                        (parts[0], parts[1], parts[2], parts[3], parts[4]);
                    let diff = if args.show_diff {
                        let (dout, _, dcode, _) =
                            self.run_git(&["show", "--stat", hash, "--", &rel_str], 15_000)?;
                        if dcode == 0 && !dout.trim().is_empty() {
                            Some(dout)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    commits.push(GitCommit {
                        hash: hash.to_string(),
                        short_hash: short_hash.to_string(),
                        author: author.to_string(),
                        date: date.to_string(),
                        message: message.to_string(),
                        diff,
                    });
                }
            }
        }

        Ok(GitLogResult { commits })
    }

    pub fn commit(&self, args: GitCommitArgs) -> Result<GitCommitResult, String> {
        if args.message.trim().is_empty() {
            return Err("refused: empty commit message".into());
        }

        // Stage files if provided; otherwise stage all tracked changes
        if !args.files.is_empty() {
            for file in &args.files {
                let p = self.resolve_path(Some(file))?;
                let rel = p.strip_prefix(&self.root).unwrap_or(&p);
                let (_, _, code, _) = self.run_git(&["add", &rel.to_string_lossy()], 10_000)?;
                if code != 0 {
                    return Err(format!("git add failed for {}", file));
                }
            }
        } else {
            let (_, _, code, _) = self.run_git(&["add", "-A"], 10_000)?;
            if code != 0 {
                return Err("git add -A failed".into());
            }
        }

        // Check if there's anything to commit (unless allow_empty)
        if !args.allow_empty {
            let (_out, _, code, _) = self.run_git(&["diff", "--cached", "--quiet"], 5_000)?;
            if code == 0 {
                return Err("refused: nothing to commit (use allow_empty=true to override)".into());
            }
        }

        // Commit
        let (_, _, code, _) = self.run_git(&["commit", "-m", &args.message], 30_000)?;
        if code != 0 {
            return Err("git commit failed".into());
        }

        // Get the new commit hash
        let (out, _, code, _) = self.run_git(&["rev-parse", "HEAD"], 5_000)?;
        let hash = if code == 0 {
            out.trim().to_string()
        } else {
            String::new()
        };
        let short_hash = if hash.len() >= 7 {
            hash[..7].to_string()
        } else {
            hash.clone()
        };

        Ok(GitCommitResult {
            hash,
            short_hash,
            message: args.message,
        })
    }

    pub fn branch(&self, args: GitBranchArgs) -> Result<GitBranchResult, String> {
        // Get current branch
        let (out, _, code, _) = self.run_git(&["rev-parse", "--abbrev-ref", "HEAD"], 5_000)?;
        let current = if code == 0 {
            out.trim().to_string()
        } else {
            "HEAD".to_string()
        };

        // List all branches
        let (out, _, code, _) = self.run_git(&["branch", "--format=%(refname:short)"], 5_000)?;
        let mut branches = Vec::new();
        if code == 0 {
            for line in out.lines() {
                let t = line.trim();
                if !t.is_empty() {
                    branches.push(t.to_string());
                }
            }
        }

        if let Some(name) = &args.name {
            if args.delete {
                let flag = if args.force { "-D" } else { "-d" };
                let (_, _, code, _) = self.run_git(&["branch", flag, name], 10_000)?;
                if code != 0 {
                    return Err(format!("git branch {} {} failed", flag, name));
                }
                Ok(GitBranchResult {
                    branches,
                    current,
                    action: "deleted".to_string(),
                })
            } else {
                let start = args.start_point.as_deref().unwrap_or("HEAD");
                let (_, _, code, _) = self.run_git(&["branch", name, start], 10_000)?;
                if code != 0 {
                    return Err(format!("git branch {} {} failed", name, start));
                }
                branches.push(name.clone());
                Ok(GitBranchResult {
                    branches,
                    current,
                    action: "created".to_string(),
                })
            }
        } else {
            Ok(GitBranchResult {
                branches,
                current,
                action: "listed".to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_repo() -> (PathBuf, GitTools) {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("nanomite-git-test-{}-{}", std::process::id(), id));
        fs::create_dir_all(&dir).unwrap();
        let git = GitTools::new(&dir);
        git.run_git(&["init"], 10_000).unwrap();
        git.run_git(&["config", "user.email", "test@test"], 5_000)
            .unwrap();
        git.run_git(&["config", "user.name", "Test"], 5_000)
            .unwrap();
        (dir, git)
    }

    #[test]
    fn status_on_empty_repo() {
        let (dir, git) = init_repo();
        let r = git.status(GitStatusArgs { path: None }).unwrap();
        // New repo with no commits: branch may be "master", "main", or "HEAD (detached)"
        assert!(r.branch == "master" || r.branch == "main" || r.branch == "HEAD (detached)");
        assert!(r.staged.is_empty());
        assert!(r.unstaged.is_empty());
        assert!(r.untracked.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_shows_untracked_and_staged() {
        let (dir, git) = init_repo();
        fs::write(dir.join("new.txt"), "hello").unwrap();
        let r = git.status(GitStatusArgs { path: None }).unwrap();
        assert!(r.untracked.iter().any(|p| p == "new.txt"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_shows_working_tree_changes() {
        let (dir, git) = init_repo();
        fs::write(dir.join("a.txt"), "v1").unwrap();
        git.run_git(&["add", "a.txt"], 5_000).unwrap();
        git.run_git(&["commit", "-m", "initial"], 10_000).unwrap();
        fs::write(dir.join("a.txt"), "v2").unwrap();
        let r = git
            .diff(GitDiffArgs {
                path: None,
                staged: false,
                include_untracked: false,
            })
            .unwrap();
        assert!(r.diff.contains("v2"));
        // git diff shows old content with - prefix
        assert!(r.diff.contains("-v1"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_staged_shows_index_changes() {
        let (dir, git) = init_repo();
        fs::write(dir.join("b.txt"), "orig").unwrap();
        git.run_git(&["add", "b.txt"], 5_000).unwrap();
        git.run_git(&["commit", "-m", "initial"], 10_000).unwrap();
        fs::write(dir.join("b.txt"), "modified").unwrap();
        git.run_git(&["add", "b.txt"], 5_000).unwrap();
        let r = git
            .diff(GitDiffArgs {
                path: None,
                staged: true,
                include_untracked: false,
            })
            .unwrap();
        assert!(r.diff.contains("modified"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_returns_recent_commits() {
        let (dir, git) = init_repo();
        fs::write(dir.join("c.txt"), "1").unwrap();
        git.run_git(&["add", "c.txt"], 5_000).unwrap();
        git.run_git(&["commit", "-m", "first"], 10_000).unwrap();
        fs::write(dir.join("c.txt"), "2").unwrap();
        git.run_git(&["add", "c.txt"], 5_000).unwrap();
        git.run_git(&["commit", "-m", "second"], 10_000).unwrap();
        let r = git
            .log(GitLogArgs {
                limit: 5,
                path: None,
                show_diff: false,
            })
            .unwrap();
        assert_eq!(r.commits.len(), 2);
        assert_eq!(r.commits[0].message, "second");
        assert_eq!(r.commits[1].message, "first");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn commit_creates_a_new_commit() {
        let (dir, git) = init_repo();
        fs::write(dir.join("d.txt"), "content").unwrap();
        let r = git
            .commit(GitCommitArgs {
                message: "add d".into(),
                files: vec!["d.txt".into()],
                allow_empty: false,
            })
            .unwrap();
        assert!(!r.hash.is_empty());
        assert_eq!(r.short_hash.len(), 7);
        assert_eq!(r.message, "add d");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn commit_refuses_empty_message() {
        let (dir, git) = init_repo();
        let err = git
            .commit(GitCommitArgs {
                message: "  ".into(),
                files: vec![],
                allow_empty: false,
            })
            .unwrap_err();
        assert!(err.contains("empty commit message"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn branch_list_shows_current() {
        let (dir, git) = init_repo();
        let r = git
            .branch(GitBranchArgs {
                name: None,
                start_point: None,
                delete: false,
                force: false,
            })
            .unwrap();
        // New repo with no commits: branch may be "master", "main", or empty
        assert!(
            r.branches.contains(&"master".to_string())
                || r.branches.contains(&"main".to_string())
                || r.branches.is_empty()
        );
        assert!(r.current == "master" || r.current == "main" || r.current == "HEAD");
        assert_eq!(r.action, "listed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn branch_create_and_delete() {
        let (dir, git) = init_repo();
        // Need at least one commit to create branches
        fs::write(dir.join("initial.txt"), "init").unwrap();
        git.run_git(&["add", "initial.txt"], 5_000).unwrap();
        git.run_git(&["commit", "-m", "initial"], 10_000).unwrap();
        let r = git
            .branch(GitBranchArgs {
                name: Some("feature".into()),
                start_point: None,
                delete: false,
                force: false,
            })
            .unwrap();
        assert_eq!(r.action, "created");
        assert!(r.branches.contains(&"feature".to_string()));
        let r = git
            .branch(GitBranchArgs {
                name: Some("feature".into()),
                start_point: None,
                delete: true,
                force: false,
            })
            .unwrap();
        assert_eq!(r.action, "deleted");
        // Get fresh branch list after deletion
        let r = git
            .branch(GitBranchArgs {
                name: None,
                start_point: None,
                delete: false,
                force: false,
            })
            .unwrap();
        assert!(!r.branches.contains(&"feature".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_escape_is_refused() {
        let (dir, git) = init_repo();
        let err = git
            .status(GitStatusArgs {
                path: Some("../..".into()),
            })
            .unwrap_err();
        assert!(err.contains("escapes the workspace"));
        let _ = fs::remove_dir_all(&dir);
    }
}
