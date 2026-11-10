//! `workmux guard`: report changes to protected paths in each worktree.

use anyhow::Result;

use crate::config::Config;
use crate::guard;

pub fn run(json: bool) -> Result<()> {
    let cfg = Config::global_guard_config();
    if cfg.protected_paths.is_empty() {
        println!("No protected paths configured (guard.protected_paths in the global config).");
        return Ok(());
    }
    let listing = crate::git::list_worktrees_with_main_in(None)?;
    let base = crate::git::get_default_branch()?;
    let base = crate::git::get_merge_base_in(Some(&listing.main_path), &base)?;

    let mut report: Vec<(String, Vec<String>)> = Vec::new();
    for (path, branch) in &listing.worktrees {
        if *path == listing.main_path {
            continue;
        }
        let hits = guard::violations(&cfg, &guard::changed_files(path, &base)?);
        if !hits.is_empty() {
            report.push((branch.clone(), hits));
        }
    }

    if json {
        let map: serde_json::Map<_, _> = report
            .iter()
            .map(|(b, f)| (b.clone(), serde_json::json!(f)))
            .collect();
        println!("{}", serde_json::to_string_pretty(&map)?);
    } else if report.is_empty() {
        println!("No protected paths changed.");
    } else {
        for (branch, files) in &report {
            println!("{branch}");
            for f in files {
                println!("  {f}");
            }
        }
    }
    if !report.is_empty() {
        std::process::exit(2);
    }
    Ok(())
}
