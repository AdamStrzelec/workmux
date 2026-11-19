//! Persistent registry of project roots the dashboard has seen.
//!
//! The project picker is otherwise built from live agents only, so a project
//! vanishes from it as soon as its last agent pane closes. The registry keeps
//! every local repo root the dashboard has resolved, so the picker can still
//! offer projects that have no running agent.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::util::write_atomic;
use crate::xdg;

const FILE_NAME: &str = "projects.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    projects: Vec<PathBuf>,
}

/// Handle to the on-disk list of known project roots.
pub struct ProjectRegistry {
    path: PathBuf,
}

impl ProjectRegistry {
    /// Registry file in the XDG state directory.
    pub fn open() -> Result<Self> {
        Ok(Self::at(xdg::state_dir()?.join(FILE_NAME)))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// Remembered roots that still exist as directories.
    ///
    /// A missing or unreadable file yields an empty list: the registry is a
    /// convenience and must never break the picker.
    pub fn load(&self) -> Vec<PathBuf> {
        self.read_all()
            .into_iter()
            .filter(|root| root.is_dir())
            .collect()
    }

    /// Add roots not yet remembered. Writes only when something is new.
    ///
    /// Entries whose directory is gone are kept: the path may be on a volume
    /// that is only unmounted for now. `load` hides them in the meantime.
    pub fn record<'a>(&self, roots: impl IntoIterator<Item = &'a PathBuf>) -> Result<()> {
        let mut known = self.read_all();
        let before = known.len();
        for root in roots {
            if !known.contains(root) {
                known.push(root.clone());
            }
        }
        if known.len() == before {
            return Ok(());
        }

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let json = serde_json::to_vec_pretty(&RegistryFile { projects: known })
            .context("Failed to serialize project registry")?;
        write_atomic(&self.path, &json)
    }

    fn read_all(&self) -> Vec<PathBuf> {
        read_file(&self.path)
            .map(|f| f.projects)
            .unwrap_or_default()
    }
}

fn read_file(path: &Path) -> Option<RegistryFile> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(dir: &tempfile::TempDir) -> ProjectRegistry {
        ProjectRegistry::at(dir.path().join("state").join(FILE_NAME))
    }

    fn make_dir(parent: &Path, name: &str) -> PathBuf {
        let path = parent.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn load_without_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(registry(&dir).load().is_empty());
    }

    #[test]
    fn recorded_roots_survive_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let a = make_dir(dir.path(), "a");
        let b = make_dir(dir.path(), "b");

        registry(&dir).record([&a, &b]).unwrap();

        assert_eq!(registry(&dir).load(), vec![a, b]);
    }

    #[test]
    fn record_does_not_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let a = make_dir(dir.path(), "a");
        let reg = registry(&dir);

        reg.record([&a]).unwrap();
        reg.record([&a]).unwrap();

        assert_eq!(reg.load(), vec![a]);
    }

    #[test]
    fn record_skips_write_when_nothing_is_new() {
        let dir = tempfile::tempdir().unwrap();
        let a = make_dir(dir.path(), "a");
        let reg = registry(&dir);
        reg.record([&a]).unwrap();
        let modified = std::fs::metadata(&reg.path).unwrap().modified().unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));
        reg.record([&a]).unwrap();

        assert_eq!(
            std::fs::metadata(&reg.path).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn load_hides_deleted_directories_but_record_keeps_them() {
        let dir = tempfile::tempdir().unwrap();
        let gone = make_dir(dir.path(), "gone");
        let kept = make_dir(dir.path(), "kept");
        let reg = registry(&dir);
        reg.record([&gone, &kept]).unwrap();

        std::fs::remove_dir_all(&gone).unwrap();

        assert_eq!(reg.load(), vec![kept.clone()]);

        // Still remembered: restoring the directory brings it back.
        std::fs::create_dir_all(&gone).unwrap();
        assert_eq!(reg.load(), vec![gone, kept]);
    }

    #[test]
    fn corrupt_file_is_treated_as_empty_and_repaired_on_record() {
        let dir = tempfile::tempdir().unwrap();
        let a = make_dir(dir.path(), "a");
        let reg = registry(&dir);
        std::fs::create_dir_all(reg.path.parent().unwrap()).unwrap();
        std::fs::write(&reg.path, b"not json").unwrap();

        assert!(reg.load().is_empty());

        reg.record([&a]).unwrap();
        assert_eq!(reg.load(), vec![a]);
    }
}
