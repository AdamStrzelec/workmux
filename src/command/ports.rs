//! `workmux ports`: listening TCP ports across all projects.

use anyhow::Result;

use crate::ports::{self, Listener};

fn name_of(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

pub fn run(json: bool, all: bool) -> Result<()> {
    let listeners: Vec<Listener> = ports::scan()
        .into_iter()
        .filter(|l| all || l.worktree.is_some() || l.window.is_some())
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&listeners)?);
        return Ok(());
    }
    if listeners.is_empty() {
        println!(
            "No listening ports{}.",
            if all {
                ""
            } else {
                " in worktrees or tmux (use --all)"
            }
        );
        return Ok(());
    }
    let rows: Vec<[String; 5]> = listeners
        .iter()
        .map(|l| {
            [
                l.port.to_string(),
                format!("{} {}", l.command, l.pid),
                l.worktree
                    .as_deref()
                    .map(name_of)
                    .unwrap_or_else(|| "-".into()),
                l.tool().unwrap_or("-").to_string(),
                l.worktree
                    .as_deref()
                    .or(l.cwd.as_deref())
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "-".into()),
            ]
        })
        .collect();
    let header = ["PORT", "PROCESS", "WORKTREE", "TOOL", "PATH"];
    let mut widths = header.map(str::len);
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let print = |cells: [&str; 5]| {
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
    Ok(())
}
