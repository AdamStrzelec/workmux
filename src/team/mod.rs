//! Shared team view: every workmux user publishes a small snapshot of their
//! agents into a common directory, and `workmux team` reads all of them.
//!
//! Users on one machine (or on machines sharing a mount) get "who works on
//! what" without any daemon. Each user writes only their own file, so the
//! directory needs group write access and nothing else.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `team` section of the global config. Global only: the directory is a
/// machine-level fact, not a project one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamConfig {
    /// Shared directory, e.g. `/var/lib/workmux`. Team features are off
    /// while this is unset.
    #[serde(default)]
    pub dir: Option<String>,
    /// Name shown to others. Defaults to `$USER`.
    #[serde(default)]
    pub name: Option<String>,
    /// Snapshots older than this are shown as stale.
    #[serde(default = "default_stale_secs")]
    pub stale_after_secs: u64,
    /// Publish automatically from the dashboard (once a minute).
    #[serde(default = "default_true")]
    pub auto_publish: bool,
    /// Share pane titles (they often contain prompt summaries). Off by default.
    #[serde(default)]
    pub share_titles: bool,
    /// Project names that are never published.
    #[serde(default)]
    pub exclude_projects: Vec<String>,
    /// Also append audit events to `audit-<user>.log` in the team dir so
    /// others can read them with `workmux audit --team`. Off by default.
    #[serde(default)]
    pub share_audit: bool,
}

fn default_true() -> bool {
    true
}

fn default_stale_secs() -> u64 {
    300
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub project: String,
    pub worktree: String,
    pub branch: String,
    pub status: String,
    pub session: String,
    pub window: String,
    pub title: Option<String>,
    pub activity_ts: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub user: String,
    pub host: String,
    pub published_at: u64,
    pub agents: Vec<Entry>,
}

/// Make a user-supplied name safe to use as a file stem.
pub fn sanitize_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_matches('.').to_string();
    if s.is_empty() { "unknown".into() } else { s }
}

pub fn resolve_dir(cfg: &TeamConfig) -> Result<PathBuf> {
    match cfg.dir.as_deref().filter(|d| !d.is_empty()) {
        Some(d) => Ok(PathBuf::from(d)),
        None => bail!("team.dir is not set in the global config"),
    }
}

pub fn resolve_user(cfg: &TeamConfig) -> String {
    let raw = cfg
        .name
        .clone()
        .or_else(|| std::env::var("USER").ok())
        .or_else(|| std::env::var("LOGNAME").ok())
        .unwrap_or_default();
    sanitize_name(&raw)
}

fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: buffer is valid for 256 bytes; gethostname NUL-terminates or truncates.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len() - 1) };
    if rc != 0 {
        return "unknown".into();
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// Write the snapshot to `<dir>/<user>.json` atomically (temp file + rename),
/// readable and writable by the owner's group.
pub fn write_snapshot(dir: &Path, snap: &Snapshot) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("cannot create team dir {}", dir.display()))?;
    let stem = sanitize_name(&snap.user);
    let path = dir.join(format!("{stem}.json"));
    let tmp = dir.join(format!(".{stem}.json.tmp{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(snap)?)
        .with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o664))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(path)
}

/// Read every snapshot in `dir`. Unreadable or malformed files are skipped.
pub fn read_snapshots(dir: &Path) -> Result<Vec<Snapshot>> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", dir.display())),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || !name.ends_with(".json") {
            continue;
        }
        match std::fs::read(&path)
            .map_err(anyhow::Error::from)
            .and_then(|b| serde_json::from_slice::<Snapshot>(&b).map_err(Into::into))
        {
            Ok(s) => out.push(s),
            Err(e) => tracing::debug!(path = %path.display(), "skipping team snapshot: {e}"),
        }
    }
    out.sort_by(|a, b| a.user.cmp(&b.user));
    Ok(out)
}

/// Short relative age, `45s`, `12m`, `3h`, `2d`.
pub fn age(now: u64, then: u64) -> String {
    let d = now.saturating_sub(then);
    match d {
        0..=59 => format!("{d}s"),
        60..=3599 => format!("{}m", d / 60),
        3600..=86399 => format!("{}h", d / 3600),
        _ => format!("{}d", d / 86400),
    }
}

/// `<project>` from `.../<project>__worktrees/<name>`, else the parent dir name.
pub(crate) fn project_of(path: &std::path::Path) -> String {
    let parent = path.parent().and_then(|p| p.file_name());
    let parent = parent.map(|p| p.to_string_lossy().into_owned());
    match parent {
        Some(p) => p
            .strip_suffix("__worktrees")
            .map(str::to_string)
            .unwrap_or(p),
        None => String::new(),
    }
}

pub fn entry_of(pane: &crate::multiplexer::AgentPane) -> Entry {
    Entry {
        project: project_of(&pane.path),
        worktree: pane
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        branch: crate::git::get_current_branch_in(&pane.path).unwrap_or_default(),
        status: pane.status.map_or("-", |s| s.name()).to_string(),
        session: pane.session.clone(),
        window: pane.window_name.clone(),
        title: pane.pane_title.clone(),
        activity_ts: pane.activity_ts,
    }
}

/// Apply the privacy settings: drop excluded projects, hide titles unless allowed.
pub fn apply_privacy(cfg: &TeamConfig, entries: Vec<Entry>) -> Vec<Entry> {
    entries
        .into_iter()
        .filter(|e| !cfg.exclude_projects.iter().any(|p| p == &e.project))
        .map(|mut e| {
            if !cfg.share_titles {
                e.title = None;
            }
            e
        })
        .collect()
}

pub fn new_snapshot(cfg: &TeamConfig, agents: Vec<Entry>, now: u64) -> Snapshot {
    Snapshot {
        user: resolve_user(cfg),
        host: hostname(),
        published_at: now,
        agents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(branch: &str) -> Entry {
        Entry {
            project: "app".into(),
            worktree: "feat".into(),
            branch: branch.into(),
            status: "working".into(),
            session: "s".into(),
            window: "wm-feat".into(),
            title: None,
            activity_ts: Some(5),
        }
    }

    #[test]
    fn sanitize_blocks_path_tricks() {
        assert_eq!(sanitize_name("../etc/passwd"), "_etc_passwd");
        assert_eq!(sanitize_name("..."), "unknown");
        assert_eq!(sanitize_name(""), "unknown");
        assert_eq!(sanitize_name("ania.k"), "ania.k");
    }

    #[test]
    fn privacy_filters_projects_and_titles() {
        let mut a = entry("x");
        a.title = Some("secret prompt".into());
        let mut b = entry("y");
        b.project = "private".into();
        let cfg = TeamConfig {
            exclude_projects: vec!["private".into()],
            ..Default::default()
        };
        let out = apply_privacy(&cfg, vec![a.clone(), b]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, None);
        let cfg = TeamConfig {
            share_titles: true,
            ..Default::default()
        };
        assert_eq!(
            apply_privacy(&cfg, vec![a]).remove(0).title.as_deref(),
            Some("secret prompt")
        );
    }

    #[test]
    fn age_formats() {
        assert_eq!(age(100, 55), "45s");
        assert_eq!(age(1000, 280), "12m");
        assert_eq!(age(20000, 9000), "3h");
        assert_eq!(age(200000, 0), "2d");
        assert_eq!(age(5, 10), "0s");
    }

    #[test]
    fn write_then_read_roundtrip_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |user: &str| Snapshot {
            user: user.into(),
            host: "h".into(),
            published_at: 1,
            agents: vec![entry("b")],
        };
        write_snapshot(dir.path(), &mk("zed")).unwrap();
        write_snapshot(dir.path(), &mk("ann")).unwrap();
        write_snapshot(dir.path(), &mk("ann")).unwrap();
        std::fs::write(dir.path().join("junk.json"), "{nope").unwrap();
        std::fs::write(dir.path().join("note.txt"), "x").unwrap();
        let got = read_snapshots(dir.path()).unwrap();
        assert_eq!(
            got.iter().map(|s| s.user.as_str()).collect::<Vec<_>>(),
            ["ann", "zed"]
        );
        assert_eq!(got[0].agents[0], entry("b"));
    }

    #[test]
    fn missing_dir_reads_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_snapshots(&dir.path().join("nope")).unwrap().is_empty());
    }

    #[test]
    fn unset_dir_is_error() {
        assert!(resolve_dir(&TeamConfig::default()).is_err());
    }
}
