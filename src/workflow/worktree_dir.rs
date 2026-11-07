//! Effective worktree directory of a repository and planning of moving
//! existing worktrees to a new directory.
//!
//! Resolution order (single source of truth for `workmux add` and the
//! dashboard): project `worktree_dir`, global `worktree_dir`, the directory
//! most existing worktrees live in (detected), the `<project>__worktrees`
//! sibling default.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

/// Where the effective worktree directory comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeDirSource {
    /// `worktree_dir` in the project config.
    Project,
    /// `worktree_dir` in the global config.
    Global,
    /// Not configured; inferred from where existing worktrees live.
    Detected,
    /// Not configured; `<project>__worktrees` beside the main worktree.
    Default,
}

impl WorktreeDirSource {
    pub fn label(self) -> &'static str {
        match self {
            WorktreeDirSource::Project => "configured in project",
            WorktreeDirSource::Global => "configured globally",
            WorktreeDirSource::Detected => "detected from existing worktrees",
            WorktreeDirSource::Default => "workmux default",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveWorktreeDir {
    pub dir: PathBuf,
    pub source: WorktreeDirSource,
}

/// Default worktree dir: `<project>__worktrees` beside the main worktree.
pub fn default_worktree_dir(main: &Path) -> PathBuf {
    let name = main
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    main.parent()
        .unwrap_or(main)
        .join(format!("{name}__worktrees"))
}

/// Parent directory shared by a strict majority of the non-main worktrees,
/// if it differs from the default.
pub fn detect_worktree_dir(
    main: &Path,
    worktrees: &[PathBuf],
    default_dir: &Path,
) -> Option<PathBuf> {
    let parents: Vec<&Path> = worktrees
        .iter()
        .filter(|p| p.as_path() != main)
        .filter_map(|p| p.parent())
        .collect();
    let mut best: Option<(&Path, usize)> = None;
    for parent in &parents {
        let count = parents.iter().filter(|p| *p == parent).count();
        if best.is_none_or(|(_, c)| count > c) {
            best = Some((parent, count));
        }
    }
    let (dir, count) = best?;
    (count * 2 > parents.len() && dir != default_dir).then(|| dir.to_path_buf())
}

fn non_empty(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|v| !v.is_empty())
}

/// Resolve the effective worktree directory. `worktrees` is only consulted
/// when nothing is configured.
pub fn resolve_worktree_dir(
    project: Option<&str>,
    global: Option<&str>,
    main: &Path,
    worktrees: impl FnOnce() -> Vec<PathBuf>,
    home: Option<&Path>,
) -> Result<EffectiveWorktreeDir> {
    if let Some(t) = non_empty(project) {
        return Ok(EffectiveWorktreeDir {
            dir: crate::util::expand_worktree_dir_with_home(t, main, home)?,
            source: WorktreeDirSource::Project,
        });
    }
    if let Some(t) = non_empty(global) {
        return Ok(EffectiveWorktreeDir {
            dir: crate::util::expand_worktree_dir_with_home(t, main, home)?,
            source: WorktreeDirSource::Global,
        });
    }
    let default_dir = default_worktree_dir(main);
    Ok(
        match detect_worktree_dir(main, &worktrees(), &default_dir) {
            Some(dir) => EffectiveWorktreeDir {
                dir,
                source: WorktreeDirSource::Detected,
            },
            None => EffectiveWorktreeDir {
                dir: default_dir,
                source: WorktreeDirSource::Default,
            },
        },
    )
}

/// Paths of all worktrees of the repository (read-only, empty on error).
pub fn list_worktree_paths(main: &Path) -> Vec<PathBuf> {
    crate::git::list_worktrees_in(Some(main))
        .map(|list| list.into_iter().map(|(p, _)| p).collect())
        .unwrap_or_default()
}

/// Display/config form of a directory: relative to the repo root when
/// inside it, `~/...` under home, absolute otherwise.
pub fn format_worktree_dir(dir: &Path, repo_root: &Path, home: Option<&Path>) -> String {
    if let Ok(rel) = dir.strip_prefix(repo_root)
        && !rel.as_os_str().is_empty()
    {
        return rel.display().to_string();
    }
    if let Some(home) = home
        && let Ok(rel) = dir.strip_prefix(home)
    {
        return if rel.as_os_str().is_empty() {
            "~".to_string()
        } else {
            format!("~/{}", rel.display())
        };
    }
    dir.display().to_string()
}

/// Paths of locked worktrees from `git worktree list --porcelain` output.
pub fn parse_locked_worktrees(porcelain: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut current: Option<PathBuf> = None;
    for line in porcelain.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current = Some(PathBuf::from(path));
        } else if (line == "locked" || line.starts_with("locked "))
            && let Some(p) = current.clone()
        {
            out.push(p);
        }
    }
    out
}

/// One existing worktree as input to move planning.
#[derive(Debug, Clone)]
pub struct MoveCandidate {
    pub path: PathBuf,
    pub locked: bool,
    /// Has an open multiplexer window or a running agent.
    pub busy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveEntry {
    Move {
        from: PathBuf,
        to: PathBuf,
    },
    /// Not under the current directory; left untouched.
    Outside {
        path: PathBuf,
    },
    /// Would be moved but cannot be; aborts the whole operation.
    Blocked {
        path: PathBuf,
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct MovePlan {
    /// Value written to `worktree_dir` after the move.
    pub template: String,
    pub from_dir: PathBuf,
    pub to_dir: PathBuf,
    pub entries: Vec<MoveEntry>,
}

impl MovePlan {
    pub fn blocked(&self) -> bool {
        self.entries
            .iter()
            .any(|e| matches!(e, MoveEntry::Blocked { .. }))
    }

    pub fn moves(&self) -> Vec<(PathBuf, PathBuf)> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                MoveEntry::Move { from, to } => Some((from.clone(), to.clone())),
                _ => None,
            })
            .collect()
    }
}

/// Map every non-main worktree directly under `from_dir` to `to_dir`.
/// `exists` reports whether a target path already exists.
pub fn plan_worktree_move(
    main: &Path,
    template: &str,
    from_dir: &Path,
    to_dir: &Path,
    worktrees: &[MoveCandidate],
    exists: impl Fn(&Path) -> bool,
) -> Result<MovePlan> {
    if from_dir == to_dir {
        bail!("worktrees already live in {}", to_dir.display());
    }
    let mut entries = Vec::new();
    for wt in worktrees.iter().filter(|w| w.path != main) {
        if wt.path.parent() != Some(from_dir) {
            entries.push(MoveEntry::Outside {
                path: wt.path.clone(),
            });
            continue;
        }
        let Some(name) = wt.path.file_name() else {
            continue;
        };
        let to = to_dir.join(name);
        let reason = if wt.locked {
            Some("locked")
        } else if wt.busy {
            Some("open tmux window / agent")
        } else if to_dir.starts_with(&wt.path) {
            Some("target dir is inside this worktree")
        } else if exists(&to) {
            Some("target path already exists")
        } else {
            None
        };
        entries.push(match reason {
            Some(r) => MoveEntry::Blocked {
                path: wt.path.clone(),
                reason: r.to_string(),
            },
            None => MoveEntry::Move {
                from: wt.path.clone(),
                to,
            },
        });
    }
    Ok(MovePlan {
        template: template.trim().to_string(),
        from_dir: from_dir.to_path_buf(),
        to_dir: to_dir.to_path_buf(),
        entries,
    })
}

/// Run `git worktree move` for each planned move (never forced). Stops at
/// the first failure. Returns the number of moved worktrees.
pub fn execute_moves(main: &Path, moves: &[(PathBuf, PathBuf)]) -> Result<usize, (usize, String)> {
    for (i, (from, to)) in moves.iter().enumerate() {
        if let Some(parent) = to.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            return Err((i, format!("create {}: {e}", parent.display())));
        }
        let res = crate::cmd::Cmd::new("git")
            .workdir(main)
            .args(&["worktree", "move"])
            .arg(&from.to_string_lossy())
            .arg(&to.to_string_lossy())
            .run();
        if let Err(e) = res {
            return Err((i, format!("move {}: {e:#}", from.display())));
        }
    }
    Ok(moves.len())
}

/// Locked worktree paths of the repository (empty on error).
pub fn list_locked_worktrees(main: &Path) -> Vec<PathBuf> {
    crate::cmd::Cmd::new("git")
        .workdir(main)
        .args(&["worktree", "list", "--porcelain"])
        .run_and_capture_stdout()
        .map(|s| parse_locked_worktrees(&s))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(v: &[&str]) -> Vec<PathBuf> {
        v.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn detect_majority_parent_outside_default() {
        let main = Path::new("/r/app");
        let default = default_worktree_dir(main);
        let wts = paths(&[
            "/r/app",
            "/r/app/.claude/worktrees/a",
            "/r/app/.claude/worktrees/b",
            "/r/other/c",
        ]);
        assert_eq!(
            detect_worktree_dir(main, &wts, &default),
            Some(PathBuf::from("/r/app/.claude/worktrees"))
        );
    }

    #[test]
    fn detect_none_for_default_tie_or_empty() {
        let main = Path::new("/r/app");
        let default = default_worktree_dir(main);
        let wts = paths(&["/r/app", "/r/app__worktrees/a"]);
        assert_eq!(detect_worktree_dir(main, &wts, &default), None);
        let tie = paths(&["/r/x/a", "/r/y/b"]);
        assert_eq!(detect_worktree_dir(main, &tie, &default), None);
        assert_eq!(
            detect_worktree_dir(main, &paths(&["/r/app"]), &default),
            None
        );
    }

    #[test]
    fn resolve_priority_project_global_detected_default() {
        let main = Path::new("/r/app");
        let wts = || paths(&["/r/app", "/r/app/.claude/worktrees/a"]);
        let r = resolve_worktree_dir(Some("../w"), Some("/g"), main, wts, None).unwrap();
        assert_eq!(r.source, WorktreeDirSource::Project);
        assert_eq!(r.dir, PathBuf::from("/r/w"));
        let r = resolve_worktree_dir(Some("  "), Some("/g/{project}"), main, wts, None).unwrap();
        assert_eq!(r.source, WorktreeDirSource::Global);
        assert_eq!(r.dir, PathBuf::from("/g/app"));
        let r = resolve_worktree_dir(None, None, main, wts, None).unwrap();
        assert_eq!(r.source, WorktreeDirSource::Detected);
        assert_eq!(r.dir, PathBuf::from("/r/app/.claude/worktrees"));
        let r = resolve_worktree_dir(None, None, main, || paths(&["/r/app"]), None).unwrap();
        assert_eq!(r.source, WorktreeDirSource::Default);
        assert_eq!(r.dir, PathBuf::from("/r/app__worktrees"));
    }

    #[test]
    fn format_dir_keeps_leading_dot() {
        let root = Path::new("/home/u/code/app");
        let home = Some(Path::new("/home/u"));
        assert_eq!(
            format_worktree_dir(Path::new("/home/u/code/app/.claude/worktrees"), root, home),
            ".claude/worktrees"
        );
        assert_eq!(
            format_worktree_dir(Path::new("/home/u/wts"), root, home),
            "~/wts"
        );
        assert_eq!(format_worktree_dir(Path::new("/home/u"), root, home), "~");
        assert_eq!(
            format_worktree_dir(Path::new("/opt/w"), root, home),
            "/opt/w"
        );
    }

    #[test]
    fn locked_parsed_from_porcelain() {
        let out = "worktree /r/app\nHEAD abc\nbranch refs/heads/main\n\n\
                   worktree /r/w/a\nHEAD abc\nbranch refs/heads/a\nlocked\n\n\
                   worktree /r/w/b\nHEAD abc\nbranch refs/heads/b\nlocked reason here\n\n\
                   worktree /r/w/c\nHEAD abc\nbranch refs/heads/c\n";
        assert_eq!(parse_locked_worktrees(out), paths(&["/r/w/a", "/r/w/b"]));
    }

    fn cand(p: &str, locked: bool, busy: bool) -> MoveCandidate {
        MoveCandidate {
            path: PathBuf::from(p),
            locked,
            busy,
        }
    }

    #[test]
    fn plan_maps_old_to_new_and_keeps_outside() {
        let main = Path::new("/r/app");
        let wts = vec![
            cand("/r/app", false, false),
            cand("/r/app/.claude/worktrees/a", false, false),
            cand("/r/elsewhere/b", false, true),
        ];
        let plan = plan_worktree_move(
            main,
            " ../app__worktrees ",
            Path::new("/r/app/.claude/worktrees"),
            Path::new("/r/app__worktrees"),
            &wts,
            |_| false,
        )
        .unwrap();
        assert!(!plan.blocked());
        assert_eq!(plan.template, "../app__worktrees");
        assert_eq!(
            plan.entries,
            vec![
                MoveEntry::Move {
                    from: "/r/app/.claude/worktrees/a".into(),
                    to: "/r/app__worktrees/a".into()
                },
                MoveEntry::Outside {
                    path: "/r/elsewhere/b".into()
                },
            ]
        );
    }

    #[test]
    fn plan_blocks_locked_busy_and_existing() {
        let main = Path::new("/r/app");
        let wts = vec![
            cand("/r/w/a", true, false),
            cand("/r/w/b", false, true),
            cand("/r/w/c", false, false),
            cand("/r/w/d", false, false),
        ];
        let plan = plan_worktree_move(main, "/n", Path::new("/r/w"), Path::new("/n"), &wts, |p| {
            p == Path::new("/n/c")
        })
        .unwrap();
        assert!(plan.blocked());
        let reasons: Vec<_> = plan
            .entries
            .iter()
            .filter_map(|e| match e {
                MoveEntry::Blocked { reason, .. } => Some(reason.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            reasons,
            vec![
                "locked",
                "open tmux window / agent",
                "target path already exists"
            ]
        );
        assert_eq!(plan.moves(), vec![("/r/w/d".into(), "/n/d".into())]);
    }

    #[test]
    fn plan_rejects_same_dir() {
        let d = Path::new("/r/w");
        assert!(plan_worktree_move(Path::new("/r/app"), "x", d, d, &[], |_| false).is_err());
    }
}
