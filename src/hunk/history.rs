//! History rewriting: amend and reword earlier commits non-interactively.
//!
//! Both use `fixup!` / `amend!` commits plus an autosquash rebase, so the editor
//! never opens. Hooks and signing are disabled like everywhere else in `hunk`.

use anyhow::{Context, Result};

use super::diff::{git_cmd, run_git};
use super::ops::{is_protected, require_clean_index};

fn git_out(args: &[&str]) -> Result<String> {
    Ok(run_git(git_cmd()?.args(args))?.trim().to_string())
}

fn rebase_in_progress() -> Result<bool> {
    let dir = git_out(&["rev-parse", "--git-dir"])?;
    let dir = std::path::Path::new(&dir);
    Ok(["rebase-merge", "rebase-apply", "CHERRY_PICK_HEAD"]
        .iter()
        .any(|name| dir.join(name).exists()))
}

fn has_staged_changes() -> Result<bool> {
    let status = git_cmd()?
        .args(["diff", "--cached", "--quiet"])
        .status()
        .context("failed to check staged changes")?;
    Ok(!status.success())
}

/// Resolve `commit` and check that rewriting it is safe. Returns its full SHA.
fn resolve_rewrite_target(commit: &str, allow_pushed: bool) -> Result<String> {
    if rebase_in_progress()? {
        anyhow::bail!("a rebase or cherry-pick is in progress; finish or abort it first");
    }
    let branch = git_cmd()?
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .output()
        .context("failed to check current branch")?;
    if !branch.status.success() {
        anyhow::bail!("HEAD is detached; check out a branch before rewriting history");
    }
    let branch = String::from_utf8_lossy(&branch.stdout).trim().to_string();
    if is_protected(&branch) {
        anyhow::bail!(
            "refusing to rewrite history on protected branch '{branch}'; use a feature branch"
        );
    }

    let sha = git_out(&["rev-parse", "--verify", &format!("{commit}^{{commit}}")])
        .with_context(|| format!("commit '{commit}' not found"))?;
    let is_ancestor = git_cmd()?
        .args(["merge-base", "--is-ancestor", &sha, "HEAD"])
        .status()
        .context("failed to check ancestry")?;
    if !is_ancestor.success() {
        anyhow::bail!("{commit} is not an ancestor of HEAD");
    }
    if !git_out(&["rev-list", "--merges", &format!("{sha}..HEAD")])?.is_empty() {
        anyhow::bail!(
            "there are merge commits between {commit} and HEAD; rebase would flatten them"
        );
    }
    if !allow_pushed && !git_out(&["branch", "-r", "--contains", &sha])?.is_empty() {
        anyhow::bail!(
            "{commit} is already on a remote branch; rewriting it needs a force push \
             (use --allow-pushed to do it anyway)"
        );
    }
    Ok(sha)
}

fn short(sha: &str) -> &str {
    &sha[..7.min(sha.len())]
}

/// Squash the pending `fixup!`/`amend!` commit into `target` via a non-interactive rebase.
/// On failure before the rebase starts, the temporary commit is undone (soft reset).
fn autosquash(target: &str, what: &str) -> Result<()> {
    let has_parent = git_cmd()?
        .args(["rev-parse", "--verify", "--quiet", &format!("{target}^")])
        .output()
        .context("failed to inspect commit parents")?
        .status
        .success();
    let mut cmd = git_cmd()?;
    cmd.args(["rebase", "-i", "--autosquash", "--autostash"]);
    if has_parent {
        cmd.arg(format!("{target}~1"));
    } else {
        cmd.arg("--root");
    }
    cmd.env("GIT_SEQUENCE_EDITOR", "true");
    let output = cmd.output().context("failed to run git rebase")?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if rebase_in_progress()? {
        anyhow::bail!(
            "conflict while {what} {}; resolve it and run 'git rebase --continue', \
             or abort with 'git rebase --abort'\n{}",
            short(target),
            stderr.trim()
        );
    }
    let _ = git_cmd()?.args(["reset", "--soft", "HEAD~1"]).status();
    anyhow::bail!("rebase failed: {}", stderr.trim())
}

fn commits_after(sha: &str) -> Result<usize> {
    git_out(&["rev-list", "--count", &format!("{sha}..HEAD")])?
        .parse()
        .context("unexpected rev-list output")
}

fn describe_at(distance: usize, verb: &str) {
    let spec = if distance == 0 {
        "HEAD".to_string()
    } else {
        format!("HEAD~{distance}")
    };
    if let Ok(info) = git_out(&["log", "-1", "--format=%h %s", &spec]) {
        eprintln!("{verb} {info}");
    }
}

fn commit_checked(args: &[&str], failure: &str) -> Result<()> {
    let output = git_cmd()?
        .args(args)
        .output()
        .with_context(|| failure.to_string())?;
    if !output.status.success() {
        anyhow::bail!(
            "{failure}: {}{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Fold the staged changes into an earlier commit (or HEAD).
pub fn amend(commit: &str, allow_pushed: bool) -> Result<()> {
    if !has_staged_changes()? {
        anyhow::bail!("no staged changes to amend; stage hunks first ('workmux hunk stage')");
    }
    let target = resolve_rewrite_target(commit, allow_pushed)?;
    let distance = commits_after(&target)?;
    if distance == 0 {
        commit_checked(
            &["commit", "--amend", "--no-edit"],
            "git commit --amend failed",
        )?;
    } else {
        commit_checked(
            &["commit", "-m", &format!("fixup! {target}")],
            "failed to create fixup commit",
        )?;
        autosquash(&target, "amending")?;
    }
    describe_at(distance, "amended");
    Ok(())
}

/// Change the message of an earlier commit (or HEAD) without touching its content.
pub fn reword(commit: &str, message: &[String], allow_pushed: bool) -> Result<()> {
    if message.is_empty() {
        anyhow::bail!("no message given; use -m");
    }
    require_clean_index()?;
    let target = resolve_rewrite_target(commit, allow_pushed)?;
    let message = message.join("\n\n");
    let distance = commits_after(&target)?;
    if distance == 0 {
        commit_checked(
            &["commit", "--amend", "--only", "-m", &message],
            "git commit --amend failed",
        )?;
    } else {
        commit_checked(
            &[
                "commit",
                "--allow-empty",
                "-m",
                &format!("amend! {target}\n\n{message}"),
            ],
            "failed to create reword commit",
        )?;
        autosquash(&target, "rewording")?;
    }
    describe_at(distance, "reworded");
    Ok(())
}
