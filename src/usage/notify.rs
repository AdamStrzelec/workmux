//! Deliver usage alerts: system notification plus a tmux message on every
//! attached client (the only channel that reaches a user over SSH).

use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use super::alert::{self, Alert, UsageConfig, WindowKind};
use super::{Snapshot, now_unix};

type Delivered = Vec<(WindowKind, i64)>;

fn state_path() -> Result<PathBuf> {
    Ok(crate::xdg::state_dir()?.join("usage-alerts.json"))
}

fn load_delivered(path: &std::path::Path) -> Delivered {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_delivered(path: &std::path::Path, delivered: &Delivered) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(delivered)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Drop entries for cycles that already reset; they can never match again.
fn prune(delivered: &mut Delivered, now: i64) {
    delivered.retain(|(_, resets_at)| *resets_at > now);
}

/// Evaluate the configured rules and notify for any newly due alert.
/// Never fails the caller: alerting is best effort.
pub fn check_and_notify(config: &UsageConfig, snap: &Snapshot) {
    if config.alerts.is_empty() {
        return;
    }
    let Ok(path) = state_path() else { return };
    let Some(_lock) = super::cache::try_lock(&path.with_extension("lock")) else {
        return; // another process is delivering right now
    };
    let now = now_unix();
    let mut delivered = load_delivered(&path);
    prune(&mut delivered, now);

    let due = alert::due(&config.alerts, snap, now, &delivered);
    for a in &due {
        send(a);
        delivered.push((a.window, a.resets_at_unix));
    }
    let _ = save_delivered(&path, &delivered);
}

fn send(alert: &Alert) {
    let message = alert.message();
    crate::workflow::merge::show_notification(&message);
    tmux_message(&message);
}

/// Show `message` for 10 s on every attached tmux client.
fn tmux_message(message: &str) {
    let Ok(out) = Command::new("tmux")
        .args(["list-clients", "-F", "#{client_name}"])
        .output()
    else {
        return;
    };
    for client in String::from_utf8_lossy(&out.stdout).lines() {
        let _ = Command::new("tmux")
            .args(["display-message", "-c", client, "-d", "10000", message])
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_drops_past_cycles_only() {
        let mut d = vec![(WindowKind::Session, 100), (WindowKind::Weekly, 900)];
        prune(&mut d, 500);
        assert_eq!(d, vec![(WindowKind::Weekly, 900)]);
    }

    #[test]
    fn delivered_roundtrip_and_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        assert!(load_delivered(&path).is_empty());
        save_delivered(&path, &vec![(WindowKind::Session, 7)]).unwrap();
        assert_eq!(load_delivered(&path), vec![(WindowKind::Session, 7)]);
        std::fs::write(&path, "junk").unwrap();
        assert!(load_delivered(&path).is_empty());
    }
}
