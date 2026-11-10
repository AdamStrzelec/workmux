//! Protected paths: detect changes to files agents must not touch (e.g.
//! `.env`). Detection only; nothing is blocked or reverted here.

use anyhow::{Context, Result, bail};
use glob::{MatchOptions, Pattern};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `guard` section of the global config.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardConfig {
    /// Glob patterns. A pattern without `/` matches the file name at any
    /// depth (`.env*`); one with `/` matches the repo-relative path
    /// (`secrets/**`).
    #[serde(default)]
    pub protected_paths: Vec<String>,
    /// Shell command run before `workmux merge` (e.g. an LLM review). It gets
    /// the branch diff on stdin and `WORKMUX_BRANCH`/`WORKMUX_TARGET` in the
    /// environment; a non-zero exit blocks the merge.
    #[serde(default)]
    pub review_command: Option<String>,
}

const OPTS: MatchOptions = MatchOptions {
    case_sensitive: true,
    require_literal_separator: false,
    require_literal_leading_dot: false,
};

/// Files from `files` that match any pattern. Invalid patterns are skipped.
pub fn violations(cfg: &GuardConfig, files: &[String]) -> Vec<String> {
    let patterns: Vec<(Pattern, bool)> = cfg
        .protected_paths
        .iter()
        .filter_map(|p| Pattern::new(p).ok().map(|pat| (pat, !p.contains('/'))))
        .collect();
    let mut out: Vec<String> = files
        .iter()
        .filter(|f| {
            let name = f.rsplit('/').next().unwrap_or(f);
            patterns
                .iter()
                .any(|(pat, by_name)| pat.matches_with(if *by_name { name } else { f }, OPTS))
        })
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}

pub fn git_lines(path: &Path, args: &[&str]) -> Result<Vec<String>> {
    let out = crate::git::unattended_git(Some(path))?
        .args(args)
        .output()
        .context("failed to run git")?;
    if !out.status.success() {
        bail!("git {} failed", args.first().copied().unwrap_or(""));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect())
}

/// Paths changed in `path` relative to `base`: commits since the merge base
/// plus uncommitted and untracked files.
pub fn changed_files(path: &Path, base: &str) -> Result<Vec<String>> {
    let range = format!("{base}...HEAD");
    let mut files = git_lines(path, &["diff", "--name-only", &range])?;
    for line in git_lines(path, &["status", "--porcelain"])? {
        if let Some(rest) = line.get(3..) {
            let name = rest.rsplit(" -> ").next().unwrap_or(rest);
            files.push(name.trim_matches('"').to_string());
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Refuse a merge when the worktree `name` changed protected files compared
/// to `target`. No-op when no patterns are configured.
pub fn ensure_merge_allowed(cfg: &GuardConfig, name: &str, target: &str) -> Result<()> {
    if cfg.protected_paths.is_empty() {
        return Ok(());
    }
    let (path, _) = crate::git::find_worktree(name)?;
    let base = crate::git::get_merge_base_in(Some(&path), target)?;
    let hits = violations(cfg, &changed_files(&path, &base)?);
    if hits.is_empty() {
        return Ok(());
    }
    crate::audit::record_action("merge blocked (protected files)".into(), false);
    bail!(
        "merge blocked: protected files changed:\n  {}\nReview them, or rerun with --allow-protected.",
        hits.join("\n  ")
    )
}

/// Run `guard.review_command` for the worktree `name` before merging into
/// `target`. No-op when unset. A non-zero exit blocks the merge.
pub fn ensure_review_passed(cfg: &GuardConfig, name: &str, target: &str) -> Result<()> {
    let Some(cmd) = cfg
        .review_command
        .as_deref()
        .filter(|c| !c.trim().is_empty())
    else {
        return Ok(());
    };
    let (path, _) = crate::git::find_worktree(name)?;
    let base = crate::git::get_merge_base_in(Some(&path), target)?;
    let range = format!("{base}...HEAD");
    let diff = crate::git::unattended_git(Some(&path))?
        .args(["diff", &range])
        .output()
        .context("failed to run git diff")?
        .stdout;
    let ok = run_review(cmd, &path, name, target, &diff)?;
    if ok {
        return Ok(());
    }
    crate::audit::record_action("merge blocked (review failed)".into(), false);
    bail!("merge blocked: review command failed. Fix the findings, or rerun with --skip-review.")
}

fn run_review(cmd: &str, dir: &Path, branch: &str, target: &str, diff: &[u8]) -> Result<bool> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("sh")
        .args(["-c", cmd])
        .current_dir(dir)
        .env("WORKMUX_BRANCH", branch)
        .env("WORKMUX_TARGET", target)
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to start review command")?;
    if let Some(mut stdin) = child.stdin.take() {
        // The command may exit without reading the diff; ignore broken pipe.
        let _ = stdin.write_all(diff);
    }
    Ok(child.wait()?.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(p: &[&str]) -> GuardConfig {
        GuardConfig {
            protected_paths: p.iter().map(|s| s.to_string()).collect(),
            review_command: None,
        }
    }

    #[test]
    fn name_pattern_matches_at_any_depth() {
        let v = violations(
            &cfg(&[".env*"]),
            &["a/.env.local".into(), "src/main.rs".into()],
        );
        assert_eq!(v, vec!["a/.env.local"]);
    }

    #[test]
    fn path_pattern_matches_relative_path() {
        let v = violations(
            &cfg(&["secrets/**"]),
            &["secrets/k.pem".into(), "x/secrets/k".into()],
        );
        assert_eq!(v, vec!["secrets/k.pem"]);
    }

    #[test]
    fn empty_config_and_bad_pattern_match_nothing() {
        assert!(violations(&cfg(&[]), &[".env".into()]).is_empty());
        assert!(violations(&cfg(&["[bad"]), &[".env".into()]).is_empty());
    }

    #[test]
    fn review_command_exit_status_decides() {
        let d = std::env::temp_dir();
        assert!(run_review("exit 0", &d, "b", "main", b"x").unwrap());
        assert!(!run_review("exit 3", &d, "b", "main", b"x").unwrap());
        assert!(run_review("grep -q hello", &d, "b", "main", b"hello").unwrap());
        assert!(run_review("test \"$WORKMUX_BRANCH\" = feat", &d, "feat", "main", b"").unwrap());
    }
}
