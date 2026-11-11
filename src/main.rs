mod agent_display;
mod agent_error;
mod agent_identity;
mod agent_setup;
mod agent_staleness;
mod audit;
mod claude;
mod claude_launch;
mod cli;
mod cmd;
mod collision;
mod command;
mod config;
mod config_yaml;
mod consult;
mod creation_time;
mod enforce;
mod errlog;
mod frozen_config;
mod git;
mod github;
mod gitlab;
mod global_init;
mod guard;
mod history;
mod hunk;
mod llm;
mod logger;
mod markdown;
mod multiplexer;
mod naming;
mod nerdfont;
mod oversight;
mod ports;
mod profile;
mod prompt;
mod remote;
mod sandbox;
mod shell;
mod skills;
mod snapshot;
mod spinner;
mod state;
mod team;
mod template;
#[cfg(test)]
mod test_support;
mod tips;
mod tmux_style;
mod ui;
mod usage;
mod util;
mod workflow;
mod worktree_extras;
mod xdg;

use anyhow::Result;
use tracing::{error, info};

fn main() -> Result<()> {
    logger::init()?;
    let context = LogContext::current();
    let args = std::env::args().collect::<Vec<_>>();
    let command_name = errlog::command_name(&args);
    errlog::install_panic_hook(command_name.clone());
    let deferred_cleanup_worker = args.get(1).is_some_and(|arg| arg == "_deferred-cleanup");
    info!(
        args = ?args,
        cwd = ?context.cwd,
        tmux_pane = ?context.tmux_pane,
        "workmux start"
    );

    let (safety_profile, profile_note) = config::Config::global_safety_profile();
    let result = profile::check_noted(safety_profile, profile_note.as_deref(), &args)
        .and_then(|()| cli::run());
    match result {
        Ok(result) => {
            info!(
                cwd = ?context.cwd,
                tmux_pane = ?context.tmux_pane,
                "workmux finished successfully"
            );
            if !deferred_cleanup_worker {
                audit::record_command(&args, true);
            }
            Ok(result)
        }
        Err(err) => {
            if !deferred_cleanup_worker {
                error!(error = ?err, "workmux failed");
                errlog::record("error", &command_name, &format!("{err:#}"), None, None);
                audit::record_command(&args, false);
            }
            if let Some(kind) = agent_error::kind_of(&err) {
                eprintln!("Error: {}: {err:#}", kind.name());
                std::process::exit(kind.exit_code());
            }
            Err(err)
        }
    }
}

struct LogContext {
    cwd: Option<std::path::PathBuf>,
    tmux_pane: Option<String>,
}

impl LogContext {
    fn current() -> Self {
        Self {
            cwd: std::env::current_dir().ok(),
            tmux_pane: std::env::var("TMUX_PANE").ok(),
        }
    }
}
