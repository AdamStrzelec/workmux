//! `workmux audit`: read the local audit log.

use anyhow::Result;

use crate::audit;

pub fn run(json: bool, last: usize, team: bool) -> Result<()> {
    let all = if team {
        let cfg = crate::config::Config::global_team_config();
        let dir = crate::team::resolve_dir(&cfg)?;
        audit::read_shared(&dir)?
    } else {
        audit::read_all()
    };
    let skip = all.len().saturating_sub(last);
    let events = &all[skip..];
    if json {
        println!("{}", serde_json::to_string_pretty(events)?);
        return Ok(());
    }
    if events.is_empty() {
        println!("No audit events recorded.");
        return Ok(());
    }
    for e in events {
        println!(
            "{} {} {} {}:{} {}",
            e.ts,
            e.user,
            e.action,
            e.project,
            e.branch,
            if e.ok { "ok" } else { "FAILED" }
        );
    }
    Ok(())
}
