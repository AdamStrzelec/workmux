//! Claude launch settings (`claude:` in `.workmux.yaml`) and run tools
//! (`tools:`). Pure data + argument building; no I/O.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Permission preset applied before explicit tool lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaudePreset {
    #[default]
    None,
    ReadOnly,
    NoGit,
    GitNoCommit,
    Full,
    Yolo,
}

impl ClaudePreset {
    pub const ALL: [ClaudePreset; 6] = [
        ClaudePreset::None,
        ClaudePreset::ReadOnly,
        ClaudePreset::NoGit,
        ClaudePreset::GitNoCommit,
        ClaudePreset::Full,
        ClaudePreset::Yolo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ClaudePreset::None => "none",
            ClaudePreset::ReadOnly => "read-only",
            ClaudePreset::NoGit => "no-git",
            ClaudePreset::GitNoCommit => "git-no-commit",
            ClaudePreset::Full => "full",
            ClaudePreset::Yolo => "yolo",
        }
    }

    /// (allowed, disallowed) tool lists of the preset.
    fn tools(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            ClaudePreset::ReadOnly => (&["Read", "Grep", "Glob"], &["Edit", "Write", "Bash"]),
            ClaudePreset::NoGit => (&[], &["Bash(git:*)", "Bash(gh:*)"]),
            ClaudePreset::GitNoCommit => (
                &[],
                &["Bash(git commit:*)", "Bash(git push:*)", "Bash(gh pr:*)"],
            ),
            _ => (&[], &[]),
        }
    }

    /// Next preset in [`Self::ALL`] (wraps).
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// Permission modes accepted by `claude --permission-mode`.
pub const PERMISSION_MODES: [&str; 4] = ["default", "acceptEdits", "plan", "bypassPermissions"];

/// Model shortcuts offered by the editor (any string is accepted).
pub const MODELS: [&str; 3] = ["opus", "sonnet", "haiku"];

/// `claude:` section of the config. Every field is optional.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct ClaudeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<ClaudePreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disallowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_dirs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    /// Resume the Claude session saved on close (`c`) when reopening. Default: true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_on_reopen: Option<bool>,
}

fn non_empty(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn push_unique(list: &mut Vec<String>, items: impl IntoIterator<Item = String>) {
    for item in items {
        let item = item.trim().to_string();
        if !item.is_empty() && !list.contains(&item) {
            list.push(item);
        }
    }
}

impl ClaudeConfig {
    pub fn resume_on_reopen(&self) -> bool {
        self.resume_on_reopen.unwrap_or(true)
    }

    pub fn preset(&self) -> ClaudePreset {
        self.preset.unwrap_or_default()
    }

    /// True when nothing would be written to the config.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Claude CLI arguments for these settings (one argv entry per item, so
    /// values with spaces stay intact; quote with [`shell_join`] for display).
    pub fn build_args(&self) -> Vec<String> {
        let preset = self.preset();
        let mut args = Vec::new();
        if let Some(model) = non_empty(&self.model) {
            args.extend(["--model".to_string(), model.to_string()]);
        }
        let mode = non_empty(&self.permission_mode)
            .or((preset == ClaudePreset::Full).then_some("acceptEdits"));
        if let Some(mode) = mode {
            args.extend(["--permission-mode".to_string(), mode.to_string()]);
        }
        if preset == ClaudePreset::Yolo {
            args.push("--dangerously-skip-permissions".to_string());
        }
        let (preset_allowed, preset_disallowed) = preset.tools();
        let mut allowed = Vec::new();
        push_unique(&mut allowed, preset_allowed.iter().map(|s| s.to_string()));
        push_unique(&mut allowed, self.allowed_tools.iter().cloned());
        let mut disallowed = Vec::new();
        push_unique(
            &mut disallowed,
            preset_disallowed.iter().map(|s| s.to_string()),
        );
        push_unique(&mut disallowed, self.disallowed_tools.iter().cloned());
        // An explicit allow wins over a preset deny of the same tool.
        disallowed.retain(|t| !self.allowed_tools.iter().any(|a| a.trim() == t));
        if !allowed.is_empty() {
            args.push("--allowedTools".to_string());
            args.extend(allowed);
        }
        if !disallowed.is_empty() {
            args.push("--disallowedTools".to_string());
            args.extend(disallowed);
        }
        let dirs: Vec<String> = self
            .add_dirs
            .iter()
            .map(|d| d.trim())
            .filter(|d| !d.is_empty())
            .map(String::from)
            .collect();
        if !dirs.is_empty() {
            args.push("--add-dir".to_string());
            args.extend(dirs);
        }
        if let Some(prompt) = non_empty(&self.append_system_prompt) {
            args.extend(["--append-system-prompt".to_string(), prompt.to_string()]);
        }
        args.extend(
            self.extra_args
                .iter()
                .filter(|a| !a.trim().is_empty())
                .cloned(),
        );
        args
    }
}

/// Variadic flags (`--allowedTools a b`) swallow following positional
/// arguments, so callers that append a prompt must terminate them. Returns
/// true when `args` ends inside a variadic list.
pub fn ends_with_variadic(args: &[String]) -> bool {
    const VARIADIC: [&str; 3] = ["--allowedTools", "--disallowedTools", "--add-dir"];
    let last_flag = args.iter().rposition(|a| a.starts_with("--"));
    last_flag.is_some_and(|i| VARIADIC.contains(&args[i].as_str()))
}

/// Settings args to append to an existing claude argv: drops a skip-permissions
/// flag already present and terminates a trailing variadic list with `--` so a
/// prompt appended later is not taken as a tool name.
pub fn merge_launch_args(existing: &[String], mut args: Vec<String>) -> Vec<String> {
    const SKIP: &str = "--dangerously-skip-permissions";
    if existing.iter().any(|a| a == SKIP) {
        args.retain(|a| a != SKIP);
    }
    if ends_with_variadic(&args) {
        args.push("--".to_string());
    }
    args
}

impl ClaudeConfig {
    /// `self` with every field set in `over` replacing it (per-worktree
    /// override on top of the project settings).
    pub fn overlay(&self, over: &ClaudeConfig) -> ClaudeConfig {
        let list = |base: &Vec<String>, over: &Vec<String>| {
            if over.is_empty() {
                base.clone()
            } else {
                over.clone()
            }
        };
        ClaudeConfig {
            model: over.model.clone().or_else(|| self.model.clone()),
            permission_mode: over
                .permission_mode
                .clone()
                .or_else(|| self.permission_mode.clone()),
            preset: over.preset.or(self.preset),
            allowed_tools: list(&self.allowed_tools, &over.allowed_tools),
            disallowed_tools: list(&self.disallowed_tools, &over.disallowed_tools),
            add_dirs: list(&self.add_dirs, &over.add_dirs),
            append_system_prompt: over
                .append_system_prompt
                .clone()
                .or_else(|| self.append_system_prompt.clone()),
            extra_args: list(&self.extra_args, &over.extra_args),
            resume_on_reopen: over.resume_on_reopen.or(self.resume_on_reopen),
        }
    }
}

/// Worktree metadata key holding the per-worktree Claude override (JSON).
pub const CLAUDE_CONFIG_META: &str = "claude-config";

/// Per-worktree Claude override, if one is saved.
pub fn worktree_override(handle: &str, workdir: Option<&Path>) -> Option<ClaudeConfig> {
    crate::git::get_worktree_meta_in(handle, CLAUDE_CONFIG_META, workdir)
        .and_then(|json| serde_json::from_str(&json).ok())
        .filter(|c: &ClaudeConfig| !c.is_empty())
}

/// Save (or remove, when empty) the per-worktree Claude override.
pub fn save_worktree_override(
    handle: &str,
    workdir: Option<&Path>,
    config: &ClaudeConfig,
) -> anyhow::Result<()> {
    if config.is_empty() {
        return crate::git::unset_worktree_meta_in(handle, CLAUDE_CONFIG_META, workdir);
    }
    let json = serde_json::to_string(config)?;
    crate::git::set_worktree_meta_in(handle, CLAUDE_CONFIG_META, &json, workdir)
}

/// Shell-quote and join args for display or a command line.
pub fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            shlex::try_quote(a)
                .map(|q| q.into_owned())
                .unwrap_or_else(|_| a.clone())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Session to resume when (re)opening a worktree window: the saved one,
/// when resume is enabled, nothing else asked for a resume mode, this is not
/// an extra `--new` window and the transcript still exists.
pub fn session_to_resume(
    enabled: bool,
    other_resume_requested: bool,
    new_window: bool,
    saved: Option<String>,
    transcript_exists: impl FnOnce(&str) -> bool,
) -> Option<String> {
    if !enabled || other_resume_requested || new_window {
        return None;
    }
    let saved = saved
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    transcript_exists(&saved).then_some(saved)
}

/// One run tool: a command started in its own tmux window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct ToolConfig {
    pub key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub command: String,
    /// Ready-made defaults (`nextjs`, `vite`, `react`, `storybook`). Fills
    /// name, command, port and `auto_port` that are not set explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// Port the tool listens on; a warning is shown when it is already taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// When `port` is taken, start the tool on the next free port instead.
    /// `{port}` in the command is replaced and `PORT` is set for the command.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_port: bool,
}

/// Built-in dev server presets: `(name, command, port)`. `{port}` is
/// replaced by the chosen port when `auto_port` is on.
pub const TOOL_PRESETS: &[(&str, &str, u16)] = &[
    ("nextjs", "npm run dev -- --port {port}", 3000),
    ("vite", "npm run dev -- --port {port}", 5173),
    ("react", "npm start", 3000),
    ("storybook", "npm run storybook -- --port {port} --ci", 6006),
];

impl ToolConfig {
    /// Fill unset fields from the named preset, then drop the `preset` field.
    pub fn resolve_preset(&mut self) -> Result<(), String> {
        if let Some(name) = self.preset.take() {
            let (_, command, port) = TOOL_PRESETS
                .iter()
                .find(|(n, _, _)| *n == name)
                .ok_or_else(|| {
                    let known: Vec<&str> = TOOL_PRESETS.iter().map(|p| p.0).collect();
                    format!("unknown tool preset '{name}' (known: {})", known.join(", "))
                })?;
            if self.name.trim().is_empty() {
                self.name = name;
            }
            if self.command.trim().is_empty() {
                self.command = command.to_string();
            }
            if self.port.is_none() {
                self.port = Some(*port);
                self.auto_port = true;
            }
        }
        if self.name.trim().is_empty() || self.command.trim().is_empty() {
            return Err(format!(
                "tool '{}' needs a name and a command (or a preset)",
                self.key
            ));
        }
        Ok(())
    }

    pub fn key_char(&self) -> Option<char> {
        let mut chars = self.key.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    }
}

/// `tools:` section: a plain list, or `{ close_with_session, items }`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolsConfig {
    pub items: Vec<ToolConfig>,
    /// Kill tool windows when the worktree window is closed with `c`. Default: true.
    pub close_with_session: Option<bool>,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum ToolsRepr {
    List(Vec<ToolConfig>),
    Map {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        close_with_session: Option<bool>,
        #[serde(default)]
        items: Vec<ToolConfig>,
    },
}

impl<'de> Deserialize<'de> for ToolsConfig {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut cfg = match ToolsRepr::deserialize(d)? {
            ToolsRepr::List(items) => ToolsConfig {
                items,
                close_with_session: None,
            },
            ToolsRepr::Map {
                close_with_session,
                items,
            } => ToolsConfig {
                items,
                close_with_session,
            },
        };
        for t in &mut cfg.items {
            t.resolve_preset().map_err(serde::de::Error::custom)?;
        }
        Ok(cfg)
    }
}

impl Serialize for ToolsConfig {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let repr = match self.close_with_session {
            None => ToolsRepr::List(self.items.clone()),
            Some(v) => ToolsRepr::Map {
                close_with_session: Some(v),
                items: self.items.clone(),
            },
        };
        repr.serialize(s)
    }
}

impl ToolsConfig {
    pub fn close_with_session(&self) -> bool {
        self.close_with_session.unwrap_or(true)
    }

    #[cfg(test)]
    pub fn by_key(&self, c: char) -> Option<&ToolConfig> {
        self.items.iter().find(|t| t.key_char() == Some(c))
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Separator between worktree handle and tool name in tool window names.
pub const TOOL_WINDOW_SEP: &str = "/";

/// tmux window name of a tool window (before `window_prefix`). `:` and `.`
/// are tmux target separators, so they are replaced in the tool part.
pub fn tool_window_name(handle: &str, tool: &str) -> String {
    let tool: String = tool
        .chars()
        .map(|c| {
            if c.is_whitespace() || c == '.' || c == ':' {
                '-'
            } else {
                c
            }
        })
        .collect();
    format!("{handle}{TOOL_WINDOW_SEP}{tool}")
}

/// Whether `window_name` (already without prefix) is a tool window of `handle`.
pub fn is_tool_window_of(window_name: &str, handle: &str) -> bool {
    window_name
        .strip_prefix(handle)
        .and_then(|rest| rest.strip_prefix(TOOL_WINDOW_SEP))
        .is_some_and(|tool| !tool.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(yaml: &str) -> ClaudeConfig {
        serde_yaml::from_str(yaml).unwrap()
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn empty_config_builds_no_args() {
        assert!(ClaudeConfig::default().build_args().is_empty());
        assert!(ClaudeConfig::default().resume_on_reopen());
    }

    #[test]
    fn model_and_permission_mode() {
        let c = cfg("model: opus\npermission_mode: plan\n");
        assert_eq!(
            c.build_args(),
            s(&["--model", "opus", "--permission-mode", "plan"])
        );
    }

    #[test]
    fn preset_read_only() {
        let c = cfg("preset: read-only\n");
        assert_eq!(
            c.build_args(),
            s(&[
                "--allowedTools",
                "Read",
                "Grep",
                "Glob",
                "--disallowedTools",
                "Edit",
                "Write",
                "Bash"
            ])
        );
    }

    #[test]
    fn preset_no_git_and_git_no_commit() {
        assert_eq!(
            cfg("preset: no-git").build_args(),
            s(&["--disallowedTools", "Bash(git:*)", "Bash(gh:*)"])
        );
        assert_eq!(
            cfg("preset: git-no-commit").build_args(),
            s(&[
                "--disallowedTools",
                "Bash(git commit:*)",
                "Bash(git push:*)",
                "Bash(gh pr:*)"
            ])
        );
    }

    #[test]
    fn preset_full_and_yolo() {
        assert_eq!(
            cfg("preset: full").build_args(),
            s(&["--permission-mode", "acceptEdits"])
        );
        // Explicit mode wins over the preset default.
        assert_eq!(
            cfg("preset: full\npermission_mode: plan").build_args(),
            s(&["--permission-mode", "plan"])
        );
        assert_eq!(
            cfg("preset: yolo").build_args(),
            s(&["--dangerously-skip-permissions"])
        );
        assert!(cfg("preset: none").build_args().is_empty());
    }

    #[test]
    fn explicit_lists_merge_on_top_of_preset() {
        let c =
            cfg("preset: read-only\nallowed_tools: [Bash, Read]\ndisallowed_tools: [WebFetch]\n");
        assert_eq!(
            c.build_args(),
            s(&[
                "--allowedTools",
                "Read",
                "Grep",
                "Glob",
                "Bash",
                "--disallowedTools",
                "Edit",
                "Write",
                "WebFetch"
            ])
        );
    }

    #[test]
    fn dirs_prompt_and_extra_args() {
        let c = cfg(
            "add_dirs: [../shared, '  ']\nappend_system_prompt: \"Be terse; don't push\"\nextra_args: [--verbose]\n",
        );
        let args = c.build_args();
        assert_eq!(
            args,
            s(&[
                "--add-dir",
                "../shared",
                "--append-system-prompt",
                "Be terse; don't push",
                "--verbose"
            ])
        );
        // Quoted output parses back to the same argv.
        assert_eq!(shlex::split(&shell_join(&args)).unwrap(), args);
    }

    #[test]
    fn shell_join_quotes_tool_patterns() {
        assert_eq!(
            shell_join(&s(&["--disallowedTools", "Bash(git commit:*)"])),
            "--disallowedTools 'Bash(git commit:*)'"
        );
        let round = shlex::split(&shell_join(&cfg("preset: git-no-commit").build_args())).unwrap();
        assert_eq!(round, cfg("preset: git-no-commit").build_args());
    }

    #[test]
    fn variadic_detection() {
        assert!(ends_with_variadic(&cfg("preset: no-git").build_args()));
        assert!(!ends_with_variadic(&cfg("model: opus").build_args()));
        assert!(!ends_with_variadic(
            &cfg("preset: no-git\nappend_system_prompt: hi").build_args()
        ));
    }

    #[test]
    fn merge_launch_args_dedups_and_terminates() {
        let existing = s(&["--dangerously-skip-permissions"]);
        assert!(merge_launch_args(&existing, cfg("preset: yolo").build_args()).is_empty());
        assert_eq!(
            merge_launch_args(&[], cfg("preset: no-git").build_args()),
            s(&["--disallowedTools", "Bash(git:*)", "Bash(gh:*)", "--"])
        );
        assert_eq!(
            merge_launch_args(&[], cfg("model: opus").build_args()),
            s(&["--model", "opus"])
        );
    }

    #[test]
    fn overlay_replaces_only_set_fields() {
        let base = cfg("model: opus\npreset: no-git\nallowed_tools: [Read]\nadd_dirs: [/a]\n");
        let over = cfg("model: sonnet\nallowed_tools: [Bash]\n");
        let merged = base.overlay(&over);
        assert_eq!(merged.model.as_deref(), Some("sonnet"));
        assert_eq!(merged.preset, Some(ClaudePreset::NoGit));
        assert_eq!(merged.allowed_tools, vec!["Bash".to_string()]);
        assert_eq!(merged.add_dirs, vec!["/a".to_string()]);
        assert_eq!(base.overlay(&ClaudeConfig::default()), base);
    }

    #[test]
    fn session_to_resume_rules() {
        let id = || Some("abc".to_string());
        assert_eq!(
            session_to_resume(true, false, false, id(), |_| true),
            Some("abc".into())
        );
        assert_eq!(session_to_resume(false, false, false, id(), |_| true), None);
        assert_eq!(session_to_resume(true, true, false, id(), |_| true), None);
        assert_eq!(session_to_resume(true, false, true, id(), |_| true), None);
        assert_eq!(session_to_resume(true, false, false, id(), |_| false), None);
        assert_eq!(session_to_resume(true, false, false, None, |_| true), None);
        assert_eq!(
            session_to_resume(true, false, false, Some(" ".into()), |_| true),
            None
        );
    }

    #[test]
    fn claude_config_round_trips() {
        let c = cfg("model: sonnet\npreset: git-no-commit\nresume_on_reopen: false\n");
        assert!(!c.resume_on_reopen());
        let text = serde_yaml::to_string(&c).unwrap();
        assert_eq!(
            text,
            "model: sonnet\npreset: git-no-commit\nresume_on_reopen: false\n"
        );
        assert_eq!(cfg(&text), c);
    }

    #[test]
    fn preset_cycle_wraps() {
        assert_eq!(ClaudePreset::None.next(), ClaudePreset::ReadOnly);
        assert_eq!(ClaudePreset::Yolo.next(), ClaudePreset::None);
    }

    #[test]
    fn tools_parse_list_and_map() {
        let list: ToolsConfig =
            serde_yaml::from_str("- { key: d, name: dev, command: npm run dev }\n").unwrap();
        assert_eq!(list.items.len(), 1);
        assert!(list.close_with_session());
        assert_eq!(list.by_key('d').unwrap().command, "npm run dev");
        assert!(list.by_key('x').is_none());

        let map: ToolsConfig = serde_yaml::from_str(
            "close_with_session: false\nitems:\n  - { key: t, name: test, command: npm test }\n",
        )
        .unwrap();
        assert!(!map.close_with_session());
        assert_eq!(map.items[0].name, "test");
    }

    #[test]
    fn preset_fills_unset_fields_and_overrides_win() {
        let t: ToolsConfig = serde_yaml::from_str(
            "- { key: d, preset: nextjs }\n- { key: s, preset: storybook, port: 7000, name: sb }\n",
        )
        .unwrap();
        assert_eq!(t.items[0].name, "nextjs");
        assert_eq!(t.items[0].port, Some(3000));
        assert!(t.items[0].auto_port);
        assert!(t.items[0].command.contains("{port}"));
        assert_eq!(t.items[1].name, "sb");
        assert_eq!(t.items[1].port, Some(7000));
        assert!(serde_yaml::from_str::<ToolsConfig>("- { key: x, preset: nope }").is_err());
        assert!(serde_yaml::from_str::<ToolsConfig>("- { key: x }").is_err());
    }

    #[test]
    fn tools_serialize_round_trip() {
        let mut t = ToolsConfig {
            items: vec![ToolConfig {
                key: "d".into(),
                name: "dev".into(),
                command: "npm run dev".into(),
                port: None,
                auto_port: false,
                preset: None,
            }],
            close_with_session: None,
        };
        let text = serde_yaml::to_string(&t).unwrap();
        assert!(text.starts_with("- key: d"));
        assert_eq!(serde_yaml::from_str::<ToolsConfig>(&text).unwrap(), t);
        t.close_with_session = Some(false);
        let text = serde_yaml::to_string(&t).unwrap();
        assert!(text.contains("close_with_session: false"));
        assert_eq!(serde_yaml::from_str::<ToolsConfig>(&text).unwrap(), t);
    }

    #[test]
    fn tool_key_must_be_single_char() {
        let mut t = ToolConfig::default();
        assert_eq!(t.key_char(), None);
        t.key = "dd".into();
        assert_eq!(t.key_char(), None);
        t.key = "d".into();
        assert_eq!(t.key_char(), Some('d'));
    }

    #[test]
    fn tool_window_naming_and_lookup() {
        assert_eq!(tool_window_name("feat-x", "dev"), "feat-x/dev");
        assert_eq!(
            tool_window_name("feat-x", "dev server.1"),
            "feat-x/dev-server-1"
        );
        assert!(is_tool_window_of("feat-x/dev", "feat-x"));
        assert!(!is_tool_window_of("feat-x", "feat-x"));
        assert!(!is_tool_window_of("feat-x/", "feat-x"));
        assert!(!is_tool_window_of("feat-xy/dev", "feat-x"));
        assert!(!is_tool_window_of("other/dev", "feat-x"));
    }
}
