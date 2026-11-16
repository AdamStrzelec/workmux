//! State of the settings menu overlay: edits scalar options of the global
//! `config.yaml` or the project `.workmux.yaml` in place (comments kept).

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::settings_menu::{self, COMPLEX, Kind, OPTIONS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsScope {
    Global,
    Project,
}

impl SettingsScope {
    pub fn label(self) -> &'static str {
        match self {
            SettingsScope::Global => "global",
            SettingsScope::Project => "project",
        }
    }
}

pub struct SettingsOverlay {
    pub scope: SettingsScope,
    pub repo_root: PathBuf,
    pub global_path: PathBuf,
    pub project_path: PathBuf,
    /// Text of the file of the current scope.
    pub text: String,
    pub cursor: usize,
    /// Typing a text value for the option under the cursor.
    pub edit: Option<String>,
    /// Last result, shown at the bottom.
    pub message: Option<String>,
}

impl SettingsOverlay {
    pub fn load(repo_root: PathBuf) -> Result<Self> {
        let global_path = crate::config::global_config_path()
            .context("Could not determine the global config path")?;
        let project_path = super::session_config::config_path_for(&repo_root);
        let mut s = Self {
            scope: SettingsScope::Global,
            repo_root,
            global_path,
            project_path,
            text: String::new(),
            cursor: 0,
            edit: None,
            message: None,
        };
        s.reload();
        Ok(s)
    }

    pub fn path(&self) -> &PathBuf {
        match self.scope {
            SettingsScope::Global => &self.global_path,
            SettingsScope::Project => &self.project_path,
        }
    }

    /// Re-read the file of the current scope (also after an editor closed).
    pub fn reload(&mut self) {
        self.text = std::fs::read_to_string(self.path()).unwrap_or_default();
    }

    pub fn row_count() -> usize {
        OPTIONS.len() + COMPLEX.len()
    }

    pub fn move_cursor(&mut self, down: bool) {
        let max = Self::row_count().saturating_sub(1);
        self.cursor = if down {
            (self.cursor + 1).min(max)
        } else {
            self.cursor.saturating_sub(1)
        };
    }

    pub fn toggle_scope(&mut self) {
        self.scope = match self.scope {
            SettingsScope::Global => SettingsScope::Project,
            SettingsScope::Project => SettingsScope::Global,
        };
        self.edit = None;
        self.message = None;
        self.reload();
    }

    /// Key of the row under the cursor and whether it is a scalar option.
    pub fn current_key(&self) -> &'static str {
        match OPTIONS.get(self.cursor) {
            Some(o) => o.key,
            None => COMPLEX[self.cursor - OPTIONS.len()].0,
        }
    }

    /// Value of option `i` in the current scope, if set there.
    pub fn value_of(&self, i: usize) -> Option<String> {
        settings_menu::current_value(&self.text, OPTIONS[i].key)
    }

    fn write(&mut self, text: String) -> Result<()> {
        settings_menu::validate_config(&text)?;
        let path = self.path().clone();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        std::fs::write(&tmp, &text)?;
        std::fs::rename(&tmp, &path).with_context(|| format!("write {}", path.display()))?;
        if self.scope == SettingsScope::Project {
            let _ = super::session_config::exclude_if_untracked(&self.repo_root, &path);
        }
        self.text = text;
        Ok(())
    }

    fn apply(&mut self, i: usize, value: Option<&str>) {
        self.reload();
        let opt = &OPTIONS[i];
        let result = settings_menu::render_set(&self.text, opt, value).and_then(|t| self.write(t));
        self.message = Some(match result {
            Ok(()) => match value {
                Some(v) => format!("{} = {v} ({})", opt.key, self.scope.label()),
                None => format!("{} reset ({})", opt.key, self.scope.label()),
            },
            Err(e) => format!("Not saved: {e}"),
        });
    }

    /// Enter: toggle a bool, cycle a choice, start typing a text value.
    pub fn activate(&mut self) {
        let Some(opt) = OPTIONS.get(self.cursor) else {
            self.message = Some("Nested option: press e to edit it in the editor".into());
            return;
        };
        let i = self.cursor;
        let current = self.value_of(i);
        match opt.kind {
            Kind::Bool => {
                let now = current.as_deref().unwrap_or(opt.default) == "true";
                self.apply(i, Some(if now { "false" } else { "true" }));
            }
            Kind::Choice(list) => {
                let cur = current.as_deref().unwrap_or(opt.default);
                let next = list
                    .iter()
                    .position(|v| *v == cur)
                    .map_or(0, |p| (p + 1) % list.len());
                self.apply(i, Some(list[next]));
            }
            Kind::Text => self.edit = Some(current.unwrap_or_default()),
        }
    }

    pub fn commit_edit(&mut self) {
        if let Some(buffer) = self.edit.take() {
            let value = buffer.trim().to_string();
            if value.is_empty() {
                self.message = Some("Empty value: use r to reset instead".into());
            } else {
                self.apply(self.cursor, Some(&value));
            }
        }
    }

    pub fn reset(&mut self) {
        if self.cursor < OPTIONS.len() {
            self.apply(self.cursor, None);
        }
    }

    /// Open the current scope's file at the key under the cursor in a tmux
    /// popup running `$VISUAL`/`$EDITOR`. The overlay reloads on next key.
    pub fn open_in_editor(&mut self) {
        let path = self.path().clone();
        if !path.exists() {
            let made = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|_| std::fs::write(&path, ""));
            if let Err(e) = made {
                self.message = Some(format!("Cannot create {}: {e}", path.display()));
                return;
            }
        }
        let line = settings_menu::find_key_line(&self.text, self.current_key()).unwrap_or(1);
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_string());
        let cmd = format!(
            "{editor} +{line} {}",
            crate::shell::shell_quote(&path.to_string_lossy())
        );
        let started = std::process::Command::new("tmux")
            .args(["display-popup", "-E", "-w", "90%", "-h", "90%", &cmd])
            .spawn();
        self.message = Some(match started {
            Ok(_) => format!("Editing {} (changes load on next key)", path.display()),
            Err(e) => format!("Cannot open editor popup (needs tmux): {e}"),
        });
    }
}

impl super::App {
    /// Open the settings menu for the project shown in the Worktrees tab
    /// (falls back to the current directory when none is loaded).
    pub fn show_settings(&mut self) {
        let root = self
            .worktrees
            .iter()
            .find(|w| w.is_main)
            .map(|w| w.path.clone())
            .or_else(|| std::env::current_dir().ok());
        let Some(root) = root else {
            self.status_message = Some((
                "No project directory".to_string(),
                std::time::Instant::now(),
            ));
            return;
        };
        match SettingsOverlay::load(root) {
            Ok(o) => self.pending_settings = Some(o),
            Err(e) => {
                self.status_message = Some((
                    format!("Failed to open settings: {e}"),
                    std::time::Instant::now(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlay(dir: &std::path::Path) -> SettingsOverlay {
        SettingsOverlay {
            scope: SettingsScope::Project,
            repo_root: dir.to_path_buf(),
            global_path: dir.join("global.yaml"),
            project_path: dir.join(".workmux.yaml"),
            text: String::new(),
            cursor: 0,
            edit: None,
            message: None,
        }
    }

    fn row(key: &str) -> usize {
        OPTIONS.iter().position(|o| o.key == key).unwrap()
    }

    #[test]
    fn cycles_choice_toggles_bool_and_resets() {
        let dir = tempfile::tempdir().unwrap();
        let mut o = overlay(dir.path());
        std::fs::write(&o.project_path, "# merge_strategy: rebase\nagent: claude\n").unwrap();
        o.cursor = row("merge_strategy");
        o.activate(); // default merge -> rebase
        assert_eq!(o.value_of(o.cursor).as_deref(), Some("rebase"));
        o.activate();
        assert_eq!(o.value_of(o.cursor).as_deref(), Some("squash"));
        o.cursor = row("merge_keep");
        o.activate();
        assert_eq!(o.value_of(o.cursor).as_deref(), Some("true"));
        o.reset();
        assert_eq!(o.value_of(o.cursor), None);
        let disk = std::fs::read_to_string(&o.project_path).unwrap();
        assert!(disk.contains("merge_strategy: squash\n"));
        assert!(disk.contains("agent: claude\n"));
    }

    #[test]
    fn text_edit_saves_and_invalid_file_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut o = overlay(dir.path());
        o.cursor = row("window_prefix");
        o.activate();
        o.edit.as_mut().unwrap().push_str("x-");
        o.commit_edit();
        assert!(
            std::fs::read_to_string(&o.project_path)
                .unwrap()
                .contains("window_prefix: x-")
        );
        std::fs::write(&o.project_path, "agent: [broken\n").unwrap();
        o.cursor = row("agent");
        o.edit = Some("codex".into());
        o.commit_edit();
        assert!(o.message.as_deref().unwrap().starts_with("Not saved"));
        assert_eq!(
            std::fs::read_to_string(&o.project_path).unwrap(),
            "agent: [broken\n"
        );
    }

    #[test]
    fn nested_row_is_not_changed_by_enter() {
        let dir = tempfile::tempdir().unwrap();
        let mut o = overlay(dir.path());
        o.cursor = OPTIONS.len();
        o.activate();
        assert!(!o.project_path.exists());
        assert!(o.message.is_some());
    }
}
