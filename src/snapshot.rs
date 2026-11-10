//! Worktree snapshots: save the full working tree (tracked, modified and
//! untracked, minus ignored files) as a dangling commit kept alive by a ref
//! under `refs/workmux/snapshots/<branch>/`, and restore it later.
//! Nothing is written to branches, the index of the worktree or the history.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Output;

const REF_ROOT: &str = "refs/workmux/snapshots";

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Snapshot {
    pub id: String,
    pub ts: u64,
    pub label: String,
}

fn git(path: &Path, args: &[&str], envs: &[(&str, String)]) -> Result<Output> {
    let mut cmd = crate::git::unattended_git(Some(path))?;
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().context("failed to run git")
}

fn git_ok(path: &Path, args: &[&str], envs: &[(&str, String)]) -> Result<String> {
    let out = git(path, args, envs)?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Branch name made safe for use as a ref path component.
pub fn ref_component(branch: &str) -> String {
    let s: String = branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches(['.', '-']).to_string();
    if s.is_empty() { "detached".into() } else { s }
}

/// Last path component of a snapshot ref is `<ts>-<label>`.
pub fn parse_ref(refname: &str) -> Option<Snapshot> {
    let leaf = refname.rsplit('/').next()?;
    let (ts, label) = leaf.split_once('-').unwrap_or((leaf, ""));
    Some(Snapshot {
        id: leaf.to_string(),
        ts: ts.parse().ok()?,
        label: label.to_string(),
    })
}

fn branch_prefix(path: &Path) -> Result<String> {
    let branch = git_ok(path, &["rev-parse", "--abbrev-ref", "HEAD"], &[])?;
    Ok(format!("{REF_ROOT}/{}", ref_component(&branch)))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Save the current working tree. Returns the snapshot id.
pub fn save(path: &Path, label: &str) -> Result<Snapshot> {
    let prefix = branch_prefix(path)?;
    let git_dir: PathBuf = {
        let d = git_ok(path, &["rev-parse", "--absolute-git-dir"], &[])?;
        PathBuf::from(d)
    };
    // Temporary index so the real index is never touched.
    let tmp_index = git_dir.join(format!("workmux-snapshot-index-{}", std::process::id()));
    let idx = [("GIT_INDEX_FILE", tmp_index.to_string_lossy().into_owned())];
    let result = (|| -> Result<Snapshot> {
        git_ok(path, &["read-tree", "HEAD"], &idx)?;
        git_ok(path, &["add", "-A"], &idx)?;
        let tree = git_ok(path, &["write-tree"], &idx)?;
        let ident = [
            ("GIT_AUTHOR_NAME", "workmux".to_string()),
            ("GIT_AUTHOR_EMAIL", "workmux@localhost".to_string()),
            ("GIT_COMMITTER_NAME", "workmux".to_string()),
            ("GIT_COMMITTER_EMAIL", "workmux@localhost".to_string()),
        ];
        let msg = if label.is_empty() {
            "workmux snapshot".to_string()
        } else {
            format!("workmux snapshot: {label}")
        };
        let commit = git_ok(
            path,
            &["commit-tree", &tree, "-p", "HEAD", "-m", &msg],
            &ident,
        )?;
        let ts = now();
        let leaf = if label.is_empty() {
            ts.to_string()
        } else {
            format!("{ts}-{}", ref_component(label))
        };
        git_ok(
            path,
            &["update-ref", &format!("{prefix}/{leaf}"), &commit],
            &[],
        )?;
        Ok(Snapshot {
            id: leaf,
            ts,
            label: label.to_string(),
        })
    })();
    let _ = std::fs::remove_file(&tmp_index);
    result
}

/// Snapshots of the current branch, newest first.
pub fn list(path: &Path) -> Result<Vec<Snapshot>> {
    let prefix = branch_prefix(path)?;
    let out = git_ok(
        path,
        &["for-each-ref", "--format=%(refname)", &format!("{prefix}/")],
        &[],
    )?;
    let mut snaps: Vec<Snapshot> = out.lines().filter_map(parse_ref).collect();
    snaps.sort_by(|a, b| b.ts.cmp(&a.ts).then(b.id.cmp(&a.id)));
    Ok(snaps)
}

/// Restore the working tree to snapshot `id`. A `before-restore` snapshot is
/// saved first, so the current state is never lost. Commits are not touched.
pub fn restore(path: &Path, id: &str) -> Result<Snapshot> {
    let snaps = list(path)?;
    let target = snaps.iter().find(|s| s.id == id).with_context(|| {
        format!("no snapshot '{id}' on this branch (see `workmux snapshot list`)")
    })?;
    let prefix = branch_prefix(path)?;
    let before = save(path, "before-restore")?;
    let refname = format!("{prefix}/{}", target.id);
    git_ok(path, &["read-tree", "--reset", "-u", &refname], &[])?;
    // Unstage: keep the restored files in the working tree only.
    git_ok(path, &["reset", "-q"], &[])?;
    Ok(before)
}

/// Snapshots to delete when keeping the newest `keep` (input newest first).
fn to_prune(snaps: &[Snapshot], keep: usize) -> &[Snapshot] {
    snaps.get(keep..).unwrap_or(&[])
}

/// Delete all but the newest `keep` snapshots of the current branch.
/// Returns the removed ids. Only the refs go; objects are left to git gc.
pub fn prune(path: &Path, keep: usize) -> Result<Vec<String>> {
    let snaps = list(path)?;
    let prefix = branch_prefix(path)?;
    let mut removed = Vec::new();
    for s in to_prune(&snaps, keep) {
        git_ok(
            path,
            &["update-ref", "-d", &format!("{prefix}/{}", s.id)],
            &[],
        )?;
        removed.push(s.id.clone());
    }
    Ok(removed)
}

/// Prune to `guard.snapshot_keep` when configured (and above 0). Best effort:
/// failures are ignored so a save is never reported as failed because of it.
pub fn auto_prune(path: &Path) {
    let keep = crate::config::Config::global_guard_config().snapshot_keep;
    if let Some(keep) = keep.filter(|k| *k > 0) {
        let _ = prune(path, keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_component_is_safe() {
        assert_eq!(ref_component("feat/login page"), "feat-login-page");
        assert_eq!(ref_component("..x.."), "x");
        assert_eq!(ref_component("///"), "detached");
    }

    #[test]
    fn parses_ref_leaf() {
        let s = parse_ref("refs/workmux/snapshots/main/1700000000-before-fix").unwrap();
        assert_eq!(s.ts, 1700000000);
        assert_eq!(s.label, "before-fix");
        let s = parse_ref("refs/workmux/snapshots/main/1700000000").unwrap();
        assert_eq!(s.label, "");
        assert!(parse_ref("refs/workmux/snapshots/main/abc").is_none());
    }

    #[test]
    fn prune_keeps_newest() {
        let mk = |ts: u64| Snapshot {
            id: ts.to_string(),
            ts,
            label: String::new(),
        };
        let snaps = vec![mk(3), mk(2), mk(1)];
        let ids: Vec<_> = to_prune(&snaps, 1).iter().map(|s| s.ts).collect();
        assert_eq!(ids, vec![2, 1]);
        assert!(to_prune(&snaps, 3).is_empty());
        assert!(to_prune(&snaps, 10).is_empty());
        assert_eq!(to_prune(&snaps, 0).len(), 3);
    }
}
