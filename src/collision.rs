//! Collision detection: the same file changed in several worktrees, or the
//! same branch worked on by several users. Detection only.

use std::collections::BTreeMap;

use crate::team::Snapshot;

/// Files changed in two or more branches: `file -> branches` (sorted).
pub fn file_overlaps(changes: &[(String, Vec<String>)]) -> BTreeMap<String, Vec<String>> {
    let mut by_file: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (branch, files) in changes {
        for f in files {
            let v = by_file.entry(f.clone()).or_default();
            if !v.contains(branch) {
                v.push(branch.clone());
            }
        }
    }
    by_file.retain(|_, b| b.len() > 1);
    for b in by_file.values_mut() {
        b.sort();
    }
    by_file
}

/// Branches of `project` that fresh snapshots of two or more users have an
/// agent on: `branch -> users` (sorted).
pub fn shared_branches(
    snaps: &[Snapshot],
    project: &str,
    now: u64,
    stale_after: u64,
) -> BTreeMap<String, Vec<String>> {
    let mut by_branch: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in snaps {
        if now.saturating_sub(s.published_at) > stale_after {
            continue;
        }
        for a in s.agents.iter().filter(|a| a.project == project) {
            let v = by_branch.entry(a.branch.clone()).or_default();
            if !v.contains(&s.user) {
                v.push(s.user.clone());
            }
        }
    }
    by_branch.retain(|_, u| u.len() > 1);
    for u in by_branch.values_mut() {
        u.sort();
    }
    by_branch
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::Entry;

    fn s(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn overlap_needs_two_branches() {
        let m = file_overlaps(&[
            (s("a"), vec![s("x.rs"), s("y.rs")]),
            (s("b"), vec![s("x.rs")]),
            (s("a"), vec![s("x.rs")]),
        ]);
        assert_eq!(m.len(), 1);
        assert_eq!(m["x.rs"], vec!["a", "b"]);
    }

    fn snap(user: &str, at: u64, project: &str, branch: &str) -> Snapshot {
        Snapshot {
            user: s(user),
            host: s("h"),
            published_at: at,
            agents: vec![Entry {
                project: s(project),
                worktree: s("w"),
                branch: s(branch),
                status: s("working"),
                session: s("s"),
                window: s("w"),
                title: None,
                activity_ts: None,
            }],
        }
    }

    #[test]
    fn shared_branch_ignores_stale_other_projects_and_single_user() {
        let snaps = [
            snap("ania", 100, "p", "feat"),
            snap("marek", 100, "p", "feat"),
            snap("old", 0, "p", "feat"),
            snap("x", 100, "q", "feat"),
            snap("ania", 100, "p", "solo"),
        ];
        let m = shared_branches(&snaps, "p", 110, 60);
        assert_eq!(m.len(), 1);
        assert_eq!(m["feat"], vec!["ania", "marek"]);
    }
}
