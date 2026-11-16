//! Fast-forward the local default branch (`main`) from `origin` without
//! switching to it (dashboard `U`).
//!
//! Only ever moves the branch forward: a dirty or diverged `main` is reported
//! and left untouched.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use super::{App, AppEvent};

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = crate::git::unattended_git(Some(dir))?.args(args).output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(anyhow!(
            "{}",
            err.lines().last().unwrap_or("git failed").trim()
        ))
    }
}

/// Path of the worktree that has `branch` checked out, if any.
pub fn worktree_of(porcelain: &str, branch: &str) -> Option<PathBuf> {
    let want = format!("branch refs/heads/{branch}");
    porcelain
        .split("\n\n")
        .find(|block| block.lines().any(|l| l == want))
        .and_then(|block| block.lines().find_map(|l| l.strip_prefix("worktree ")))
        .map(PathBuf::from)
}

/// Fetch and fast-forward the default branch of the repo at `repo`.
pub fn update_default_branch(repo: &Path) -> Result<String> {
    let branch = crate::git::get_default_branch_in(Some(repo))?;
    git(repo, &["fetch", "origin", &branch])?;
    let remote = format!("origin/{branch}");
    let before = git(repo, &["rev-parse", &branch])?;
    let porcelain = git(repo, &["worktree", "list", "--porcelain"])?;
    match worktree_of(&porcelain, &branch) {
        Some(wt) => {
            git(&wt, &["merge", "--ff-only", &remote]).map_err(|e| {
                anyhow!("{branch} cannot fast-forward (local changes or diverged): {e}")
            })?;
        }
        None => {
            git(repo, &["fetch", "origin", &format!("{branch}:{branch}")])?;
        }
    }
    let after = git(repo, &["rev-parse", &branch])?;
    if before == after {
        return Ok(format!("{branch} already up to date"));
    }
    let n = git(
        repo,
        &["rev-list", "--count", &format!("{before}..{after}")],
    )
    .unwrap_or_default();
    Ok(format!("{branch} updated (+{n} commits)"))
}

impl App {
    /// Update the default branch of the current project in the background.
    pub fn update_main_branch(&mut self) {
        let root = self
            .worktrees
            .iter()
            .find(|w| w.is_main)
            .map(|w| w.path.clone())
            .or_else(|| std::env::current_dir().ok());
        let Some(root) = root else {
            self.status_message = Some((
                "No project directory".to_string(),
                std::time::Instant::now(),
            ));
            return;
        };
        self.status_message = Some(("Updating main...".to_string(), std::time::Instant::now()));
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let r = update_default_branch(&root).map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::MainUpdated(r));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_worktree_with_branch() {
        let p = "worktree /r\nHEAD abc\nbranch refs/heads/main\n\nworktree /r__wt/x\nHEAD def\nbranch refs/heads/feat\n";
        assert_eq!(worktree_of(p, "main"), Some(PathBuf::from("/r")));
        assert_eq!(worktree_of(p, "feat"), Some(PathBuf::from("/r__wt/x")));
        assert_eq!(worktree_of(p, "nope"), None);
    }

    fn sh(dir: &Path, args: &[&str]) {
        let ok = crate::git::unattended_git(Some(dir))
            .unwrap()
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "{args:?}");
    }

    #[test]
    fn fast_forwards_checked_out_main_and_refuses_diverged() {
        let t = tempfile::tempdir().unwrap();
        let o = t.path().join("origin");
        let c = t.path().join("clone");
        std::fs::create_dir(&o).unwrap();
        sh(&o, &["init", "-q", "-b", "main"]);
        sh(
            &o,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "a",
            ],
        );
        sh(t.path(), &["clone", "-q", o.to_str().unwrap(), "clone"]);
        sh(
            &o,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "b",
            ],
        );
        let msg = update_default_branch(&c).unwrap();
        assert!(msg.contains("+1"), "{msg}");
        assert!(update_default_branch(&c).unwrap().contains("up to date"));
        // diverge: local commit + new remote commit
        sh(
            &c,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "l",
            ],
        );
        sh(
            &o,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "r",
            ],
        );
        assert!(update_default_branch(&c).is_err());
    }
}
