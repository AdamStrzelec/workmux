//! `workmux team`: who works on what across users sharing a team directory.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::config::Config;
use crate::multiplexer::{create_backend, detect_backend_strict};
use crate::state::StateStore;
use crate::team::{self, Entry, Snapshot, entry_of};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn local_entries() -> Result<Vec<Entry>> {
    let mux = create_backend(detect_backend_strict()?);
    let store = StateStore::open_read_only()?;
    let report = store.load_reconciled_agent_report(mux.as_ref())?;
    Ok(report.agents.iter().map(entry_of).collect())
}

pub fn run(json: bool, no_publish: bool) -> Result<()> {
    let cfg = Config::global_team_config();
    let dir = team::resolve_dir(&cfg)?;
    let now = now();

    if !no_publish {
        let snap = team::new_snapshot(&cfg, team::apply_privacy(&cfg, local_entries()?), now);
        team::write_snapshot(&dir, &snap)?;
    }
    let snaps = team::read_snapshots(&dir)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&snaps)?);
        return Ok(());
    }
    print_table(&snaps, now, cfg.stale_after_secs);
    Ok(())
}

fn print_table(snaps: &[Snapshot], now: u64, stale_after: u64) {
    if snaps.is_empty() {
        println!("No team snapshots yet.");
        return;
    }
    let header = ["USER", "PROJECT", "BRANCH", "STATUS", "WINDOW", "ACTIVE"];
    let mut rows: Vec<[String; 6]> = Vec::new();
    for s in snaps {
        let stale = now.saturating_sub(s.published_at) > stale_after;
        let user = if stale {
            format!("{} (stale {})", s.user, team::age(now, s.published_at))
        } else {
            s.user.clone()
        };
        if s.agents.is_empty() {
            rows.push([
                user,
                "-".into(),
                "-".into(),
                "idle".into(),
                "-".into(),
                "-".into(),
            ]);
            continue;
        }
        for (i, a) in s.agents.iter().enumerate() {
            rows.push([
                if i == 0 { user.clone() } else { String::new() },
                a.project.clone(),
                a.branch.clone(),
                a.status.clone(),
                format!("{}:{}", a.session, a.window),
                a.activity_ts.map_or("-".into(), |t| team::age(now, t)),
            ]);
        }
    }
    let mut widths = header.map(str::len);
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let print = |cells: [&str; 6]| {
        let line: Vec<String> = cells
            .iter()
            .zip(widths)
            .map(|(c, w)| format!("{c:<w$}"))
            .collect();
        println!("{}", line.join("  ").trim_end());
    };
    print(header);
    for row in &rows {
        print(row.each_ref().map(String::as_str));
    }
}
