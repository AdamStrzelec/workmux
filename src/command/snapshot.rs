//! `workmux snapshot`: save, list and restore the working tree of the current worktree.

use anyhow::Result;

use crate::snapshot;

pub fn save(label: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let snap = snapshot::save(&cwd, label.as_deref().unwrap_or(""))?;
    snapshot::auto_prune(&cwd);
    println!("Saved snapshot {}", snap.id);
    Ok(())
}

pub fn list(json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let snaps = snapshot::list(&cwd)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&snaps)?);
    } else if snaps.is_empty() {
        println!("No snapshots on this branch.");
    } else {
        for s in &snaps {
            println!("{}", s.id);
        }
    }
    Ok(())
}

pub fn restore(id: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let before = snapshot::restore(&cwd, id)?;
    println!(
        "Restored {id}. Previous state saved as {} (restore it to undo).",
        before.id
    );
    Ok(())
}

pub fn prune(keep: usize) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let removed = snapshot::prune(&cwd, keep)?;
    println!("Removed {} snapshot(s), kept up to {keep}.", removed.len());
    Ok(())
}
