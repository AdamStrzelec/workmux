//! `workmux init --global`: write the default global config
//! (`~/.config/workmux/config.yaml`), or add missing default keys to an
//! existing one without touching anything else.

use std::path::Path;

use anyhow::{Context, Result};
use serde_yaml::Value;

/// Template written when no global config exists.
pub const TEMPLATE: &str = "\
# workmux global config (applies to every project; a project's .workmux.yaml overrides it).
nerdfont: true
theme: night-sorbet
# Each worktree gets its own tmux session.
mode: session
# Every pane (agent, shell, commands) opens in its own tab, no splits.
split_panes: false
# Projects shown in the dashboard project picker (`P`).
# projects:
#   - ~/repozytoria/some-project
";

/// Keys added to an existing config when missing.
const DEFAULTS: [(&str, &str); 3] = [
    ("nerdfont", "true"),
    ("mode", "session"),
    ("split_panes", "false"),
];

/// New text for an existing config and the keys that were added.
pub fn add_missing_defaults(text: &str) -> Result<(String, Vec<&'static str>)> {
    let doc: Value = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_yaml::from_str(text).context("existing global config is not valid YAML")?
    };
    let mut updated = text.to_string();
    let mut added = Vec::new();
    for (key, value) in DEFAULTS {
        if doc.get(key).is_none() {
            let value: Value = serde_yaml::from_str(value)?;
            updated = crate::config_yaml::render_top_level_key(&updated, key, Some(&value))?;
            added.push(key);
        }
    }
    Ok((updated, added))
}

pub fn run(force: bool) -> Result<()> {
    let path = crate::config::global_config_path().context("cannot resolve config directory")?;
    write_at(&path, force)
}

fn write_at(path: &Path, force: bool) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    if !path.exists() {
        std::fs::write(path, TEMPLATE).with_context(|| format!("write {}", path.display()))?;
        println!("✓ Created {}", path.display());
        return Ok(());
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if force {
        let backup = path.with_extension("yaml.bak");
        std::fs::write(&backup, &text).with_context(|| format!("write {}", backup.display()))?;
        std::fs::write(path, TEMPLATE).with_context(|| format!("write {}", path.display()))?;
        println!(
            "✓ Rewrote {} (old file: {})",
            path.display(),
            backup.display()
        );
        return Ok(());
    }
    let (updated, added) = add_missing_defaults(&text)?;
    if added.is_empty() {
        println!(
            "{} already has the defaults; nothing changed.",
            path.display()
        );
        println!("  Use --global --force to rewrite it (old file is backed up).");
    } else {
        std::fs::write(path, updated).with_context(|| format!("write {}", path.display()))?;
        println!("✓ Added {} to {}", added.join(", "), path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses_with_expected_defaults() {
        let doc: Value = serde_yaml::from_str(TEMPLATE).unwrap();
        assert_eq!(doc["mode"].as_str(), Some("session"));
        assert_eq!(doc["split_panes"].as_bool(), Some(false));
        assert!(add_missing_defaults(TEMPLATE).unwrap().1.is_empty());
    }

    #[test]
    fn adds_only_missing_keys_and_keeps_rest() {
        let (out, added) = add_missing_defaults("# mine\nmode: window\ntheme: x\n").unwrap();
        assert_eq!(added, vec!["nerdfont", "split_panes"]);
        assert!(out.starts_with("# mine\nmode: window\ntheme: x\n"));
        let doc: Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(doc["mode"].as_str(), Some("window"));
        assert_eq!(doc["split_panes"].as_bool(), Some(false));
    }

    #[test]
    fn write_creates_merges_and_forces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workmux/config.yaml");
        write_at(&path, false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
        std::fs::write(&path, "theme: x\n").unwrap();
        write_at(&path, false).unwrap();
        let merged = std::fs::read_to_string(&path).unwrap();
        assert!(merged.starts_with("theme: x\n") && merged.contains("split_panes: false"));
        write_at(&path, true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
        assert_eq!(
            std::fs::read_to_string(path.with_extension("yaml.bak")).unwrap(),
            merged
        );
    }
}
