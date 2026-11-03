//! Session tab editor: edit the `windows` layout of a project's `.workmux.yaml`.
//!
//! Each tab maps to one tmux window with a single pane running one command.
//! Extra panes of an existing window are preserved untouched on save.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_yaml::{Mapping, Value};

use super::App;
use crate::cmd::Cmd;

const CONFIG_NAMES: [&str; 2] = [".workmux.yaml", ".workmux.yml"];

/// One tmux tab (window) in the session layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTab {
    pub name: String,
    /// Command for the tab's first pane. Empty means a plain shell.
    pub command: String,
    /// Further panes of the window, kept as-is.
    pub extra_panes: Vec<Value>,
}

impl SessionTab {
    fn new(name: &str, command: &str) -> Self {
        Self {
            name: name.to_string(),
            command: command.to_string(),
            extra_panes: Vec::new(),
        }
    }
}

/// Field currently being typed in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTabField {
    Name,
    Command,
}

/// Inline text edit of one field of the tab under the cursor.
pub struct SessionTabEdit {
    pub field: SessionTabField,
    pub buffer: String,
}

/// State of the session config modal.
pub struct SessionConfigEditor {
    pub project_name: String,
    pub repo_root: PathBuf,
    pub config_path: PathBuf,
    pub tabs: Vec<SessionTab>,
    pub cursor: usize,
    /// Tab focused when the session opens.
    pub focus: usize,
    pub edit: Option<SessionTabEdit>,
    pub dirty: bool,
}

/// Locate an existing project config, or the default path to create.
fn config_path_for(repo_root: &Path) -> PathBuf {
    CONFIG_NAMES
        .iter()
        .map(|name| repo_root.join(name))
        .find(|p| p.exists())
        .unwrap_or_else(|| repo_root.join(CONFIG_NAMES[0]))
}

fn pane_command(pane: &Value) -> String {
    pane.get("command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn pane_focus(pane: &Value) -> bool {
    pane.get("focus").and_then(Value::as_bool).unwrap_or(false)
}

/// Read tabs from a config document. `windows` maps 1:1; plain `panes`
/// become one tab per pane; nothing configured gives agent + shell.
pub fn tabs_from_yaml(doc: &Value) -> (Vec<SessionTab>, usize) {
    let mut tabs = Vec::new();
    let mut focus = 0;

    if let Some(windows) = doc.get("windows").and_then(Value::as_sequence) {
        for (i, window) in windows.iter().enumerate() {
            let name = window
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let panes = window
                .get("panes")
                .and_then(Value::as_sequence)
                .cloned()
                .unwrap_or_default();
            if panes.iter().any(pane_focus) {
                focus = i;
            }
            let command = panes.first().map(pane_command).unwrap_or_default();
            let extra_panes = panes.into_iter().skip(1).collect();
            tabs.push(SessionTab {
                name,
                command,
                extra_panes,
            });
        }
    } else if let Some(panes) = doc.get("panes").and_then(Value::as_sequence) {
        for (i, pane) in panes.iter().enumerate() {
            if pane_focus(pane) {
                focus = i;
            }
            let command = pane_command(pane);
            let name = if command == "<agent>" { "agent" } else { "" };
            tabs.push(SessionTab::new(name, &command));
        }
    }

    if tabs.is_empty() {
        tabs = vec![
            SessionTab::new("agent", "<agent>"),
            SessionTab::new("shell", ""),
        ];
        focus = 0;
    }
    let focus = focus.min(tabs.len() - 1);
    (tabs, focus)
}

/// Write tabs into a config document as `windows` + `mode: session`,
/// removing top-level `panes`. Other keys are left untouched.
pub fn apply_tabs_to_yaml(doc: Value, tabs: &[SessionTab], focus: usize) -> Value {
    let mut map = match doc {
        Value::Mapping(map) => map,
        _ => Mapping::new(),
    };
    map.remove("panes");

    let windows: Vec<Value> = tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let mut pane = Mapping::new();
            let command = tab.command.trim();
            if !command.is_empty() {
                pane.insert("command".into(), command.into());
            }
            if i == focus {
                pane.insert("focus".into(), true.into());
            }
            let mut panes = vec![Value::Mapping(pane)];
            panes.extend(tab.extra_panes.iter().cloned());

            let mut window = Mapping::new();
            let name = tab.name.trim();
            if !name.is_empty() {
                window.insert("name".into(), name.into());
            }
            window.insert("panes".into(), Value::Sequence(panes));
            Value::Mapping(window)
        })
        .collect();

    map.insert("mode".into(), "session".into());
    map.insert("windows".into(), Value::Sequence(windows));
    Value::Mapping(map)
}

const LAYOUT_KEYS: [&str; 3] = ["panes", "windows", "mode"];

/// Top-level key a column-0 line starts, if any (`key:` or `key: value`).
fn top_level_key(line: &str) -> Option<&str> {
    if line.starts_with([' ', '\t', '#', '-']) || line.trim().is_empty() {
        return None;
    }
    line.split_once(':').map(|(key, _)| key.trim())
}

/// Replace layout keys in the config text, keeping every other line
/// (including comments and key order) byte for byte.
pub fn render_config_text(text: &str, tabs: &[SessionTab], focus: usize) -> Result<String> {
    let doc: Value = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_yaml::from_str(text).context("parse project config")?
    };
    if !matches!(doc, Value::Mapping(_) | Value::Null) {
        anyhow::bail!("project config is not a YAML mapping");
    }

    let mut kept = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if let Some(key) = top_level_key(line) {
            skipping = LAYOUT_KEYS.contains(&key);
        } else if skipping && (line.trim().is_empty() || line.starts_with('#')) {
            // Blank lines and column-0 comments separate blocks; keep them.
            kept.push(line);
            continue;
        }
        if !skipping {
            kept.push(line);
        }
    }
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    while kept.first().is_some_and(|l| l.trim().is_empty()) {
        kept.remove(0);
    }

    let layout = apply_tabs_to_yaml(Value::Null, tabs, focus);
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(&serde_yaml::to_string(&layout)?);

    // Guard: the result must parse and keep every non-layout key.
    let parsed: Value = serde_yaml::from_str(&out).context("generated config is invalid")?;
    if let Value::Mapping(original) = &doc {
        for key in original.keys() {
            let is_layout = key.as_str().is_some_and(|k| LAYOUT_KEYS.contains(&k));
            if !is_layout && parsed.get(key) != original.get(key) {
                anyhow::bail!("refusing to save: would change key {key:?}");
            }
        }
    }
    Ok(out)
}

/// Keep an untracked project config out of `git status` via `.git/info/exclude`.
fn exclude_if_untracked(repo_root: &Path, config_path: &Path) -> Result<()> {
    let Some(file_name) = config_path.file_name().and_then(|n| n.to_str()) else {
        return Ok(());
    };
    let tracked = Cmd::new("git")
        .workdir(repo_root)
        .args(&["ls-files", "--error-unmatch", file_name])
        .run_as_check()
        .unwrap_or(false);
    if tracked {
        return Ok(());
    }
    let common_dir = Cmd::new("git")
        .workdir(repo_root)
        .args(&["rev-parse", "--git-common-dir"])
        .run_and_capture_stdout()?;
    let common_dir = PathBuf::from(common_dir.trim());
    let common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        repo_root.join(common_dir)
    };
    let exclude = common_dir.join("info").join("exclude");
    let pattern = format!("/{file_name}");
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == pattern) {
        return Ok(());
    }
    if let Some(parent) = exclude.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&pattern);
    content.push('\n');
    std::fs::write(&exclude, content).with_context(|| format!("write {}", exclude.display()))
}

impl SessionConfigEditor {
    pub fn load(project_name: String, repo_root: PathBuf) -> Result<Self> {
        let config_path = config_path_for(&repo_root);
        let doc = if config_path.exists() {
            let text = std::fs::read_to_string(&config_path)
                .with_context(|| format!("read {}", config_path.display()))?;
            serde_yaml::from_str(&text).unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        let (tabs, focus) = tabs_from_yaml(&doc);
        Ok(Self {
            project_name,
            repo_root,
            config_path,
            tabs,
            cursor: 0,
            focus,
            edit: None,
            dirty: false,
        })
    }

    pub fn save(&self) -> Result<()> {
        let text = if self.config_path.exists() {
            std::fs::read_to_string(&self.config_path)?
        } else {
            String::new()
        };
        let updated = render_config_text(&text, &self.tabs, self.focus)?;
        std::fs::write(&self.config_path, updated)
            .with_context(|| format!("write {}", self.config_path.display()))?;
        exclude_if_untracked(&self.repo_root, &self.config_path)
    }

    pub fn move_cursor(&mut self, down: bool) {
        if down {
            if self.cursor + 1 < self.tabs.len() {
                self.cursor += 1;
            }
        } else {
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    /// Swap the tab under the cursor with its neighbour, keeping focus attached.
    pub fn move_tab(&mut self, down: bool) {
        let from = self.cursor;
        let to = if down {
            if from + 1 >= self.tabs.len() {
                return;
            }
            from + 1
        } else {
            let Some(to) = from.checked_sub(1) else {
                return;
            };
            to
        };
        self.tabs.swap(from, to);
        if self.focus == from {
            self.focus = to;
        } else if self.focus == to {
            self.focus = from;
        }
        self.cursor = to;
        self.dirty = true;
    }

    pub fn add_tab(&mut self) {
        let at = (self.cursor + 1).min(self.tabs.len());
        self.tabs.insert(at, SessionTab::new("", ""));
        if self.focus >= at && self.tabs.len() > 1 {
            self.focus += 1;
        }
        self.cursor = at;
        self.dirty = true;
        self.start_edit(SessionTabField::Name);
    }

    pub fn delete_tab(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        let at = self.cursor;
        self.tabs.remove(at);
        if self.focus > at || (self.focus == at && self.focus >= self.tabs.len()) {
            self.focus = self.focus.saturating_sub(1);
        }
        self.cursor = self.cursor.min(self.tabs.len() - 1);
        self.dirty = true;
    }

    pub fn set_focus(&mut self) {
        if self.focus != self.cursor {
            self.focus = self.cursor;
            self.dirty = true;
        }
    }

    pub fn start_edit(&mut self, field: SessionTabField) {
        let Some(tab) = self.tabs.get(self.cursor) else {
            return;
        };
        let buffer = match field {
            SessionTabField::Name => tab.name.clone(),
            SessionTabField::Command => tab.command.clone(),
        };
        self.edit = Some(SessionTabEdit { field, buffer });
    }

    /// Commit the edit. Confirming a name moves straight on to the command.
    pub fn commit_edit(&mut self) {
        let Some(edit) = self.edit.take() else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(self.cursor) else {
            return;
        };
        let target = match edit.field {
            SessionTabField::Name => &mut tab.name,
            SessionTabField::Command => &mut tab.command,
        };
        if *target != edit.buffer {
            *target = edit.buffer;
            self.dirty = true;
        }
        if edit.field == SessionTabField::Name && tab.command.is_empty() {
            self.start_edit(SessionTabField::Command);
        }
    }
}

impl App {
    /// Open the session tab editor for the project shown in the Worktrees tab.
    pub fn show_session_config(&mut self) {
        let Some(main) = self.worktrees.iter().find(|w| w.is_main) else {
            self.status_message =
                Some(("No project loaded".to_string(), std::time::Instant::now()));
            return;
        };
        let repo_root = main.path.clone();
        let project_name = super::super::agent::extract_project_name(&repo_root);
        match SessionConfigEditor::load(project_name, repo_root) {
            Ok(editor) => self.pending_session_config = Some(editor),
            Err(e) => {
                self.status_message = Some((
                    format!("Failed to load config: {e}"),
                    std::time::Instant::now(),
                ))
            }
        }
    }

    pub fn save_session_config(&mut self) {
        let Some(editor) = self.pending_session_config.take() else {
            return;
        };
        let message = match editor.save() {
            Ok(()) => format!(
                "Saved {} tabs to {} (applies to newly opened worktrees)",
                editor.tabs.len(),
                editor.config_path.display()
            ),
            Err(e) => {
                let msg = format!("Failed to save config: {e}");
                self.pending_session_config = Some(editor);
                msg
            }
        };
        self.status_message = Some((message, std::time::Instant::now()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(s: &str) -> Value {
        serde_yaml::from_str(s).unwrap()
    }

    fn editor(tabs: Vec<SessionTab>) -> SessionConfigEditor {
        SessionConfigEditor {
            project_name: "p".into(),
            repo_root: PathBuf::from("/tmp"),
            config_path: PathBuf::from("/tmp/.workmux.yaml"),
            tabs,
            cursor: 0,
            focus: 0,
            edit: None,
            dirty: false,
        }
    }

    #[test]
    fn empty_config_defaults_to_agent_and_shell() {
        let (tabs, focus) = tabs_from_yaml(&Value::Null);
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].command, "<agent>");
        assert_eq!(tabs[1].command, "");
        assert_eq!(focus, 0);
    }

    #[test]
    fn panes_become_one_tab_each() {
        let doc = yaml(
            "panes:\n  - command: <agent>\n  - command: npm run dev\n    split: horizontal\n    focus: true\n",
        );
        let (tabs, focus) = tabs_from_yaml(&doc);
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].name, "agent");
        assert_eq!(tabs[1].command, "npm run dev");
        assert_eq!(focus, 1);
    }

    #[test]
    fn windows_roundtrip_keeps_extra_panes_and_other_keys() {
        let doc = yaml(
            "agent: claude\nwindows:\n  - name: dev\n    panes:\n      - command: npm run dev\n      - command: tail -f log\n        split: vertical\n",
        );
        let (tabs, focus) = tabs_from_yaml(&doc);
        assert_eq!(tabs[0].extra_panes.len(), 1);
        let out = apply_tabs_to_yaml(doc, &tabs, focus);
        assert_eq!(out["agent"], Value::from("claude"));
        assert_eq!(out["mode"], Value::from("session"));
        let panes = out["windows"][0]["panes"].as_sequence().unwrap();
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[1]["command"], Value::from("tail -f log"));
        assert_eq!(panes[0]["focus"], Value::from(true));
    }

    #[test]
    fn apply_removes_panes_and_omits_blank_fields() {
        let doc = yaml("panes:\n  - command: <agent>\n");
        let tabs = vec![
            SessionTab::new("agent", "<agent>"),
            SessionTab::new("", "  "),
        ];
        let out = apply_tabs_to_yaml(doc, &tabs, 1);
        assert!(out.get("panes").is_none());
        let second = &out["windows"][1];
        assert!(second.get("name").is_none());
        assert!(second["panes"][0].get("command").is_none());
        assert_eq!(second["panes"][0]["focus"], Value::from(true));
    }

    #[test]
    fn render_text_keeps_comments_and_order() {
        let text = "# header\npanes:\n  - command: <agent> # agent\n    focus: true\n  - split: horizontal\n\n# Hooks below\nfiles:\n  symlink:\n    - CLAUDE.md # shared\n\npre_merge:\n  - just check\nmode: window\n";
        let tabs = vec![
            SessionTab::new("agent", "<agent>"),
            SessionTab::new("dev", "npm run dev"),
        ];
        let out = render_config_text(text, &tabs, 0).unwrap();
        assert!(out.starts_with("# header\n"));
        assert!(out.contains("# Hooks below\nfiles:\n  symlink:\n    - CLAUDE.md # shared\n"));
        assert!(out.contains("pre_merge:\n  - just check"));
        assert!(!out.contains("panes:\n  - command: <agent> # agent"));
        assert!(!out.contains("mode: window"));
        let parsed: Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(parsed["mode"], Value::from("session"));
        assert_eq!(parsed["windows"][1]["name"], Value::from("dev"));
        assert!(parsed.get("panes").is_none());
    }

    #[test]
    fn render_text_from_empty_file() {
        let tabs = vec![SessionTab::new("agent", "<agent>")];
        let out = render_config_text("", &tabs, 0).unwrap();
        assert!(out.starts_with("mode: session\n"));
    }

    #[test]
    fn move_tab_carries_focus() {
        let mut ed = editor(vec![SessionTab::new("a", ""), SessionTab::new("b", "")]);
        ed.move_tab(true);
        assert_eq!(ed.tabs[1].name, "a");
        assert_eq!(ed.focus, 1);
        assert_eq!(ed.cursor, 1);
        ed.move_tab(true);
        assert_eq!(ed.cursor, 1);
    }

    #[test]
    fn delete_keeps_last_tab_and_fixes_focus() {
        let mut ed = editor(vec![SessionTab::new("a", ""), SessionTab::new("b", "")]);
        ed.focus = 1;
        ed.cursor = 0;
        ed.delete_tab();
        assert_eq!(ed.tabs.len(), 1);
        assert_eq!(ed.focus, 0);
        ed.delete_tab();
        assert_eq!(ed.tabs.len(), 1);
    }

    #[test]
    fn add_then_name_flows_into_command_edit() {
        let mut ed = editor(vec![SessionTab::new("a", "x")]);
        ed.add_tab();
        assert_eq!(ed.cursor, 1);
        ed.edit.as_mut().unwrap().buffer = "dev".into();
        ed.commit_edit();
        assert_eq!(ed.tabs[1].name, "dev");
        assert_eq!(ed.edit.as_ref().unwrap().field, SessionTabField::Command);
        ed.edit.as_mut().unwrap().buffer = "npm run dev".into();
        ed.commit_edit();
        assert_eq!(ed.tabs[1].command, "npm run dev");
        assert!(ed.edit.is_none());
        assert!(ed.dirty);
    }
}
