//! Local error journal: `errors.log` in the state dir, one JSON object per
//! line. Records panics and commands that ended with an error so a bug can be
//! reviewed later. Nothing is sent anywhere; only the user decides what to
//! share.
//!
//! Only the subcommand name is stored, never argument values, because
//! arguments can hold prompts and paths.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

/// Keep the journal bounded: above this size the oldest half is dropped.
const MAX_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub ts: u64,
    /// `panic` or `error`
    pub kind: String,
    pub version: String,
    pub os: String,
    /// Subcommand name only, e.g. `hunk`.
    pub command: String,
    pub message: String,
    pub location: Option<String>,
    pub backtrace: Option<String>,
}

pub fn path() -> Option<PathBuf> {
    crate::xdg::state_dir().ok().map(|d| d.join("errors.log"))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// First non-flag argument after the binary name.
pub fn command_name(args: &[String]) -> String {
    args.iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .cloned()
        .unwrap_or_default()
}

fn one_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
}

pub fn record(
    kind: &str,
    command: &str,
    message: &str,
    location: Option<String>,
    bt: Option<String>,
) {
    let rec = Record {
        ts: now(),
        kind: kind.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        command: command.into(),
        message: one_line(message),
        location,
        backtrace: bt,
    };
    // A failing journal must never mask the real error.
    let _ = append(&rec);
}

fn append(rec: &Record) -> std::io::Result<()> {
    let Some(path) = path() else { return Ok(()) };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    trim(&path)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let line = serde_json::to_string(rec).map_err(std::io::Error::other)?;
    writeln!(f, "{line}")
}

/// Drop the oldest lines once the file grows past `MAX_BYTES`.
fn trim(path: &std::path::Path) -> std::io::Result<()> {
    let Ok(meta) = std::fs::metadata(path) else {
        return Ok(());
    };
    if meta.len() <= MAX_BYTES {
        return Ok(());
    }
    let text = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = text.lines().collect();
    let kept = lines[lines.len() / 2..].join("\n");
    std::fs::write(path, format!("{kept}\n"))
}

pub fn read_all() -> Vec<Record> {
    let Some(path) = path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Record panics, then run the previous hook so the user still sees them.
pub fn install_panic_hook(command: String) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "panic".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()));
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        record("panic", &command, &message, location, Some(bt));
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_name_skips_flags() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(command_name(&a(&["workmux", "hunk", "list"])), "hunk");
        assert_eq!(command_name(&a(&["workmux", "--debug", "add"])), "add");
        assert_eq!(command_name(&a(&["workmux"])), "");
    }

    #[test]
    fn trim_drops_oldest_half() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("errors.log");
        let line = format!("{}\n", "x".repeat(1000));
        std::fs::write(&p, line.repeat(700)).unwrap();
        trim(&p).unwrap();
        let n = std::fs::read_to_string(&p).unwrap().lines().count();
        assert_eq!(n, 350);
    }

    #[test]
    fn record_roundtrips() {
        let r = Record {
            ts: 1,
            kind: "error".into(),
            version: "v".into(),
            os: "o".into(),
            command: "hunk".into(),
            message: "boom".into(),
            location: None,
            backtrace: None,
        };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Record>(&s).unwrap(), r);
    }
}
