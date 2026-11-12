//! Counts shown in the dashboard header: protected-path hits and collisions
//! for the repo the dashboard runs in. Best effort; any error yields zeros.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::{collision, config::Config, guard, team};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Worktrees with changed protected files.
    pub guard: usize,
    /// Files changed in 2+ worktrees plus branches shared by 2+ users.
    pub collisions: usize,
}

pub fn compute() -> Counts {
    compute_inner().unwrap_or_default()
}

fn compute_inner() -> anyhow::Result<Counts> {
    let listing = crate::git::list_worktrees_with_main_in(None)?;
    let base = crate::git::get_default_branch()?;
    let base = crate::git::get_merge_base_in(Some(&listing.main_path), &base)?;
    let guard_cfg = Config::global_guard_config();

    let mut counts = Counts::default();
    let mut changes: Vec<(String, Vec<String>)> = Vec::new();
    for (path, branch) in &listing.worktrees {
        if *path == listing.main_path {
            continue;
        }
        let Ok(files) = guard::changed_files(path, &base) else {
            continue;
        };
        if !guard::violations(&guard_cfg, &files).is_empty() {
            counts.guard += 1;
        }
        changes.push((branch.clone(), files));
    }
    counts.collisions += collision::file_overlaps(&changes).len();

    let cfg = Config::global_team_config();
    let project = team::project_of(&std::env::current_dir().unwrap_or_default());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(snaps) = team::resolve_dir(&cfg).and_then(|d| team::read_snapshots(&d)) {
        counts.collisions +=
            collision::shared_branches(&snaps, &project, now, cfg.stale_after_secs).len();
    }
    Ok(counts)
}

/// Header text, e.g. `guard 1 · collisions 2`. None when nothing to show.
pub fn label(c: &Counts) -> Option<String> {
    let mut parts = Vec::new();
    if c.guard > 0 {
        parts.push(format!("guard {}", c.guard));
    }
    if c.collisions > 0 {
        parts.push(format!("collisions {}", c.collisions));
    }
    (!parts.is_empty()).then(|| format!("\u{26a0} {}", parts.join(" \u{b7} ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_lists_only_nonzero_parts() {
        assert_eq!(label(&Counts::default()), None);
        assert_eq!(
            label(&Counts {
                guard: 1,
                collisions: 0
            })
            .as_deref(),
            Some("\u{26a0} guard 1")
        );
        assert_eq!(
            label(&Counts {
                guard: 2,
                collisions: 3
            })
            .as_deref(),
            Some("\u{26a0} guard 2 \u{b7} collisions 3")
        );
    }
}
