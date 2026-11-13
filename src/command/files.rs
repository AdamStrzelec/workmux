//! `workmux files`: choose untracked/ignored files that every new worktree
//! gets as symlinks. The selection is per user and per repo (see `symlinks`).

use anyhow::{Result, bail};
use std::io::{BufRead, Write};

use crate::cli::FilesCommands;
use crate::{git, symlinks};

pub fn run(command: Option<FilesCommands>) -> Result<()> {
    let root = git::get_main_worktree_root()?;
    match command.unwrap_or(FilesCommands::Select) {
        FilesCommands::List => list(&root),
        FilesCommands::Select => select(&root),
        FilesCommands::Add { paths } => {
            let mut cur = symlinks::selected(&root);
            for p in &paths {
                let pat = symlinks::pattern_for(p);
                if !cur.contains(&pat) {
                    cur.push(pat);
                }
            }
            symlinks::set_selected(&root, cur)?;
            println!("Added {} path(s).", paths.len());
            Ok(())
        }
        FilesCommands::Remove { paths } => {
            let drop: Vec<String> = paths.iter().map(|p| symlinks::pattern_for(p)).collect();
            let mut cur = symlinks::selected(&root);
            cur.retain(|p| !drop.contains(p));
            symlinks::set_selected(&root, cur)?;
            println!("Removed.");
            Ok(())
        }
        FilesCommands::Clear => {
            symlinks::set_selected(&root, Vec::new())?;
            println!("Selection cleared.");
            Ok(())
        }
    }
}

fn list(root: &std::path::Path) -> Result<()> {
    let sel = symlinks::selected(root);
    println!("Selected (symlinked into every new worktree):");
    if sel.is_empty() {
        println!("  (none)");
    }
    for p in &sel {
        println!("  {p}");
    }
    println!("\nAvailable (untracked or ignored):");
    for c in symlinks::candidates(root)? {
        let mark = if sel.contains(&symlinks::pattern_for(&c)) {
            "x"
        } else {
            " "
        };
        println!("  [{mark}] {c}");
    }
    Ok(())
}

fn select(root: &std::path::Path) -> Result<()> {
    let cands = symlinks::candidates(root)?;
    if cands.is_empty() {
        println!("No untracked or ignored files found in {}.", root.display());
        return Ok(());
    }
    let sel = symlinks::selected(root);
    let mut marked: Vec<bool> = cands
        .iter()
        .map(|c| sel.contains(&symlinks::pattern_for(c)))
        .collect();
    // Patterns the user added by hand that match no candidate stay untouched.
    let keep: Vec<String> = sel
        .iter()
        .filter(|p| !cands.iter().any(|c| &symlinks::pattern_for(c) == *p))
        .cloned()
        .collect();
    let stdin = std::io::stdin();
    loop {
        for (i, c) in cands.iter().enumerate() {
            println!("{:>3} [{}] {c}", i + 1, if marked[i] { "x" } else { " " });
        }
        print!("Toggle numbers (1 3 5-7, all, none), Enter = save, q = cancel: ");
        std::io::stdout().flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            bail!("no input (stdin closed); selection not saved");
        }
        let line = line.trim();
        if line == "q" {
            println!("Cancelled.");
            return Ok(());
        }
        if line.is_empty() {
            break;
        }
        if !symlinks::apply_input(line, &mut marked) {
            println!("Not understood, nothing changed.");
        }
    }
    let mut out = keep;
    out.extend(
        cands
            .iter()
            .zip(&marked)
            .filter(|(_, m)| **m)
            .map(|(c, _)| symlinks::pattern_for(c)),
    );
    let n = out.len();
    symlinks::set_selected(root, out)?;
    println!("Saved {n} path(s). New worktrees will symlink them (skip with -F).");
    Ok(())
}
