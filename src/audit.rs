//! Local audit log: `audit.log` in the state dir, one JSON object per line.
//! Records which state-changing workmux commands ran, where (project and
//! branch), by whom and whether they succeeded. Argument values are never
//! stored because they can hold prompts and paths.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub ts: u64,
    pub user: String,
    /// Command label, e.g. `merge` or `hunk commit`.
    pub action: String,
    pub project: String,
    pub branch: String,
    pub ok: bool,
}

pub fn path() -> Option<PathBuf> {
    crate::xdg::state_dir().ok().map(|d| d.join("audit.log"))
}

/// Hunk subcommands that only read.
const HUNK_READ_ONLY: &[&str] = &["list", "show"];

/// Label for a state-changing invocation, or `None` for read-only ones.
pub fn action_label(args: &[String]) -> Option<String> {
    let mut positional = args.iter().skip(1).filter(|a| !a.starts_with('-'));
    let cmd = positional.next()?.as_str();
    let cmd = if cmd == "rm" { "remove" } else { cmd };
    match cmd {
        "add" | "merge" | "remove" | "close" | "rename" | "rebase" | "send" | "run"
        | "set-base" | "resurrect" | "sync-files" => Some(cmd.to_string()),
        "hunk" => {
            let sub = positional.next()?.as_str();
            (!HUNK_READ_ONLY.contains(&sub)).then(|| format!("hunk {sub}"))
        }
        _ => None,
    }
}

/// Record a finished command if it changes state. Never fails the caller.
pub fn record_command(args: &[String], ok: bool) {
    let Some(action) = action_label(args) else {
        return;
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let cfg = crate::config::Config::global_team_config();
    let event = Event {
        ts: now(),
        user: crate::team::resolve_user(&cfg),
        action,
        project: crate::team::project_of(&cwd),
        branch: crate::git::get_current_branch_in(&cwd).unwrap_or_default(),
        ok,
    };
    let _ = append(&event);
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn append(event: &Event) -> std::io::Result<()> {
    let Some(path) = path() else { return Ok(()) };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::errlog::trim(&path, MAX_BYTES)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    writeln!(f, "{line}")
}

pub fn read_all() -> Vec<Event> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn labels_state_changing_commands_only() {
        assert_eq!(
            action_label(&a(&["workmux", "merge", "x"])).as_deref(),
            Some("merge")
        );
        assert_eq!(
            action_label(&a(&["workmux", "rm", "x"])).as_deref(),
            Some("remove")
        );
        assert_eq!(
            action_label(&a(&["workmux", "hunk", "commit", "-m", "x"])).as_deref(),
            Some("hunk commit")
        );
        assert_eq!(action_label(&a(&["workmux", "hunk", "list"])), None);
        assert_eq!(action_label(&a(&["workmux", "status"])), None);
        assert_eq!(action_label(&a(&["workmux"])), None);
    }
}
