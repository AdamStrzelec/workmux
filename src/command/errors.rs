//! `workmux errors`: read the local error journal.

use anyhow::Result;

use crate::errlog;

pub fn run(json: bool, last: usize) -> Result<()> {
    let all = errlog::read_all();
    errlog::mark_seen();
    let skip = all.len().saturating_sub(last);
    let records = &all[skip..];
    if json {
        println!("{}", serde_json::to_string_pretty(records)?);
        return Ok(());
    }
    if records.is_empty() {
        match errlog::path() {
            Some(p) => println!("No errors recorded ({}).", p.display()),
            None => println!("No errors recorded."),
        }
        return Ok(());
    }
    for r in records {
        let loc = r
            .location
            .as_deref()
            .map(|l| format!(" at {l}"))
            .unwrap_or_default();
        println!(
            "{} [{}] workmux {} v{}: {}{}",
            r.ts, r.kind, r.command, r.version, r.message, loc
        );
    }
    Ok(())
}
