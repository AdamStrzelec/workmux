//! On-disk snapshot cache with a cross-process refresh lock.

use anyhow::Result;
use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use super::{Snapshot, now_unix};

pub const DEFAULT_TTL_SECS: i64 = 55;

pub fn cache_path() -> Result<PathBuf> {
    Ok(crate::xdg::cache_dir()?.join("usage-claude.json"))
}

pub fn load(path: &Path) -> Option<Snapshot> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

pub fn save(path: &Path, snapshot: &Snapshot) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp, serde_json::to_vec(snapshot)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

/// Fresh means observed within `ttl` seconds and not from the future.
pub fn is_fresh(snapshot: &Snapshot, ttl: i64, now: i64) -> bool {
    let age = now - snapshot.observed_at_unix;
    (0..=ttl).contains(&age)
}

/// Exclusive non-blocking lock. Held until the returned file is dropped.
pub fn try_lock(path: &Path) -> Option<fs::File> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()?;
    // SAFETY: fd is valid for the lifetime of `file`.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    (rc == 0).then_some(file)
}

/// Return a fresh snapshot, probing the API only when the cache is stale.
///
/// If another process is already refreshing, the stale cache is returned
/// rather than probing twice. `force` bypasses the freshness check.
pub fn get_or_refresh(force: bool, probe: impl FnOnce() -> Result<Snapshot>) -> Result<Snapshot> {
    let path = cache_path()?;
    let cached = load(&path);
    if !force
        && let Some(snap) = &cached
        && is_fresh(snap, DEFAULT_TTL_SECS, now_unix())
    {
        return Ok(snap.clone());
    }

    let Some(_lock) = try_lock(&path.with_extension("lock")) else {
        return cached.ok_or_else(|| anyhow::anyhow!("another refresh is in progress"));
    };
    match probe() {
        Ok(snap) => {
            let _ = save(&path, &snap);
            Ok(snap)
        }
        // Fall back to stale data when the API is unreachable.
        Err(e) => cached.ok_or(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(observed: i64) -> Snapshot {
        Snapshot {
            session: None,
            weekly: None,
            scoped: vec![],
            observed_at_unix: observed,
        }
    }

    #[test]
    fn freshness_window() {
        assert!(is_fresh(&snap(100), 55, 120));
        assert!(!is_fresh(&snap(100), 55, 200));
        assert!(
            !is_fresh(&snap(300), 55, 200),
            "future timestamps are stale"
        );
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        save(&path, &snap(7)).unwrap();
        assert_eq!(load(&path).unwrap(), snap(7));
    }

    #[test]
    fn load_corrupt_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        fs::write(&path, "garbage").unwrap();
        assert!(load(&path).is_none());
    }

    #[test]
    fn lock_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.lock");
        let first = try_lock(&path).unwrap();
        assert!(try_lock(&path).is_none());
        drop(first);
        assert!(try_lock(&path).is_some());
    }
}
