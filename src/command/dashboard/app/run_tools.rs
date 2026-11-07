//! Run tools menu (`x`, worktrees tab): start configured commands in their
//! own tmux window per worktree, jump to them when already running, and
//! clean them up when the worktree window is closed or removed. Also saves
//! the Claude session of a worktree when its window is closed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::claude_launch::{ToolConfig, is_tool_window_of, tool_window_name};
use crate::multiplexer::WindowTarget;
use crate::multiplexer::conversation::{ClaudeForker, ConversationForker};

use super::App;

/// Open run tools menu.
pub struct ToolMenu {
    pub handle: String,
    pub path: PathBuf,
    pub tools: Vec<ToolConfig>,
    /// Parallel to `tools`: a window for the tool is open.
    pub running: Vec<bool>,
}

impl ToolMenu {
    pub fn tool_for_key(&self, c: char) -> Option<usize> {
        self.tools.iter().position(|t| t.key_char() == Some(c))
    }
}

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

/// Session of an open window named `full_name`, if any.
pub fn find_window<'a>(
    windows: &'a HashSet<(String, String)>,
    full_name: &str,
) -> Option<&'a (String, String)> {
    let mut matches: Vec<_> = windows.iter().filter(|(w, _)| w == full_name).collect();
    matches.sort();
    matches.into_iter().next()
}

impl App {
    fn tools_for(&self, path: &Path) -> Vec<ToolConfig> {
        crate::config::Config::load_with_location_from(path, None)
            .ok()
            .and_then(|(config, _)| config.tools)
            .map(|t| t.items)
            .unwrap_or_default()
    }

    fn all_windows(&self) -> HashSet<(String, String)> {
        self.mux.get_all_windows_with_sessions().unwrap_or_default()
    }

    /// Open the run tools menu for the selected worktree.
    pub fn show_tool_menu(&mut self) {
        let Some(wt) = self
            .worktree_table_state
            .selected()
            .and_then(|i| self.worktrees.get(i))
        else {
            return;
        };
        if self.removals.contains(&wt.path) {
            return;
        }
        let (handle, path) = (wt.handle.clone(), wt.path.clone());
        let tools = self.tools_for(&path);
        if tools.is_empty() {
            self.status_message = Some((
                "No tools configured (add them with C or `tools:` in .workmux.yaml)".to_string(),
                std::time::Instant::now(),
            ));
            return;
        }
        let windows = self.all_windows();
        let prefix = self.config.window_prefix().to_string();
        let running = tools
            .iter()
            .map(|t| {
                let full = crate::multiplexer::util::prefixed(
                    &prefix,
                    &tool_window_name(&handle, &t.name),
                );
                find_window(&windows, &full).is_some()
            })
            .collect();
        self.pending_tool_menu = Some(ToolMenu {
            handle,
            path,
            tools,
            running,
        });
    }

    /// Run (or focus) the tool bound to `c`.
    pub fn tool_menu_select(&mut self, c: char) {
        let Some(menu) = self.pending_tool_menu.as_ref() else {
            return;
        };
        let Some(idx) = menu.tool_for_key(c) else {
            return;
        };
        let menu = self.pending_tool_menu.take().expect("menu checked above");
        let tool = menu.tools[idx].clone();
        let message = match self.run_tool(&menu.handle, &menu.path, &tool) {
            Ok(true) => format!("{}: already running, switched to it", tool.name),
            Ok(false) => format!("{}: started", tool.name),
            Err(e) => format!("Failed to run {}: {e:#}", tool.name),
        };
        self.status_message = Some((message, std::time::Instant::now()));
        self.trigger_worktree_refetch();
    }

    /// Returns Ok(true) when an existing window was focused.
    fn run_tool(&mut self, handle: &str, path: &Path, tool: &ToolConfig) -> anyhow::Result<bool> {
        let prefix = self.config.window_prefix().to_string();
        let full =
            crate::multiplexer::util::prefixed(&prefix, &tool_window_name(handle, &tool.name));
        let windows = self.all_windows();
        if let Some((window, session)) = find_window(&windows, &full) {
            self.mux
                .select_window_target(&WindowTarget::new(window.clone(), Some(session.clone())))?;
            return Ok(true);
        }
        // Session mode worktrees get the tool window inside their own session.
        let session = {
            let own = crate::multiplexer::util::prefixed(&prefix, handle);
            if self.mux.session_exists(&own).unwrap_or(false) {
                Some(own)
            } else {
                self.launch_session.clone()
            }
        };
        let pane = match session.as_deref() {
            Some(session) => crate::multiplexer::util::create_window_in_parent_session(
                self.mux.as_ref(),
                session,
                &full,
                path,
            )?,
            None => self
                .mux
                .create_window(crate::multiplexer::CreateWindowParams {
                    prefix: "",
                    name: &full,
                    cwd: path,
                    after_window: None,
                })?,
        };
        if !tool.command.trim().is_empty() {
            self.mux.send_keys(&pane, tool.command.trim())?;
        }
        self.mux
            .select_window_target(&WindowTarget::new(full, session))?;
        Ok(false)
    }

    /// Kill every tool window of `handle`. Returns how many were closed.
    pub(super) fn kill_tool_windows(&self, handle: &str) -> usize {
        let windows = self.all_windows();
        let found = tool_windows_of(&windows, self.config.window_prefix(), handle);
        found
            .into_iter()
            .filter(|(window, session)| {
                self.mux
                    .kill_window_target(&WindowTarget::new(window.clone(), Some(session.clone())))
                    .is_ok()
            })
            .count()
    }

    /// Whether closing the worktree window also closes its tool windows.
    pub(super) fn close_tools_with_session(&self, path: &Path) -> bool {
        crate::config::Config::load_with_location_from(path, None)
            .ok()
            .and_then(|(config, _)| config.tools)
            .is_none_or(|t| t.close_with_session())
    }

    /// Remember the Claude session of the worktree's agent so reopening the
    /// window resumes it. Best effort: failures only skip the resume.
    pub(super) fn save_claude_session(&self, handle: &str, path: &Path) {
        // Exact cwd first, else a subdirectory not owned by a nested worktree.
        let owned_by_nested = |p: &Path| {
            self.worktrees
                .iter()
                .any(|w| w.path != path && w.path.starts_with(path) && p.starts_with(&w.path))
        };
        let Some(agent) = self.all_agents.iter().find(|a| a.path == path).or_else(|| {
            self.all_agents
                .iter()
                .find(|a| a.path.starts_with(path) && !owned_by_nested(&a.path))
        }) else {
            return;
        };
        let is_claude = agent.agent_kind.as_deref() == Some("claude")
            || agent.agent_command.as_deref().is_some_and(|c| {
                c.split_whitespace()
                    .next()
                    .is_some_and(|p| p.ends_with("claude"))
            });
        if !is_claude {
            return;
        }
        let from_state = crate::state::StateStore::new()
            .ok()
            .and_then(|store| store.list_all_agents().ok())
            .and_then(|agents| {
                agents
                    .into_iter()
                    .find(|s| s.pane_key.pane_id == agent.pane_id && s.workdir == agent.path)
            })
            .and_then(|s| s.agent_session_id);
        let session = from_state.or_else(|| {
            ClaudeForker::new()
                .find_latest_conversation(&agent.path)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[(&str, &str)]) -> HashSet<(String, String)> {
        items
            .iter()
            .map(|(w, s)| (w.to_string(), s.to_string()))
            .collect()
    }

    #[test]
    fn finds_only_tool_windows_of_handle() {
        let windows = set(&[
            ("wm-feat", "main"),
            ("wm-feat/dev", "main"),
            ("wm-feat/test", "other"),
            ("wm-feat-2/dev", "main"),
            ("feat/dev", "main"),
        ]);
        assert_eq!(
            tool_windows_of(&windows, "wm-", "feat"),
            vec![
                ("wm-feat/dev".to_string(), "main".to_string()),
                ("wm-feat/test".to_string(), "other".to_string())
            ]
        );
        assert!(tool_windows_of(&windows, "wm-", "nope").is_empty());
    }

    #[test]
    fn find_window_by_full_name() {
        let windows = set(&[("wm-feat/dev", "s1"), ("wm-feat", "s1")]);
        assert_eq!(
            find_window(&windows, "wm-feat/dev").map(|(_, s)| s.as_str()),
            Some("s1")
        );
        assert!(find_window(&windows, "wm-feat/test").is_none());
    }

    #[test]
    fn tool_menu_key_lookup() {
        let menu = ToolMenu {
            handle: "h".into(),
            path: PathBuf::from("/x"),
            tools: vec![
                ToolConfig {
                    key: "d".into(),
                    name: "dev".into(),
                    command: "npm run dev".into(),
                },
                ToolConfig {
                    key: "t".into(),
                    name: "test".into(),
                    command: "npm test".into(),
                },
            ],
            running: vec![false, true],
        };
        assert_eq!(menu.tool_for_key('t'), Some(1));
        assert_eq!(menu.tool_for_key('z'), None);
    }
}
