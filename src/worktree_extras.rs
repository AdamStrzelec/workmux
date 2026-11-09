//! Side effects shared by every way a worktree window is closed or removed
//! (dashboard `c`/`d`/sweep, `workmux close`, `workmux remove`/`merge`):
//! killing run tool windows and remembering the Claude session to resume.

use std::collections::HashSet;
use std::path::Path;

use crate::claude_launch::is_tool_window_of;
use crate::multiplexer::conversation::{ClaudeForker, ConversationForker};
use crate::multiplexer::{Multiplexer, WindowTarget};

/// `(window, session)` pairs of the tool windows of `handle`.
pub fn tool_windows_of(
    windows: &HashSet<(String, String)>,
    prefix: &str,
    handle: &str,
) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = windows
        .iter()
        .filter(|(window, _)| {
            window
                .strip_prefix(prefix)
                .is_some_and(|name| is_tool_window_of(name, handle))
        })
        .cloned()
        .collect();
    found.sort();
    found
}

/// Kill every tool window of `handle`. Returns how many were closed.
pub fn kill_tool_windows(mux: &dyn Multiplexer, prefix: &str, handle: &str) -> usize {
    let windows = mux.get_all_windows_with_sessions().unwrap_or_default();
    tool_windows_of(&windows, prefix, handle)
        .into_iter()
        .filter(|(window, session)| {
            mux.kill_window_target(&WindowTarget::new(window.clone(), Some(session.clone())))
                .is_ok()
        })
        .count()
}

/// Whether closing the worktree window also closes its tool windows.
pub fn close_tools_with_session(path: &Path) -> bool {
    crate::config::Config::load_with_location_from(path, None)
        .ok()
        .and_then(|(config, _)| config.tools)
        .is_none_or(|t| t.close_with_session())
}

/// Remember the session of a Claude agent running in the worktree so the
/// next `open` resumes it. `pane_id` narrows the lookup when known.
/// Best effort: failures only skip the resume.
pub fn save_claude_session(handle: &str, path: &Path, pane_id: Option<&str>) {
    let agents = crate::state::StateStore::new()
        .ok()
        .and_then(|store| store.list_all_agents().ok())
        .unwrap_or_default();
    let mut candidates: Vec<_> = agents
        .into_iter()
        .filter(|a| a.workdir.starts_with(path))
        .filter(|a| pane_id.is_none_or(|p| a.pane_key.pane_id == p))
        .filter(|a| a.agent_kind.as_deref().is_none_or(|k| k == "claude"))
        .collect();
    // Exact worktree root first, then most recently active.
    candidates.sort_by_key(|a| (a.workdir != path, std::cmp::Reverse(a.updated_ts)));
    let Some(agent) = candidates.into_iter().next() else {
        return;
    };
    let session = agent.agent_session_id.clone().or_else(|| {
        ClaudeForker::new()
            .find_latest_conversation(&agent.workdir)
            .ok()
            .flatten()
            .map(|s| s.id)
    });
    if let Some(id) = session {
        let _ = crate::git::set_worktree_meta_in(
            handle,
            crate::git::CLAUDE_SESSION_META,
            &id,
            Some(path),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_only_tool_windows_of_handle() {
        let windows: HashSet<(String, String)> = [
            ("wm-feat/dev".to_string(), "s".to_string()),
            ("wm-feat".to_string(), "s".to_string()),
            ("wm-feature/dev".to_string(), "s".to_string()),
            ("other".to_string(), "s".to_string()),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            tool_windows_of(&windows, "wm-", "feat"),
            vec![("wm-feat/dev".to_string(), "s".to_string())]
        );
    }
}
