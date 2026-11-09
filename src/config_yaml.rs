//! Line-preserving edits of top-level keys in YAML config files.

use anyhow::{Context, Result};
use serde_yaml::Value;

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
