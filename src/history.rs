//! Search of local Claude Code conversations (`<config>/projects/*/*.jsonl`).
//! Read-only: nothing is written and nothing leaves the machine.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;
use serde_json::Value;

/// Files larger than this are skipped (pathological transcripts).
const MAX_FILE_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Message {
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SessionFile {
    pub id: String,
    pub path: PathBuf,
    pub modified: SystemTime,
    /// Project directory name under `projects/` (encoded path).
    pub project: String,
}

pub fn projects_dir() -> Option<PathBuf> {
    let base = std::env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .ok()
        .or_else(|| home::home_dir().map(|h| h.join(".claude")))?;
    Some(base.join("projects"))
}

/// Session files of one project dir, newest first.
pub fn sessions_in(dir: &Path) -> Vec<SessionFile> {
    let project = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<SessionFile> = rd
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                return None;
            }
            let meta = e.metadata().ok()?;
            if meta.len() > MAX_FILE_BYTES {
                return None;
            }
            Some(SessionFile {
                id: path.file_stem()?.to_string_lossy().into_owned(),
                modified: meta.modified().ok()?,
                path,
                project: project.clone(),
            })
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.modified));
    out
}

/// Sessions of the project directory matching `cwd`, or of every project.
pub fn collect(cwd: &Path, all: bool) -> Vec<SessionFile> {
    let Some(root) = projects_dir() else {
        return Vec::new();
    };
    let mut out = if all {
        fs::read_dir(&root)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .flat_map(|e| sessions_in(&e.path()))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        let enc = crate::multiplexer::conversation::ClaudeForker::encode_path(cwd);
        sessions_in(&root.join(enc))
    };
    out.sort_by_key(|s| std::cmp::Reverse(s.modified));
    out
}

/// Text of a message `content` value (string or array of blocks). Tool
/// results, tool calls and thinking blocks are ignored.
fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Injected or system-generated user text, not something a person typed.
fn is_noise(text: &str) -> bool {
    let t = text.trim_start();
    t.is_empty()
        || t.starts_with("<command-")
        || t.starts_with("<local-command")
        || t.starts_with("<system-reminder>")
        || t.starts_with("Caveat:")
        || t.starts_with("[Request interrupted")
}

/// One JSONL line to a message; None for anything but visible chat text.
pub fn parse_line(line: &str) -> Option<Message> {
    let v: Value = serde_json::from_str(line).ok()?;
    let role = v.get("type")?.as_str()?;
    if role != "user" && role != "assistant" {
        return None;
    }
    if v.get("isSidechain").and_then(Value::as_bool) == Some(true)
        || v.get("isMeta").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let text = content_text(v.get("message")?.get("content")?);
    let text = text.trim();
    if text.is_empty() || (role == "user" && is_noise(text)) {
        return None;
    }
    Some(Message {
        role: role.to_string(),
        text: text.to_string(),
    })
}

pub fn read_messages(path: &Path) -> Vec<Message> {
    let Ok(f) = fs::File::open(path) else {
        return Vec::new();
    };
    BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| parse_line(&l))
        .collect()
}

/// A short excerpt around the first case-insensitive match of `query`.
pub fn snippet(text: &str, query: &str, radius: usize) -> Option<String> {
    let q = query.to_lowercase();
    if q.is_empty() {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let lowered: Vec<char> = text.to_lowercase().chars().collect();
    // Lowercasing can change the char count; then offsets would not map.
    let src = if lowered.len() == chars.len() {
        &chars
    } else {
        &lowered
    };
    let hay: String = lowered.iter().collect();
    let byte_pos = hay.find(&q)?;
    let start_char = hay[..byte_pos].chars().count();
    let q_len = q.chars().count();
    let from = start_char.saturating_sub(radius);
    let to = (start_char + q_len + radius).min(src.len());
    let body: String = src[from..to].iter().collect();
    let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(format!(
        "{}{}{}",
        if from > 0 { "…" } else { "" },
        body,
        if to < src.len() { "…" } else { "" }
    ))
}

pub fn matches(msgs: &[Message], query: &str) -> usize {
    let q = query.to_lowercase();
    msgs.iter()
        .filter(|m| m.text.to_lowercase().contains(&q))
        .count()
}

/// Truncate to `max` chars, marking the cut.
pub fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}… [truncated]")
}

/// Last `n` things the person typed, oldest first.
pub fn recent_prompts(msgs: &[Message], n: usize) -> Vec<String> {
    let mut v: Vec<String> = msgs
        .iter()
        .rev()
        .filter(|m| m.role == "user")
        .take(n)
        .map(|m| m.text.clone())
        .collect();
    v.reverse();
    v
}

pub fn age(modified: SystemTime) -> String {
    let secs = SystemTime::now()
        .duration_since(modified)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_and_block_content() {
        let a = r#"{"type":"user","message":{"role":"user","content":"hello there"}}"#;
        assert_eq!(parse_line(a).unwrap().text, "hello there");
        let b = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"x"},{"type":"text","text":"hi"},{"type":"tool_use","name":"Bash"}]}}"#;
        let m = parse_line(b).unwrap();
        assert_eq!((m.role.as_str(), m.text.as_str()), ("assistant", "hi"));
    }

    #[test]
    fn skips_noise_tool_results_sidechains_and_junk() {
        for l in [
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"out"}]}}"#,
            r#"{"type":"user","message":{"content":"<system-reminder>x</system-reminder>"}}"#,
            r#"{"type":"user","message":{"content":"<command-name>/compact</command-name>"}}"#,
            r#"{"type":"user","isMeta":true,"message":{"content":"meta"}}"#,
            r#"{"type":"assistant","isSidechain":true,"message":{"content":"side"}}"#,
            r#"{"type":"summary","summary":"s"}"#,
            "not json",
        ] {
            assert!(parse_line(l).is_none(), "{l}");
        }
    }

    #[test]
    fn snippet_is_case_insensitive_and_clipped() {
        let t = "aaaa bbbb Needle cccc dddd";
        assert_eq!(snippet(t, "needle", 5).unwrap(), "…bbbb Needle cccc…");
        assert!(snippet(t, "zzz", 5).is_none());
        assert_eq!(snippet("Needle", "needle", 10).unwrap(), "Needle");
    }

    #[test]
    fn counts_matches_and_recent_prompts() {
        let m = |r: &str, t: &str| Message {
            role: r.into(),
            text: t.into(),
        };
        let msgs = vec![
            m("user", "one"),
            m("assistant", "answer one"),
            m("user", "two"),
            m("user", "three"),
        ];
        assert_eq!(matches(&msgs, "ONE"), 2);
        assert_eq!(recent_prompts(&msgs, 2), vec!["two", "three"]);
    }

    #[test]
    fn clip_marks_truncation() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("abcdef", 3), "abc… [truncated]");
    }
}
