//! Option table and comment-preserving text edits behind the dashboard
//! settings menu. Only plain scalar top-level keys are edited here; nested
//! options (`panes`, `sandbox`, ...) are edited by opening the file in
//! `$EDITOR`.

use anyhow::{Context, Result, bail};
use serde_yaml::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Choice(&'static [&'static str]),
    Text,
}

pub struct Opt {
    pub key: &'static str,
    pub section: &'static str,
    pub kind: Kind,
    /// Shown as the default and used for the restored example comment.
    pub default: &'static str,
    pub desc: &'static str,
}

const SCHEMES: &[&str] = &[
    "default",
    "emberforge",
    "glacier-signal",
    "obsidian-pop",
    "slate-garden",
    "phosphor-arcade",
    "lasergrid",
    "mossfire",
    "night-sorbet",
    "graphite-code",
    "festival-circuit",
    "teal-drift",
];

const fn opt(
    key: &'static str,
    section: &'static str,
    kind: Kind,
    default: &'static str,
    desc: &'static str,
) -> Opt {
    Opt {
        key,
        section,
        kind,
        default,
        desc,
    }
}

/// Scalar options editable inside the menu.
pub const OPTIONS: &[Opt] = &[
    opt(
        "theme",
        "Appearance",
        Kind::Choice(SCHEMES),
        "default",
        "Dashboard color scheme",
    ),
    opt(
        "main_branch",
        "Git",
        Kind::Text,
        "main",
        "Branch workmux merges into (auto-detected when unset)",
    ),
    opt(
        "base_branch",
        "Git",
        Kind::Text,
        "auto",
        "Branch new worktrees start from (`auto` = main branch)",
    ),
    opt(
        "merge_strategy",
        "Git",
        Kind::Choice(&["merge", "rebase", "squash"]),
        "merge",
        "Default strategy of `workmux merge`",
    ),
    opt(
        "merge_keep",
        "Git",
        Kind::Bool,
        "false",
        "Keep worktree, window and branch after merge",
    ),
    opt(
        "confirm_unmerged_removal",
        "Git",
        Kind::Bool,
        "false",
        "Ask before removing an unmerged branch",
    ),
    opt(
        "worktree_dir",
        "Naming & Paths",
        Kind::Text,
        ".worktrees",
        "Directory holding the worktrees",
    ),
    opt(
        "worktree_naming",
        "Naming & Paths",
        Kind::Choice(&["full", "basename"]),
        "full",
        "How directory names derive from branch names",
    ),
    opt(
        "worktree_prefix",
        "Naming & Paths",
        Kind::Text,
        "",
        "Prefix of worktree directory names",
    ),
    opt(
        "window_prefix",
        "Naming & Paths",
        Kind::Text,
        "wm-",
        "Prefix of tmux window names",
    ),
    opt(
        "mode",
        "Tmux",
        Kind::Choice(&["window", "session"]),
        "window",
        "One tmux window or one session per worktree",
    ),
    opt(
        "default_session",
        "Tmux",
        Kind::Text,
        "main",
        "Session used when none is current",
    ),
    opt(
        "window_placement",
        "Tmux",
        Kind::Choice(&["after_current", "rightmost"]),
        "after_current",
        "Where new windows are inserted",
    ),
    opt(
        "status_format",
        "Tmux",
        Kind::Bool,
        "true",
        "Let workmux set the tmux status bar format",
    ),
    opt(
        "agent",
        "Agent & AI",
        Kind::Text,
        "claude",
        "Default agent command",
    ),
    opt(
        "stale_after",
        "Dashboard",
        Kind::Text,
        "5h",
        "Idle time after which an agent counts as stale (e.g. 90m, 5h)",
    ),
];

/// Nested options: the menu lists them and opens the file in `$EDITOR`.
pub const COMPLEX: &[(&str, &str)] = &[
    ("panes", "Tmux pane layout"),
    ("windows", "Several tmux windows per worktree"),
    ("status_icons", "Status icons"),
    ("auto_name", "Automatic branch naming"),
    ("hook_shell", "Shell used by hooks"),
    ("post_create", "Commands after creating a worktree"),
    ("pre_merge", "Commands before merge"),
    ("pre_remove", "Commands before removal"),
    ("files", "Files copied or symlinked into worktrees"),
    ("remote_files", "Files for remote worktrees"),
    ("dashboard", "Dashboard behaviour"),
    ("sidebar", "Sidebar behaviour"),
    ("sandbox", "Agent sandbox"),
];

/// Top-level key a column-0 line starts, if any.
fn active_key(line: &str) -> Option<&str> {
    if line.starts_with([' ', '\t', '#', '-']) || line.trim().is_empty() {
        return None;
    }
    line.split_once(':').map(|(k, _)| k.trim())
}

/// Key of a commented-out example (`# key: value`) at column 0.
fn example_key(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('#')?.trim_start();
    if rest.starts_with(['#', '-', ' ']) || rest.is_empty() {
        return None;
    }
    let (k, _) = rest.split_once(':')?;
    (!k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(k)
}

/// 1-based line of the active `key:` or its commented example, if any.
pub fn find_key_line(text: &str, key: &str) -> Option<usize> {
    let line_of = |f: &dyn Fn(&str) -> bool| text.lines().position(|l| f(l)).map(|i| i + 1);
    line_of(&|l| active_key(l) == Some(key)).or_else(|| line_of(&|l| example_key(l) == Some(key)))
}

/// Scalar value of an active top-level `key` as a string (None when unset
/// or not a scalar).
pub fn current_value(text: &str, key: &str) -> Option<String> {
    let doc: Value = serde_yaml::from_str(text).ok()?;
    match doc.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// True when `key` is active with a nested/multi-line value.
fn is_nested(text: &str, key: &str) -> bool {
    serde_yaml::from_str::<Value>(text)
        .ok()
        .and_then(|d| d.get(key).cloned())
        .is_some_and(|v| matches!(v, Value::Mapping(_) | Value::Sequence(_)))
}

/// Validate `value` against the option's kind.
pub fn validate(opt: &Opt, value: &str) -> Result<()> {
    match opt.kind {
        Kind::Bool if value != "true" && value != "false" => bail!("expected true or false"),
        Kind::Choice(list) if !list.contains(&value) => {
            bail!("expected one of: {}", list.join(", "))
        }
        _ => Ok(()),
    }
}

/// Set (`Some`) or reset (`None`) a scalar `key`, keeping every other line
/// byte for byte. Set replaces an active line, else the commented example
/// from `workmux init`, else appends. Reset removes the active line when a
/// commented example exists, else turns it into a `# key: default` comment.
pub fn render_set(text: &str, opt: &Opt, value: Option<&str>) -> Result<String> {
    let key = opt.key;
    if let Some(v) = value {
        validate(opt, v)?;
    }
    if is_nested(text, key) {
        bail!("`{key}` is nested in this file; edit it with the editor");
    }
    let before: Value = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_yaml::from_str(text).context("parse config")?
    };
    if !matches!(before, Value::Mapping(_) | Value::Null) {
        bail!("config is not a YAML mapping");
    }
    let rendered = value.map(|v| match opt.kind {
        Kind::Text => format!("{key}: {}", crate::config::yaml_scalar(v)),
        _ => format!("{key}: {v}"),
    });
    let has_example = text.lines().any(|l| example_key(l) == Some(key));

    let mut out: Vec<String> = Vec::new();
    let mut placed = false;
    for line in text.lines() {
        if active_key(line) == Some(key) {
            match (&rendered, has_example) {
                (Some(r), _) if !placed => {
                    out.push(r.clone());
                    placed = true;
                }
                (None, false) if !placed => {
                    out.push(format!("# {key}: {}", opt.default));
                    placed = true;
                }
                _ => {}
            }
            continue;
        }
        if !placed
            && let Some(r) = &rendered
            && example_key(line) == Some(key)
        {
            out.push(r.clone());
            placed = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !placed && let Some(r) = rendered {
        while out.last().is_some_and(|l| l.trim().is_empty()) {
            out.pop();
        }
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(r);
    }
    let mut result = out.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }

    let after: Value = serde_yaml::from_str(&result).context("generated config is invalid")?;
    if let Value::Mapping(orig) = &before {
        for k in orig.keys() {
            if k.as_str() != Some(key) && after.get(k) != orig.get(k) {
                bail!("refusing to save: would change key {k:?}");
            }
        }
    }
    let wrote = current_value(&result, key);
    if wrote.as_deref() != value {
        bail!("refusing to save: {key} did not round-trip");
    }
    Ok(result)
}

/// Check that the whole text still loads as a workmux config.
pub fn validate_config(text: &str) -> Result<()> {
    serde_yaml::from_str::<crate::config::Config>(text)
        .map(|_| ())
        .context("config no longer loads")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o(key: &str) -> &'static Opt {
        OPTIONS.iter().find(|o| o.key == key).unwrap()
    }

    const TEMPLATE: &str = "# header\n\n# merge_strategy: rebase\n#\n# merge_keep: true\nagent: claude\n\nsidebar:\n  width: 3\n";

    #[test]
    fn set_replaces_example_in_place_keeping_rest() {
        let out = render_set(TEMPLATE, o("merge_strategy"), Some("squash")).unwrap();
        assert_eq!(
            out,
            "# header\n\nmerge_strategy: squash\n#\n# merge_keep: true\nagent: claude\n\nsidebar:\n  width: 3\n"
        );
    }

    #[test]
    fn set_replaces_active_line() {
        let out = render_set(TEMPLATE, o("agent"), Some("codex")).unwrap();
        assert!(out.contains("agent: codex\n"));
        assert!(!out.contains("agent: claude"));
        assert!(out.contains("sidebar:\n  width: 3\n"));
    }

    #[test]
    fn set_appends_when_no_example() {
        let out = render_set("agent: claude\n", o("window_prefix"), Some("wm-")).unwrap();
        assert_eq!(out, "agent: claude\n\nwindow_prefix: wm-\n");
    }

    #[test]
    fn set_into_empty_file() {
        let out = render_set("", o("merge_keep"), Some("true")).unwrap();
        assert_eq!(out, "merge_keep: true\n");
    }

    #[test]
    fn reset_removes_line_when_example_exists() {
        let set = render_set(TEMPLATE, o("merge_keep"), Some("false")).unwrap();
        let out = render_set(&set, o("merge_keep"), None).unwrap();
        assert!(!out.contains("\nmerge_keep:"));
        assert_eq!(
            out.matches("# merge_keep: false").count(),
            1,
            "example restored"
        );
    }

    #[test]
    fn reset_comments_line_without_example() {
        let out = render_set("agent: codex\n", o("agent"), None).unwrap();
        assert_eq!(out, "# agent: claude\n");
    }

    #[test]
    fn reset_unset_key_is_noop() {
        assert_eq!(render_set(TEMPLATE, o("theme"), None).unwrap(), TEMPLATE);
    }

    #[test]
    fn rejects_bad_values() {
        assert!(render_set(TEMPLATE, o("merge_strategy"), Some("fast")).is_err());
        assert!(render_set(TEMPLATE, o("merge_keep"), Some("yes")).is_err());
    }

    #[test]
    fn refuses_nested_key() {
        assert!(render_set("theme:\n  scheme: lasergrid\n", o("theme"), Some("default")).is_err());
    }

    #[test]
    fn text_values_are_quoted_when_needed() {
        let out = render_set("", o("worktree_prefix"), Some("a: b")).unwrap();
        assert_eq!(
            current_value(&out, "worktree_prefix").as_deref(),
            Some("a: b")
        );
    }

    #[test]
    fn finds_key_or_example_line() {
        assert_eq!(find_key_line(TEMPLATE, "merge_strategy"), Some(3));
        assert_eq!(find_key_line(TEMPLATE, "agent"), Some(6));
        assert_eq!(find_key_line(TEMPLATE, "theme"), None);
    }

    #[test]
    fn real_template_round_trips_every_option() {
        let text = crate::config::init_config_text(None);
        for opt in OPTIONS {
            let v = match opt.kind {
                Kind::Bool => "true",
                Kind::Choice(l) => l[l.len() - 1],
                Kind::Text => "x1",
            };
            let set =
                render_set(&text, opt, Some(v)).unwrap_or_else(|e| panic!("{}: {e}", opt.key));
            assert_eq!(
                current_value(&set, opt.key).as_deref(),
                Some(v),
                "{}",
                opt.key
            );
            let back = render_set(&set, opt, None).unwrap();
            assert_eq!(current_value(&back, opt.key), None, "{}", opt.key);
        }
    }
}
