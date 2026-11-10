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
        "snapshot" => {
            let sub = positional.next()?.as_str();
            matches!(sub, "restore" | "prune").then(|| format!("snapshot {sub}"))
        }
        _ => None,
    }
}

/// Record a finished command if it changes state. Never fails the caller.
pub fn record_command(args: &[String], ok: bool) {
    if let Some(action) = action_label(args) {
        record_action(action, ok);
    }
}

/// Record an explicit event (e.g. `merge blocked`). Never fails the caller.
pub fn record_action(action: String, ok: bool) {
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
    let _ = share(&cfg, &event);
}

/// Mirror the event into the team dir when `team.share_audit` is on.
/// Projects in `team.exclude_projects` are never shared.
fn share(cfg: &crate::team::TeamConfig, event: &Event) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    if !cfg.share_audit || cfg.exclude_projects.contains(&event.project) {
        return Ok(());
    }
    let Ok(dir) = crate::team::resolve_dir(cfg) else {
        return Ok(());
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(shared_file_name(&event.user));
    crate::errlog::trim(&path, MAX_BYTES)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o664)
        .open(&path)?;
    // Group-writable regardless of umask, like the team snapshots.
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664));
    }
    let line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    writeln!(f, "{line}")
}

fn shared_file_name(user: &str) -> String {
    format!("audit-{}.log", crate::team::sanitize_name(user))
}

/// Parse JSON-lines audit text, skipping malformed lines.
pub fn parse_lines(text: &str) -> Vec<Event> {
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// All users' shared events from the team dir, oldest first.
pub fn read_shared(dir: &std::path::Path) -> std::io::Result<Vec<Event>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("audit-") || !name.ends_with(".log") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            out.extend(parse_lines(&text));
        }
    }
    out.sort_by_key(|e| e.ts);
    Ok(out)
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
    parse_lines(&text)
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

    #[test]
    fn shared_events_merge_sorted_and_skip_junk() {
        let dir = tempfile::tempdir().unwrap();
        let ev = |ts, user: &str| {
            serde_json::to_string(&Event {
                ts,
                user: user.into(),
                action: "merge".into(),
                project: "p".into(),
                branch: "b".into(),
                ok: true,
            })
            .unwrap()
        };
        std::fs::write(
            dir.path().join("audit-ania.log"),
            format!("{}\nnot json\n{}\n", ev(5, "ania"), ev(1, "ania")),
        )
        .unwrap();
        std::fs::write(dir.path().join("audit-marek.log"), ev(3, "marek")).unwrap();
        std::fs::write(dir.path().join("ania.json"), "{}").unwrap();
        let all = read_shared(dir.path()).unwrap();
        let ts: Vec<_> = all.iter().map(|e| e.ts).collect();
        assert_eq!(ts, vec![1, 3, 5]);
        assert_eq!(shared_file_name("an/ia"), "audit-an_ia.log");
    }
}
