//! Extra pages of the Project Config editor (`C`): run tools and Claude
//! launch settings. Both are saved as whole top-level YAML keys (`tools:`,
//! `claude:`), leaving every other line of the file untouched.

use anyhow::{Context, Result};
use serde_yaml::Value;

use crate::claude_launch::{
    ClaudeConfig, ClaudePreset, MODELS, PERMISSION_MODES, ToolConfig, ToolsConfig,
};

/// Page shown by the Project Config editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfigPage {
    #[default]
    Layout,
    Tools,
    Claude,
}

impl ConfigPage {
    pub const ALL: [ConfigPage; 3] = [ConfigPage::Layout, ConfigPage::Tools, ConfigPage::Claude];

    pub fn next(self) -> Self {
        match self {
            ConfigPage::Layout => ConfigPage::Tools,
            ConfigPage::Tools => ConfigPage::Claude,
            ConfigPage::Claude => ConfigPage::Layout,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConfigPage::Layout => "Layout",
            ConfigPage::Tools => "Tools",
            ConfigPage::Claude => "Claude",
        }
    }
}

/// Top-level key a column-0 line starts, if any.
fn top_level_key(line: &str) -> Option<&str> {
    if line.starts_with([' ', '\t', '#', '-']) || line.trim().is_empty() {
        return None;
    }
    line.split_once(':').map(|(key, _)| key.trim())
}

/// Replace (or remove, with `None`) one top-level key block in config text,
/// keeping every other line byte for byte. A new key is appended.
pub fn render_top_level_key(text: &str, key: &str, value: Option<&Value>) -> Result<String> {
    let doc: Value = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_yaml::from_str(text).context("parse project config")?
    };
    if !matches!(doc, Value::Mapping(_) | Value::Null) {
        anyhow::bail!("project config is not a YAML mapping");
    }
    let block = match value {
        Some(v) => {
            let mut map = serde_yaml::Mapping::new();
            map.insert(key.into(), v.clone());
            Some(serde_yaml::to_string(&Value::Mapping(map))?)
        }
        None => None,
    };

    let mut out: Vec<String> = Vec::new();
    let mut placed = false;
    let mut skipping = false;
    for line in text.lines() {
        if let Some(k) = top_level_key(line) {
            skipping = k == key;
            if skipping {
                if let Some(b) = block.as_ref().filter(|_| !placed) {
                    out.extend(b.lines().map(String::from));
                    placed = true;
                }
                continue;
            }
        } else if skipping && (line.starts_with([' ', '\t', '-']) || line.trim().is_empty()) {
            continue;
        } else {
            skipping = false;
        }
        out.push(line.to_string());
    }
    if !placed && let Some(b) = block.as_ref() {
        while out.last().is_some_and(|l| l.trim().is_empty()) {
            out.pop();
        }
        if !out.is_empty() {
            out.push(String::new());
        }
        out.extend(b.lines().map(String::from));
    }
    let mut result = out.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }

    // Guard: the result must parse, keep every other key, and hold the value.
    let parsed: Value = serde_yaml::from_str(&result).context("generated config is invalid")?;
    if let Value::Mapping(original) = &doc {
        for k in original.keys() {
            if k.as_str() != Some(key) && parsed.get(k) != original.get(k) {
                anyhow::bail!("refusing to save: would change key {k:?}");
            }
        }
    }
    if parsed.get(key) != value {
        anyhow::bail!("refusing to save: {key} did not round-trip");
    }
    Ok(result)
}

/// Read `tools:` and `claude:` from a config document (invalid = empty).
pub fn pages_from_yaml(doc: &Value) -> (ToolsConfig, ClaudeConfig) {
    let tools = doc
        .get("tools")
        .and_then(|v| serde_yaml::from_value(v.clone()).ok())
        .unwrap_or_default();
    let claude = doc
        .get("claude")
        .and_then(|v| serde_yaml::from_value(v.clone()).ok())
        .unwrap_or_default();
    (tools, claude)
}

/// Field of a tool row being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolField {
    Key,
    Name,
    Command,
}

/// Tools page state.
#[derive(Debug, Clone, Default)]
pub struct ToolsPage {
    pub tools: ToolsConfig,
    pub cursor: usize,
    pub edit: Option<(ToolField, String)>,
    /// Editing a freshly added row: walk key -> name -> command.
    pub adding: bool,
    pub dirty: bool,
}

impl ToolsPage {
    pub fn new(tools: ToolsConfig) -> Self {
        Self {
            tools,
            ..Default::default()
        }
    }

    pub fn move_cursor(&mut self, down: bool) {
        let len = self.tools.items.len();
        if down {
            if self.cursor + 1 < len {
                self.cursor += 1;
            }
        } else {
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    pub fn add(&mut self) {
        let at = (self.cursor + 1).min(self.tools.items.len());
        self.tools.items.insert(at, ToolConfig::default());
        self.cursor = at;
        self.dirty = true;
        self.adding = true;
        self.start_edit(ToolField::Key);
    }

    pub fn delete(&mut self) {
        if self.cursor < self.tools.items.len() {
            self.tools.items.remove(self.cursor);
            self.cursor = self.cursor.min(self.tools.items.len().saturating_sub(1));
            self.dirty = true;
        }
    }

    pub fn toggle_close_with_session(&mut self) {
        self.tools.close_with_session = Some(!self.tools.close_with_session());
        self.dirty = true;
    }

    pub fn start_edit(&mut self, field: ToolField) {
        let Some(t) = self.tools.items.get(self.cursor) else {
            return;
        };
        let buffer = match field {
            ToolField::Key => t.key.clone(),
            ToolField::Name => t.name.clone(),
            ToolField::Command => t.command.clone(),
        };
        self.edit = Some((field, buffer));
    }

    /// Commit the edit; returns an error message for an invalid key.
    pub fn commit_edit(&mut self) -> Option<String> {
        let (field, buffer) = self.edit.take()?;
        let value = buffer.trim().to_string();
        if field == ToolField::Key {
            if value.chars().count() != 1 {
                self.edit = Some((field, buffer));
                return Some("Tool key must be exactly one character".to_string());
            }
            let taken = self
                .tools
                .items
                .iter()
                .enumerate()
                .any(|(i, t)| i != self.cursor && t.key == value);
            if taken {
                self.edit = Some((field, buffer));
                return Some(format!("Key '{value}' is already used by another tool"));
            }
        }
        let t = self.tools.items.get_mut(self.cursor)?;
        let target = match field {
            ToolField::Key => &mut t.key,
            ToolField::Name => &mut t.name,
            ToolField::Command => &mut t.command,
        };
        if *target != value {
            *target = value;
            self.dirty = true;
        }
        if self.adding {
            match field {
                ToolField::Key => self.start_edit(ToolField::Name),
                ToolField::Name => self.start_edit(ToolField::Command),
                ToolField::Command => self.adding = false,
            }
        }
        None
    }

    pub fn cancel_edit(&mut self) {
        self.edit = None;
        self.adding = false;
    }

    /// Value to save (`None` = remove the key). Rows without a name or
    /// command are dropped.
    pub fn to_value(&self) -> Result<Option<Value>> {
        let mut tools = self.tools.clone();
        tools
            .items
            .retain(|t| !t.name.trim().is_empty() && !t.command.trim().is_empty());
        if tools.is_empty() {
            return Ok(None);
        }
        Ok(Some(serde_yaml::to_value(&tools)?))
    }
}

/// Rows of the Claude page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeField {
    Model,
    PermissionMode,
    Preset,
    AllowedTools,
    DisallowedTools,
    AddDirs,
    AppendSystemPrompt,
    ExtraArgs,
    ResumeOnReopen,
}

impl ClaudeField {
    pub const ALL: [ClaudeField; 9] = [
        ClaudeField::Model,
        ClaudeField::PermissionMode,
        ClaudeField::Preset,
        ClaudeField::AllowedTools,
        ClaudeField::DisallowedTools,
        ClaudeField::AddDirs,
        ClaudeField::AppendSystemPrompt,
        ClaudeField::ExtraArgs,
        ClaudeField::ResumeOnReopen,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ClaudeField::Model => "model",
            ClaudeField::PermissionMode => "permission_mode",
            ClaudeField::Preset => "preset",
            ClaudeField::AllowedTools => "allowed_tools",
            ClaudeField::DisallowedTools => "disallowed_tools",
            ClaudeField::AddDirs => "add_dirs",
            ClaudeField::AppendSystemPrompt => "append_system_prompt",
            ClaudeField::ExtraArgs => "extra_args",
            ClaudeField::ResumeOnReopen => "resume_on_reopen",
        }
    }

    /// Whether Enter opens a text edit.
    pub fn editable(self) -> bool {
        !matches!(self, ClaudeField::Preset | ClaudeField::ResumeOnReopen)
    }
}

fn split_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect()
}

fn opt(s: &str) -> Option<String> {
    Some(s.trim().to_string()).filter(|v| !v.is_empty())
}

/// Cycle through `values` then unset.
fn cycle(current: Option<&str>, values: &[&str]) -> Option<String> {
    match current.and_then(|c| values.iter().position(|v| *v == c)) {
        None => values.first().map(|v| v.to_string()),
        Some(i) if i + 1 < values.len() => Some(values[i + 1].to_string()),
        Some(_) => None,
    }
}

/// Display/edit text of a field.
pub fn claude_field_text(c: &ClaudeConfig, f: ClaudeField) -> String {
    match f {
        ClaudeField::Model => c.model.clone().unwrap_or_default(),
        ClaudeField::PermissionMode => c.permission_mode.clone().unwrap_or_default(),
        ClaudeField::Preset => c.preset().label().to_string(),
        ClaudeField::AllowedTools => c.allowed_tools.join(", "),
        ClaudeField::DisallowedTools => c.disallowed_tools.join(", "),
        ClaudeField::AddDirs => c.add_dirs.join(", "),
        ClaudeField::AppendSystemPrompt => c.append_system_prompt.clone().unwrap_or_default(),
        ClaudeField::ExtraArgs => crate::claude_launch::shell_join(&c.extra_args),
        ClaudeField::ResumeOnReopen => c.resume_on_reopen().to_string(),
    }
}

/// Apply typed text to a field. Lists are comma separated, extra args are
/// shell words.
pub fn set_claude_field(c: &mut ClaudeConfig, f: ClaudeField, text: &str) -> Result<(), String> {
    match f {
        ClaudeField::Model => c.model = opt(text),
        ClaudeField::PermissionMode => {
            let v = opt(text);
            if let Some(mode) = v.as_deref()
                && !PERMISSION_MODES.contains(&mode)
            {
                return Err(format!(
                    "permission_mode must be one of {}",
                    PERMISSION_MODES.join(", ")
                ));
            }
            c.permission_mode = v;
        }
        ClaudeField::Preset => {
            let v = text.trim();
            c.preset = match ClaudePreset::ALL.iter().find(|p| p.label() == v) {
                Some(ClaudePreset::None) => None,
                Some(p) => Some(*p),
                None => return Err(format!("unknown preset '{v}'")),
            };
        }
        ClaudeField::AllowedTools => c.allowed_tools = split_list(text),
        ClaudeField::DisallowedTools => c.disallowed_tools = split_list(text),
        ClaudeField::AddDirs => c.add_dirs = split_list(text),
        ClaudeField::AppendSystemPrompt => c.append_system_prompt = opt(text),
        ClaudeField::ExtraArgs => {
            c.extra_args = shlex::split(text).ok_or("extra_args: unbalanced quotes")?;
        }
        ClaudeField::ResumeOnReopen => {
            c.resume_on_reopen = match text.trim() {
                "true" => None,
                "false" => Some(false),
                other => return Err(format!("expected true/false, got '{other}'")),
            }
        }
    }
    Ok(())
}

/// Space on a field: cycle model/mode/preset, toggle resume.
pub fn cycle_claude_field(c: &mut ClaudeConfig, f: ClaudeField) {
    match f {
        ClaudeField::Model => c.model = cycle(c.model.as_deref(), &MODELS),
        ClaudeField::PermissionMode => {
            c.permission_mode = cycle(c.permission_mode.as_deref(), &PERMISSION_MODES)
        }
        ClaudeField::Preset => {
            let next = c.preset().next();
            c.preset = (next != ClaudePreset::None).then_some(next);
        }
        ClaudeField::ResumeOnReopen => {
            c.resume_on_reopen = if c.resume_on_reopen() {
                Some(false)
            } else {
                None
            }
        }
        _ => {}
    }
}

/// Claude page state.
#[derive(Debug, Clone, Default)]
pub struct ClaudePage {
    pub config: ClaudeConfig,
    pub cursor: usize,
    pub edit: Option<String>,
    pub dirty: bool,
}

impl ClaudePage {
    pub fn new(config: ClaudeConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    pub fn field(&self) -> ClaudeField {
        ClaudeField::ALL[self.cursor.min(ClaudeField::ALL.len() - 1)]
    }

    pub fn move_cursor(&mut self, down: bool) {
        if down {
            self.cursor = (self.cursor + 1).min(ClaudeField::ALL.len() - 1);
        } else {
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    pub fn cycle(&mut self) {
        let before = self.config.clone();
        let field = self.field();
        cycle_claude_field(&mut self.config, field);
        self.dirty |= before != self.config;
    }

    pub fn clear(&mut self) {
        let before = self.config.clone();
        let field = self.field();
        let _ = set_claude_field(&mut self.config, field, "");
        if self.field() == ClaudeField::Preset {
            self.config.preset = None;
        }
        if self.field() == ClaudeField::ResumeOnReopen {
            self.config.resume_on_reopen = None;
        }
        self.dirty |= before != self.config;
    }

    pub fn start_edit(&mut self) {
        if self.field().editable() {
            self.edit = Some(claude_field_text(&self.config, self.field()));
        } else {
            self.cycle();
        }
    }

    pub fn commit_edit(&mut self) -> Option<String> {
        let buffer = self.edit.take()?;
        let before = self.config.clone();
        let field = self.field();
        if let Err(e) = set_claude_field(&mut self.config, field, &buffer) {
            self.edit = Some(buffer);
            return Some(e);
        }
        self.dirty |= before != self.config;
        None
    }

    pub fn to_value(&self) -> Result<Option<Value>> {
        if self.config.is_empty() {
            return Ok(None);
        }
        Ok(Some(serde_yaml::to_value(&self.config)?))
    }

    /// Resulting claude CLI arguments, shell quoted.
    pub fn args_preview(&self) -> String {
        crate::claude_launch::shell_join(&self.config.build_args())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_key_replaces_block_and_keeps_rest() {
        let text = "# comment\nagent: claude\ntools:\n- key: d\n  name: old\n  command: x\n\n# trailing\nmode: session\n";
        let value: Value =
            serde_yaml::from_str("- {key: t, name: test, command: npm test}").unwrap();
        let out = render_top_level_key(text, "tools", Some(&value)).unwrap();
        assert!(out.starts_with("# comment\nagent: claude\ntools:\n- key: t\n"));
        assert!(out.contains("# trailing\nmode: session\n"));
        assert!(!out.contains("old"));
    }

    #[test]
    fn render_key_appends_and_removes() {
        let value: Value = serde_yaml::from_str("model: opus").unwrap();
        let out = render_top_level_key("agent: claude\n", "claude", Some(&value)).unwrap();
        assert_eq!(out, "agent: claude\n\nclaude:\n  model: opus\n");
        let removed = render_top_level_key(&out, "claude", None).unwrap();
        assert_eq!(removed.trim_end(), "agent: claude");
        assert_eq!(
            render_top_level_key("", "claude", Some(&value)).unwrap(),
            "claude:\n  model: opus\n"
        );
    }

    #[test]
    fn render_key_rejects_non_mapping() {
        assert!(render_top_level_key("- a\n", "tools", None).is_err());
    }

    #[test]
    fn pages_parse_from_doc() {
        let doc: Value = serde_yaml::from_str(
            "tools:\n- {key: d, name: dev, command: npm run dev}\nclaude:\n  preset: yolo\n",
        )
        .unwrap();
        let (tools, claude) = pages_from_yaml(&doc);
        assert_eq!(tools.items[0].name, "dev");
        assert_eq!(claude.preset(), ClaudePreset::Yolo);
        let (tools, claude) = pages_from_yaml(&Value::Null);
        assert!(tools.is_empty() && claude.is_empty());
    }

    #[test]
    fn tools_page_add_walks_fields() {
        let mut page = ToolsPage::new(ToolsConfig::default());
        page.add();
        assert_eq!(page.edit.as_ref().unwrap().0, ToolField::Key);
        page.edit.as_mut().unwrap().1 = "dd".into();
        assert!(page.commit_edit().is_some(), "two-char key rejected");
        page.edit.as_mut().unwrap().1 = "d".into();
        assert!(page.commit_edit().is_none());
        assert_eq!(page.edit.as_ref().unwrap().0, ToolField::Name);
        page.edit.as_mut().unwrap().1 = "dev".into();
        page.commit_edit();
        page.edit.as_mut().unwrap().1 = "npm run dev".into();
        page.commit_edit();
        assert!(page.edit.is_none() && !page.adding && page.dirty);
        assert_eq!(page.tools.items[0].command, "npm run dev");

        page.add();
        page.edit.as_mut().unwrap().1 = "d".into();
        assert!(page.commit_edit().unwrap().contains("already used"));
        page.cancel_edit();
        // Incomplete row is not saved.
        let value = page.to_value().unwrap().unwrap();
        assert_eq!(value.as_sequence().unwrap().len(), 1);
        page.delete();
        page.cursor = 0;
        page.delete();
        assert!(page.to_value().unwrap().is_none());
    }

    #[test]
    fn tools_page_close_toggle_serializes_map() {
        let mut page = ToolsPage::new(ToolsConfig {
            items: vec![ToolConfig {
                key: "d".into(),
                name: "dev".into(),
                command: "x".into(),
            }],
            close_with_session: None,
        });
        page.toggle_close_with_session();
        let v = page.to_value().unwrap().unwrap();
        assert_eq!(v.get("close_with_session"), Some(&Value::Bool(false)));
    }

    #[test]
    fn claude_field_set_and_text() {
        let mut c = ClaudeConfig::default();
        set_claude_field(&mut c, ClaudeField::AllowedTools, "Read, Bash(git log:*) ,").unwrap();
        assert_eq!(c.allowed_tools, vec!["Read", "Bash(git log:*)"]);
        assert_eq!(
            claude_field_text(&c, ClaudeField::AllowedTools),
            "Read, Bash(git log:*)"
        );
        set_claude_field(&mut c, ClaudeField::ExtraArgs, "--verbose --name 'a b'").unwrap();
        assert_eq!(c.extra_args, vec!["--verbose", "--name", "a b"]);
        assert_eq!(
            claude_field_text(&c, ClaudeField::ExtraArgs),
            "--verbose --name 'a b'"
        );
        assert!(set_claude_field(&mut c, ClaudeField::ExtraArgs, "'oops").is_err());
        assert!(set_claude_field(&mut c, ClaudeField::PermissionMode, "nope").is_err());
        set_claude_field(&mut c, ClaudeField::PermissionMode, "plan").unwrap();
        assert_eq!(c.permission_mode.as_deref(), Some("plan"));
        set_claude_field(&mut c, ClaudeField::Model, "  ").unwrap();
        assert_eq!(c.model, None);
    }

    #[test]
    fn claude_field_cycles() {
        let mut c = ClaudeConfig::default();
        cycle_claude_field(&mut c, ClaudeField::Model);
        assert_eq!(c.model.as_deref(), Some("opus"));
        cycle_claude_field(&mut c, ClaudeField::Model);
        cycle_claude_field(&mut c, ClaudeField::Model);
        assert_eq!(c.model.as_deref(), Some("haiku"));
        cycle_claude_field(&mut c, ClaudeField::Model);
        assert_eq!(c.model, None);
        c.model = Some("custom-model".into());
        cycle_claude_field(&mut c, ClaudeField::Model);
        assert_eq!(c.model.as_deref(), Some("opus"));

        cycle_claude_field(&mut c, ClaudeField::Preset);
        assert_eq!(c.preset, Some(ClaudePreset::ReadOnly));
        cycle_claude_field(&mut c, ClaudeField::ResumeOnReopen);
        assert_eq!(c.resume_on_reopen, Some(false));
        cycle_claude_field(&mut c, ClaudeField::ResumeOnReopen);
        assert_eq!(c.resume_on_reopen, None);
    }

    #[test]
    fn claude_page_edit_and_value() {
        let mut page = ClaudePage::new(ClaudeConfig::default());
        assert!(page.to_value().unwrap().is_none());
        page.start_edit();
        page.edit = Some("sonnet".into());
        assert!(page.commit_edit().is_none());
        assert!(page.dirty);
        page.cursor = 2; // preset: Enter cycles
        page.start_edit();
        assert!(page.edit.is_none());
        assert_eq!(page.config.preset, Some(ClaudePreset::ReadOnly));
        assert!(
            page.args_preview()
                .starts_with("--model sonnet --allowedTools")
        );
        page.clear();
        assert_eq!(page.config.preset, None);
        let v = page.to_value().unwrap().unwrap();
        assert_eq!(v.get("model").and_then(Value::as_str), Some("sonnet"));
    }

    #[test]
    fn page_cycle() {
        assert_eq!(ConfigPage::Layout.next(), ConfigPage::Tools);
        assert_eq!(ConfigPage::Claude.next(), ConfigPage::Layout);
    }
}
