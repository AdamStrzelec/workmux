use anyhow::Result;
use serde::Serialize;

use crate::usage::{self, Snapshot, cache, claude, format};

#[derive(Serialize)]
struct JsonOutput<'a> {
    #[serde(flatten)]
    snapshot: &'a Snapshot,
}

pub fn run(json: bool, tmux: bool, refresh: bool) -> Result<()> {
    let snap = match cache::get_or_refresh(refresh, claude::probe) {
        Ok(snap) => snap,
        // A status bar must stay quiet when the probe fails.
        Err(_) if tmux => return Ok(()),
        Err(e) => return Err(e),
    };
    usage::notify::check_and_notify(&crate::config::Config::global_usage_config(), &snap);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&JsonOutput { snapshot: &snap })?
        );
    } else if tmux {
        println!("{}", format::tmux_status(&snap));
    } else {
        print_table(&snap);
    }
    Ok(())
}

fn print_table(snap: &Snapshot) {
    let now = usage::now_unix();
    let line = |label: &str, w: &Option<usage::Window>| {
        if let Some(w) = w {
            println!("{label:<14}{}  {}", bar_of(w), format::describe(w, now));
        }
    };
    line("5h session", &snap.session);
    line("Weekly", &snap.weekly);
    for s in &snap.scoped {
        println!(
            "{:<14}{}  {}",
            format!("Weekly {}", s.label),
            bar_of(&s.window),
            format::describe(&s.window, now)
        );
    }
}

/// Colored 20-cell bar, blank cells when the percentage is unknown.
fn bar_of(w: &usage::Window) -> String {
    let Some(p) = w.used_percent else {
        return " ".repeat(20);
    };
    let bar = format::bar(p, 20);
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return bar;
    }
    let color = match format::level(p) {
        format::Level::Ok => "32",
        format::Level::Warn => "33",
        format::Level::Critical => "31",
    };
    format!("\x1b[{color}m{bar}\x1b[0m")
}
