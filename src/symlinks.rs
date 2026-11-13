//! Per-user selection of untracked/ignored files to symlink into every new
//! worktree (`workmux files`). Stored outside any repo in the user's config
//! dir (`symlinks.yaml`), so it never touches `.workmux.yaml` or the global
//! `config.yaml` and is not shared with other users of the repository.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::FileConfig;

type Store = BTreeMap<String, Vec<String>>;

fn store_path() -> Result<PathBuf> {
    Ok(crate::xdg::config_dir()?.join("symlinks.yaml"))
}

fn key(root: &Path) -> String {
    root.canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn load_from(path: &Path) -> Store {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_yaml::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_to(path: &Path, store: &Store) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, serde_yaml::to_string(store)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Patterns the user selected for the repo at `root` (empty when none).
pub fn selected(root: &Path) -> Vec<String> {
    let Ok(path) = store_path() else {
        return Vec::new();
    };
    load_from(&path).remove(&key(root)).unwrap_or_default()
}

/// Replace the selection for `root`; an empty list removes the entry.
pub fn set_selected(root: &Path, patterns: Vec<String>) -> Result<()> {
    let path = store_path()?;
    let mut store = load_from(&path);
    if patterns.is_empty() {
        store.remove(&key(root));
    } else {
        store.insert(key(root), patterns);
    }
    save_to(&path, &store).context("Failed to save symlink selection")
}

/// `files` config plus the user's selected symlinks for `root`.
pub fn with_selected(files: &FileConfig, root: &Path) -> FileConfig {
    let mut out = files.clone();
    let extra = selected(root);
    if extra.is_empty() {
        return out;
    }
    let list = out.symlink.get_or_insert_with(Vec::new);
    for p in extra {
        if !list.contains(&p) {
            list.push(p);
        }
    }
    out
}

/// Glob pattern matching exactly the repo-relative `entry`.
pub fn pattern_for(entry: &str) -> String {
    glob::Pattern::escape(entry.trim_end_matches('/'))
}

/// Untracked and git-ignored top-level entries of `root` (directories are
/// collapsed), sorted. `.git` is never offered.
pub fn candidates(root: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for ignored in [false, true] {
        let mut args = vec!["ls-files", "--others", "--exclude-standard", "--directory"];
        if ignored {
            args.push("--ignored");
        }
        out.extend(crate::guard::git_lines(root, &args)?);
    }
    let mut out: Vec<String> = out
        .into_iter()
        .map(|e| e.trim_end_matches('/').to_string())
        .filter(|e| !e.is_empty() && e != ".git" && !e.starts_with(".git/"))
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Apply a selection command to `marked` (one flag per candidate).
/// Accepts numbers and ranges (`1 3 5-7`) that toggle, `all` and `none`.
/// Returns false on input it cannot understand.
pub fn apply_input(input: &str, marked: &mut [bool]) -> bool {
    let n = marked.len();
    let mut next = marked.to_vec();
    for tok in input.split([' ', ',']).filter(|t| !t.is_empty()) {
        match tok {
            "all" => next.iter_mut().for_each(|m| *m = true),
            "none" => next.iter_mut().for_each(|m| *m = false),
            _ => {
                let (a, b) = match tok.split_once('-') {
                    Some((a, b)) => (a.parse::<usize>(), b.parse::<usize>()),
                    None => (tok.parse::<usize>(), tok.parse::<usize>()),
                };
                let (Ok(a), Ok(b)) = (a, b) else {
                    return false;
                };
                if a == 0 || b < a || b > n {
                    return false;
                }
                for i in a..=b {
                    next[i - 1] = !next[i - 1];
                }
            }
        }
    }
    marked.copy_from_slice(&next);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_toggles_ranges_all_none() {
        let mut m = vec![false; 5];
        assert!(apply_input("1 3-4", &mut m));
        assert_eq!(m, [true, false, true, true, false]);
        assert!(apply_input("1", &mut m));
        assert_eq!(m, [false, false, true, true, false]);
        assert!(apply_input("all", &mut m));
        assert!(m.iter().all(|x| *x));
        assert!(apply_input("none, 2", &mut m));
        assert_eq!(m, [false, true, false, false, false]);
    }

    #[test]
    fn bad_input_changes_nothing() {
        let mut m = vec![true, false];
        for bad in ["0", "3", "2-1", "x", "1 9"] {
            assert!(!apply_input(bad, &mut m), "{bad}");
            assert_eq!(m, [true, false]);
        }
    }

    #[test]
    fn pattern_escapes_glob_chars_and_slash() {
        assert_eq!(pattern_for("node_modules/"), "node_modules");
        assert_eq!(pattern_for("a[1].env"), "a[[]1[]].env");
    }

    #[test]
    fn store_roundtrip_and_merge() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.yaml");
        assert!(load_from(&p).is_empty());
        let mut s = Store::new();
        s.insert("/r".into(), vec![".env".into()]);
        save_to(&p, &s).unwrap();
        assert_eq!(load_from(&p), s);
        std::fs::write(&p, ": junk :").unwrap();
        assert!(load_from(&p).is_empty());
    }

    #[test]
    fn candidates_lists_untracked_and_ignored_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        let git = |args: &[&str]| {
            let ok = crate::git::unattended_git(Some(r))
                .unwrap()
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "{args:?}");
        };
        git(&["init", "-q"]);
        std::fs::write(r.join(".gitignore"), "node_modules/\n").unwrap();
        std::fs::create_dir(r.join("node_modules")).unwrap();
        std::fs::write(r.join("node_modules/x"), "").unwrap();
        std::fs::write(r.join(".env"), "").unwrap();
        let c = candidates(r).unwrap();
        assert!(c.contains(&".env".to_string()), "{c:?}");
        assert!(c.contains(&"node_modules".to_string()), "{c:?}");
        assert!(!c.iter().any(|e| e.starts_with(".git/") || e == ".git"));
    }
}
