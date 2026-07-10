//! Best-effort git workspace snapshot for capsules.
//!
//! The daemon attaches this at checkpoint time so a capsule records WHERE the
//! work stood (branch / HEAD / dirty count) independently of what the agent
//! wrote into the summary. Everything is optional: no git, no repo, or a git
//! error simply yields `None` — a capsule must never fail because of git.

use std::path::Path;

/// The git state of a workspace at capsule-creation time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSnapshot {
    /// Current branch name (`HEAD` when detached).
    pub branch: Option<String>,
    /// Full HEAD commit SHA.
    pub head_sha: Option<String>,
    /// Number of dirty entries in `git status --porcelain`.
    pub dirty_files: Option<u32>,
}

/// Collect the git snapshot for `cwd`, or `None` when `cwd` is not inside a
/// git work tree (or git is unavailable).
pub fn collect(cwd: &Path) -> Option<GitSnapshot> {
    // Cheap membership probe first so non-repo projects cost one process.
    let inside = run_git(cwd, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.trim() != "true" {
        return None;
    }
    let branch = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let head_sha = run_git(cwd, &["rev-parse", "HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let dirty_files = run_git(cwd, &["status", "--porcelain"])
        .map(|s| s.lines().filter(|line| !line.trim().is_empty()).count() as u32);
    Some(GitSnapshot {
        branch,
        head_sha,
        dirty_files,
    })
}

/// Current HEAD SHA of `cwd`, for consume-time drift detection.
pub fn head_sha(cwd: &Path) -> Option<String> {
    run_git(cwd, &["rev-parse", "HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Run `git -C <cwd> <args>`; `Some(stdout)` only on exit code 0.
fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let mut command = crate::process::no_window_command("git");
    let output = command.arg("-C").arg(cwd).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn collect_returns_none_outside_a_repo() {
        if !git_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(collect(dir.path()), None);
    }

    #[test]
    fn collect_reads_branch_head_and_dirty_count() {
        if !git_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(git(dir.path(), &["init", "-b", "main"]));
        assert!(git(dir.path(), &["config", "user.email", "t@example.com"]));
        assert!(git(dir.path(), &["config", "user.name", "t"]));
        std::fs::write(dir.path().join("a.txt"), b"one").unwrap();
        assert!(git(dir.path(), &["add", "."]));
        assert!(git(dir.path(), &["commit", "--no-gpg-sign", "-m", "init"]));
        // One dirty file on top of the commit.
        std::fs::write(dir.path().join("b.txt"), b"two").unwrap();

        let snap = collect(dir.path()).expect("snapshot in a repo");
        assert_eq!(snap.branch.as_deref(), Some("main"));
        let sha = snap.head_sha.expect("head sha");
        assert_eq!(sha.len(), 40, "full sha, got {sha}");
        assert_eq!(snap.dirty_files, Some(1));
        assert_eq!(head_sha(dir.path()).as_deref(), Some(sha.as_str()));
    }
}
