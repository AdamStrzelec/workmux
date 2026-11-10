//! `workmux collisions`: same file in several worktrees, same branch for
//! several users (team dir).

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::collision;
use crate::config::Config;
use crate::guard;
use crate::team;

pub fn run(json: bool) -> Result<()> {
    let listing = crate::git::list_worktrees_with_main_in(None)?;
    let base = crate::git::get_default_branch()?;
    let base = crate::git::get_merge_base_in(Some(&listing.main_path), &base)?;

    let mut changes: Vec<(String, Vec<String>)> = Vec::new();
    for (path, branch) in &listing.worktrees {
        if *path == listing.main_path {
            continue;
        }
        changes.push((branch.clone(), guard::changed_files(path, &base)?));
    }
    let files = collision::file_overlaps(&changes);

    let cfg = Config::global_team_config();
    let project = team::project_of(&std::env::current_dir().unwrap_or_default());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let branches = team::resolve_dir(&cfg)
        .and_then(|dir| team::read_snapshots(&dir))
        .map(|snaps| collision::shared_branches(&snaps, &project, now, cfg.stale_after_secs))
        .unwrap_or_default();

    if json {
        let v = serde_json::json!({ "files": files, "branches": branches });
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else if files.is_empty() && branches.is_empty() {
        println!("No collisions.");
    } else {
        for (f, b) in &files {
            println!("file {f}: {}", b.join(", "));
        }
        for (b, u) in &branches {
            println!("branch {b}: {}", u.join(", "));
        }
    }
    if !files.is_empty() || !branches.is_empty() {
        std::process::exit(2);
    }
    Ok(())
}
