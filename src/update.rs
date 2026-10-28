//! `nanomite update`: refresh the source checkout from origin and rebuild
//! the release binary in place.
//!
//! Designed for the git-checkout install layout (binary built from
//! `target/release/`, usually behind a symlink). The running process keeps
//! executing the old inode while cargo replaces the file, so the update
//! takes effect on the next start.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a single git or cargo step may run before the update gives up.
const STEP_TIMEOUT: Duration = Duration::from_secs(600);

/// Entry point for `nanomite update`.
pub fn run() -> io::Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("nanomite v{current} — checking for updates");

    let Some(repo) = locate_checkout() else {
        return Err(io::Error::other(
            "update requires a git checkout install: no .git found above the running binary",
        ));
    };

    let branch = git_out(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    step("fetching origin");
    git_out(&repo, &["fetch", "origin", "--tags", "--quiet"])?;

    let behind: usize = git_out(
        &repo,
        &["rev-list", "--count", &format!("HEAD..origin/{branch}")],
    )?
    .parse()
    .unwrap_or(0);

    let latest_tag = git_opt(
        &repo,
        &[
            "describe",
            "--tags",
            "--abbrev=0",
            &format!("origin/{branch}"),
        ],
    );
    let tag_newer = latest_tag
        .as_deref()
        .and_then(parse_tag)
        .is_some_and(|tag| tag > parse_tag(current).unwrap_or((0, 0, 0)));

    if behind == 0 && !tag_newer {
        println!("nanomite v{current} is up to date");
        return Ok(());
    }

    if behind > 0 {
        let incoming = git_out(
            &repo,
            &[
                "log",
                "--oneline",
                &format!("HEAD..origin/{branch}"),
                &format!("-{MAX_INCOMING_LOG}"),
            ],
        )?;
        println!("updating {branch}: {behind} new commit(s)");
        for line in incoming.lines() {
            println!("  {line}");
        }
    } else {
        println!("new release tag available");
    }

    step("pulling (fast-forward only)");
    git_out(&repo, &["pull", "--ff-only", "--quiet"])?;

    let new_version = read_version(&repo).unwrap_or_else(|| current.to_string());
    step("rebuilding (release) — this can take a minute");
    let build = std::process::Command::new("cargo")
        .current_dir(&repo)
        .args(["build", "--release"])
        .status()
        .map_err(|e| io::Error::other(format!("cargo failed to start: {e}")))?;
    if !build.success() {
        return Err(io::Error::other(
            "cargo build --release failed — the source is updated but the binary is not; fix the build and run `cargo build --release`",
        ));
    }

    println!("updated to v{new_version} — restart nanomite to switch");
    Ok(())
}

/// Cap on the incoming-commit summary so a huge pull does not flood the page.
const MAX_INCOMING_LOG: usize = 10;

/// Locates the source checkout from the running binary: `current_exe`
/// resolves symlinks, then the first ancestor containing `.git` wins.
fn locate_checkout() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    exe.ancestors()
        .skip(1)
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

fn git_out(repo: &Path, args: &[&str]) -> Result<String, io::Error> {
    let out = run_git(repo, args)?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or("git"),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_opt(repo: &Path, args: &[&str]) -> Option<String> {
    let out = run_git(repo, args).ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn run_git(repo: &Path, args: &[&str]) -> io::Result<std::process::Output> {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(repo).args(args);
    timeout_runner(cmd, STEP_TIMEOUT)
}

fn timeout_runner(
    mut cmd: std::process::Command,
    timeout: Duration,
) -> io::Result<std::process::Output> {
    // Unix: detach from our process group so a timeout kill does not take the
    // caller down with it. (Windows: the child stays in the console group;
    // the direct-process kill in the timeout path still applies.)
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| io::Error::other(format!("failed to spawn: {e}")))?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(_) => return child.wait_with_output(),
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::other(format!(
                    "step timed out after {}s",
                    timeout.as_secs()
                )));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn read_version(repo: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(repo.join("Cargo.toml")).ok()?;
    for line in manifest.lines() {
        if let Some(rest) = line.trim().strip_prefix("version") {
            let rest = rest.trim_start();
            let rest = rest.strip_prefix('=')?.trim();
            let value = rest.trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Parses `v0.7.0` / `0.7.0` into a comparable tuple.
fn parse_tag(tag: &str) -> Option<(u64, u64, u64)> {
    let digits: String = tag
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = digits.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

fn step(label: &str) {
    println!("— {label}…");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tag_handles_v_prefix_and_defaults() {
        assert_eq!(parse_tag("v0.7.0"), Some((0, 7, 0)));
        assert_eq!(parse_tag("0.7.0"), Some((0, 7, 0)));
        assert_eq!(parse_tag("v1.2"), Some((1, 2, 0)));
        assert_eq!(parse_tag("v1"), Some((1, 0, 0)));
        assert_eq!(parse_tag("nope"), None);
    }

    #[test]
    fn tag_newer_comparison() {
        let newer = parse_tag("v0.7.0").unwrap() > parse_tag("0.6.0").unwrap();
        assert!(newer);
        let same = parse_tag("v0.6.0").unwrap() > parse_tag("0.6.0").unwrap();
        assert!(!same);
        let older = parse_tag("v0.5.9").unwrap() > parse_tag("0.6.0").unwrap();
        assert!(!older);
    }
}
