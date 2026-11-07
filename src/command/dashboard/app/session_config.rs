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

pub use crate::workflow::worktree_dir::WorktreeDirSource;
use crate::workflow::worktree_dir::{
    EffectiveWorktreeDir, MoveCandidate, MovePlan, default_worktree_dir, format_worktree_dir,
    resolve_worktree_dir,
};

/// Worktree directory setting shown in the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeDirSetting {
    /// Value currently in the project config file, if any.
    pub configured: Option<String>,
    /// Value to save. Empty means "remove the key".
    pub value: String,
    /// Directory `workmux add` uses right now, and why.
    pub effective: EffectiveWorktreeDir,
    /// `effective.dir`, display form.
    pub effective_display: String,
    /// Default directory, display form.
    pub default_display: String,
}

impl WorktreeDirSetting {
    /// Value the config should hold after saving (`None` = key removed).
    pub fn target(&self) -> Option<&str> {
        Some(self.value.trim()).filter(|v| !v.is_empty())
    }

    /// Whether saving would change the `worktree_dir` key.
    pub fn needs_write(&self) -> bool {
        self.target() != self.configured.as_deref()
    }

    pub fn source(&self) -> WorktreeDirSource {
        self.effective.source
    }
}

/// Build the worktree dir setting from the project config doc, the global
/// `worktree_dir` and the worktree paths. Uses the same resolution as
/// `workmux add`. A detected dir is prefilled as the value to save.
pub fn worktree_dir_setting(
    doc: &Value,
    global: Option<&str>,
    repo_root: &Path,
    worktrees: &[PathBuf],
    home: Option<&Path>,
) -> WorktreeDirSetting {
    let default_dir = default_worktree_dir(repo_root);
    let default_display = format_worktree_dir(&default_dir, repo_root, home);
    let configured = doc
        .get("worktree_dir")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let effective = resolve_worktree_dir(
        configured.as_deref(),
        global,
        repo_root,
        || worktrees.to_vec(),
        home,
    )
    .unwrap_or(EffectiveWorktreeDir {
        dir: default_dir,
        source: WorktreeDirSource::Default,
    });
    let effective_display = format_worktree_dir(&effective.dir, repo_root, home);
    let value = match (&configured, effective.source) {
        (Some(v), _) => v.clone(),
        (None, WorktreeDirSource::Detected) => effective_display.clone(),
        _ => String::new(),
    };
    WorktreeDirSetting {
        configured,
        value,
        effective,
        effective_display,
        default_display,
    }
}

/// Set or remove the top-level `worktree_dir` key in config text, keeping
/// every other line byte for byte. A new key replaces the commented example
/// line from `workmux init` when present, else is appended.
pub fn render_worktree_dir_text(text: &str, value: Option<&str>) -> Result<String> {
    let doc: Value = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_yaml::from_str(text).context("parse project config")?
    };
    if !matches!(doc, Value::Mapping(_) | Value::Null) {
        anyhow::bail!("project config is not a YAML mapping");
    }
    let new_line = value.map(|v| format!("worktree_dir: {}", crate::config::yaml_scalar(v)));
    let example = crate::config::WORKTREE_DIR_EXAMPLE_LINE.trim_end();

    let mut out: Vec<String> = Vec::new();
    let mut placed = false;
    let mut skipping = false;
    for line in text.lines() {
        if let Some(key) = top_level_key(line) {
            skipping = key == "worktree_dir";
            if skipping {
                if let Some(nl) = new_line.as_ref().filter(|_| !placed) {
                    out.push(nl.clone());
                    placed = true;
                }
                continue;
            }
        } else if skipping && line.starts_with([' ', '\t']) {
            continue; // continuation of the old value
        } else {
            skipping = false;
        }
        out.push(line.to_string());
    }
    if !placed && let Some(nl) = new_line.as_ref() {
        if let Some(pos) = out.iter().position(|l| l == example) {
            out[pos] = nl.clone();
        } else {
            while out.last().is_some_and(|l| l.trim().is_empty()) {
                out.pop();
            }
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push(nl.clone());
        }
    }
    let mut result = out.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }

    // Guard: the result must parse and keep every other key.
    let parsed: Value = serde_yaml::from_str(&result).context("generated config is invalid")?;
    if let Value::Mapping(original) = &doc {
        for key in original.keys() {
            if key.as_str() != Some("worktree_dir") && parsed.get(key) != original.get(key) {
                anyhow::bail!("refusing to save: would change key {key:?}");
            }
        }
    }
    let written = parsed.get("worktree_dir").and_then(Value::as_str);
    if written != value {
        anyhow::bail!("refusing to save: worktree_dir did not round-trip");
    }
    Ok(result)
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
    /// Tabs changed since load.
    pub dirty: bool,
    pub worktree_dir: WorktreeDirSetting,
    /// Inline edit buffer for the worktree directory.
    pub dir_edit: Option<String>,
    /// Inline edit buffer for the "move worktrees to" directory.
    pub move_edit: Option<String>,
    /// Move waiting for confirmation.
    pub move_plan: Option<MovePlan>,
    /// Page shown (Tab cycles).
    pub page: super::ConfigPage,
    pub tools_page: super::ToolsPage,
    pub claude_page: super::ClaudePage,
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
        let (tools, claude) = super::pages_from_yaml(&doc);
        // Read-only: listing worktrees never writes anything.
        let worktrees = crate::workflow::worktree_dir::list_worktree_paths(&repo_root);
        let global = crate::config::Config::global_worktree_dir();
        let worktree_dir = worktree_dir_setting(
            &doc,
            global.as_deref(),
            &repo_root,
            &worktrees,
            home::home_dir().as_deref(),
        );
        Ok(Self {
            project_name,
            repo_root,
            config_path,
            tabs,
            cursor: 0,
            focus,
            edit: None,
            dirty: false,
            worktree_dir,
            dir_edit: None,
            move_edit: None,
            move_plan: None,
            page: super::ConfigPage::Layout,
            tools_page: super::ToolsPage::new(tools),
            claude_page: super::ClaudePage::new(claude),
        })
    }

    /// Whether save rewrites the tab layout. Untouched tabs are written only
    /// when there is nothing else to save (keeps the old `s` behaviour).
    pub fn writes_tabs(&self) -> bool {
        self.dirty || !self.has_other_changes()
    }

    /// Changes besides the tab layout (worktree dir, tools, Claude settings).
    pub fn has_other_changes(&self) -> bool {
        self.worktree_dir.needs_write() || self.tools_page.dirty || self.claude_page.dirty
    }

    /// Any unsaved change.
    pub fn is_dirty(&self) -> bool {
        self.dirty || self.has_other_changes()
    }

    pub fn save(&self) -> Result<()> {
        let mut updated = if self.config_path.exists() {
            std::fs::read_to_string(&self.config_path)?
        } else {
            String::new()
        };
        if self.worktree_dir.needs_write() {
            if let Some(dir) = self.worktree_dir.target() {
                crate::util::expand_worktree_dir(dir, &self.repo_root)?;
            }
            updated = render_worktree_dir_text(&updated, self.worktree_dir.target())?;
        }
        if self.tools_page.dirty {
            updated = super::render_top_level_key(
                &updated,
                "tools",
                self.tools_page.to_value()?.as_ref(),
            )?;
        }
        if self.claude_page.dirty {
            updated = super::render_top_level_key(
                &updated,
                "claude",
                self.claude_page.to_value()?.as_ref(),
            )?;
        }
        if self.writes_tabs() {
            updated = render_config_text(&updated, &self.tabs, self.focus)?;
        }
        std::fs::write(&self.config_path, updated)
            .with_context(|| format!("write {}", self.config_path.display()))?;
        exclude_if_untracked(&self.repo_root, &self.config_path)
    }

    pub fn start_dir_edit(&mut self) {
        self.dir_edit = Some(self.worktree_dir.value.clone());
    }

    pub fn commit_dir_edit(&mut self) {
        if let Some(buffer) = self.dir_edit.take() {
            self.worktree_dir.value = buffer.trim().to_string();
        }
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
            Ok(()) => {
                let mut parts = Vec::new();
                if editor.worktree_dir.needs_write() {
                    parts.push(match editor.worktree_dir.target() {
                        Some(dir) => format!("worktree_dir: {dir}"),
                        None => "worktree_dir: default".to_string(),
                    });
                }
                if editor.tools_page.dirty {
                    parts.push(format!("tools ({})", editor.tools_page.tools.items.len()));
                }
                if editor.claude_page.dirty {
                    parts.push("claude settings".to_string());
                }
                if editor.writes_tabs() {
                    parts.push(format!("{} tabs", editor.tabs.len()));
                }
                format!(
                    "Saved {} to {} (applies to new worktrees only)",
                    parts.join(", "),
                    editor.config_path.display()
                )
            }
            Err(e) => {
                let msg = format!("Failed to save config: {e}");
                self.pending_session_config = Some(editor);
                msg
            }
        };
        self.status_message = Some((message, std::time::Instant::now()));
    }
}

/// Write only the `worktree_dir` key of a project config file.
fn write_worktree_dir(config_path: &Path, repo_root: &Path, value: &str) -> Result<()> {
    let text = if config_path.exists() {
        std::fs::read_to_string(config_path)?
    } else {
        String::new()
    };
    let updated = render_worktree_dir_text(&text, Some(value))?;
    std::fs::write(config_path, updated)
        .with_context(|| format!("write {}", config_path.display()))?;
    exclude_if_untracked(repo_root, config_path)
}

impl App {
    /// Start typing the directory to move existing worktrees to.
    pub fn start_worktree_move(&mut self) {
        if let Some(editor) = self.pending_session_config.as_mut() {
            editor.move_edit = Some(String::new());
        }
    }

    /// Turn the typed directory into a move plan awaiting confirmation.
    pub fn plan_worktree_move(&mut self) {
        let Some(editor) = self.pending_session_config.as_mut() else {
            return;
        };
        let Some(buffer) = editor.move_edit.take() else {
            return;
        };
        let template = buffer.trim().to_string();
        if template.is_empty() {
            return;
        }
        let root = editor.repo_root.clone();
        let result = crate::util::expand_worktree_dir(&template, &root).and_then(|to_dir| {
            let locked = crate::workflow::worktree_dir::list_locked_worktrees(&root);
            let candidates: Vec<MoveCandidate> =
                crate::workflow::worktree_dir::list_worktree_paths(&root)
                    .into_iter()
                    .map(|path| {
                        let busy = self
                            .worktrees
                            .iter()
                            .find(|w| w.path == path)
                            .is_some_and(|w| w.has_mux_window || w.agent_status.is_some());
                        MoveCandidate {
                            locked: locked.contains(&path),
                            busy,
                            path,
                        }
                    })
                    .collect();
            crate::workflow::worktree_dir::plan_worktree_move(
                &root,
                &template,
                &editor.worktree_dir.effective.dir,
                &to_dir,
                &candidates,
                |p| p.exists(),
            )
        });
        match result {
            Ok(plan) => editor.move_plan = Some(plan),
            Err(e) => {
                self.status_message =
                    Some((format!("Move worktrees: {e:#}"), std::time::Instant::now()))
            }
        }
    }

    /// Run the confirmed move in the background, then save `worktree_dir`.
    pub fn confirm_worktree_move(&mut self) {
        let Some(editor) = self.pending_session_config.as_mut() else {
            return;
        };
        let Some(plan) = editor.move_plan.take() else {
            return;
        };
        if plan.blocked() {
            self.status_message = Some((
                "Move aborted: some worktrees cannot be moved (see list)".to_string(),
                std::time::Instant::now(),
            ));
            return;
        }
        let Some(editor) = self.pending_session_config.take() else {
            return;
        };
        let moves = plan.moves();
        self.status_message = Some((
            format!("Moving {} worktree(s)...", moves.len()),
            std::time::Instant::now(),
        ));
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let root = editor.repo_root;
            let result = match crate::workflow::worktree_dir::execute_moves(&root, &moves) {
                Ok(n) => write_worktree_dir(&editor.config_path, &root, &plan.template)
                    .map(|()| {
                        format!(
                            "Moved {n} worktree(s) to {}; worktree_dir: {}",
                            plan.to_dir.display(),
                            plan.template
                        )
                    })
                    .map_err(|e| format!("moved {n} worktree(s) but saving config failed: {e:#}")),
                Err((done, e)) => Err(format!(
                    "{e} ({done} of {} moved; config not changed)",
                    moves.len()
                )),
            };
            let _ = tx.send(super::types::AppEvent::WorktreeMoveResult(result));
        });
    }

    pub(super) fn handle_worktree_move_result(&mut self, result: Result<String, String>) {
        let msg = match result {
            Ok(m) => m,
            Err(e) => format!("Move worktrees failed: {e}"),
        };
        self.status_message = Some((msg, std::time::Instant::now()));
        self.trigger_worktree_refetch();
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
            worktree_dir: WorktreeDirSetting {
                configured: None,
                value: String::new(),
                effective: EffectiveWorktreeDir {
                    dir: PathBuf::from("/p__worktrees"),
                    source: WorktreeDirSource::Default,
                },
                effective_display: "/p__worktrees".into(),
                default_display: "/p__worktrees".into(),
            },
            dir_edit: None,
            move_edit: None,
            move_plan: None,
            page: crate::command::dashboard::app::ConfigPage::Layout,
            tools_page: Default::default(),
            claude_page: Default::default(),
        }
    }

    #[test]
    fn tools_or_claude_change_alone_does_not_rewrite_tabs() {
        let mut e = editor(vec![SessionTab::new("agent", "<agent>")]);
        assert!(e.writes_tabs());
        e.tools_page.dirty = true;
        assert!(!e.writes_tabs());
        assert!(e.is_dirty());
        e.tools_page.dirty = false;
        e.claude_page.dirty = true;
        assert!(!e.writes_tabs());
        e.dirty = true;
        assert!(e.writes_tabs());
    }

    fn paths(v: &[&str]) -> Vec<PathBuf> {
        v.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn setting_shows_effective_dir_with_source() {
        let root = Path::new("/r/app");
        let wts = paths(&["/r/app", "/r/app/.claude/worktrees/a"]);
        let s = worktree_dir_setting(&yaml("worktree_dir: ../w\n"), Some("/g"), root, &wts, None);
        assert_eq!(s.source(), WorktreeDirSource::Project);
        assert_eq!(s.effective_display, "/r/w");
        assert!(!s.needs_write());
        let s = worktree_dir_setting(&Value::Null, Some("/g"), root, &wts, None);
        assert_eq!(s.source(), WorktreeDirSource::Global);
        assert_eq!(s.effective_display, "/g");
        assert_eq!(s.value, "");
        assert!(!s.needs_write());
        let s = worktree_dir_setting(&Value::Null, None, root, &wts, None);
        assert_eq!(s.source(), WorktreeDirSource::Detected);
        // Leading dot kept: `.claude/worktrees`, never `./claude/worktrees`.
        assert_eq!(s.value, ".claude/worktrees");
        assert_eq!(s.effective_display, ".claude/worktrees");
        assert!(s.needs_write());
        let s = worktree_dir_setting(&Value::Null, None, root, &paths(&["/r/app"]), None);
        assert_eq!(s.source(), WorktreeDirSource::Default);
        assert_eq!(s.effective_display, "/r/app__worktrees");
        assert!(!s.needs_write());
    }

    #[test]
    fn render_dir_replaces_existing_keeps_comments() {
        let text = "# head\nagent: claude # c\nworktree_dir: old\nfiles:\n  copy:\n    - .env\n";
        let out = render_worktree_dir_text(text, Some("{project}-wt")).unwrap();
        assert_eq!(
            out,
            "# head\nagent: claude # c\nworktree_dir: '{project}-wt'\nfiles:\n  copy:\n    - .env\n"
        );
        let out = render_worktree_dir_text(text, None).unwrap();
        assert_eq!(
            out,
            "# head\nagent: claude # c\nfiles:\n  copy:\n    - .env\n"
        );
    }

    #[test]
    fn render_dir_uses_init_example_line_or_appends() {
        let init = crate::config::init_config_text(None);
        let out = render_worktree_dir_text(&init, Some(".worktrees")).unwrap();
        assert_eq!(out, crate::config::init_config_text(Some(".worktrees")));
        assert_eq!(
            render_worktree_dir_text("agent: x\n", Some("~/w")).unwrap(),
            "agent: x\n\nworktree_dir: ~/w\n"
        );
        assert_eq!(
            render_worktree_dir_text("", Some("a b")).unwrap(),
            "worktree_dir: a b\n"
        );
    }

    #[test]
    fn save_only_dir_change_skips_tabs() {
        let mut ed = editor(vec![SessionTab::new("a", "")]);
        assert!(ed.writes_tabs());
        ed.start_dir_edit();
        ed.dir_edit = Some(" .wt ".into());
        ed.commit_dir_edit();
        assert_eq!(ed.worktree_dir.value, ".wt");
        assert!(ed.worktree_dir.needs_write());
        assert!(!ed.writes_tabs());
        ed.dirty = true;
        assert!(ed.writes_tabs());
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
