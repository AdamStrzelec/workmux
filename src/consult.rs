//! Second opinion from another model (`workmux consult`). workmux does not
//! talk to any API itself: it builds a prompt and pipes it to the command the
//! user configured in `consult.command` (e.g. `llm -m gemini`, `codex exec -`).
//! Everything in the prompt leaves this machine, so protected files are never
//! included and the size is capped.

use serde::{Deserialize, Serialize};

use crate::guard::{self, GuardConfig};

pub const DEFAULT_MAX_BYTES: usize = 200_000;

/// `consult` section of the global config.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsultConfig {
    /// Command that reads the prompt on stdin and prints the answer.
    #[serde(default)]
    pub command: Option<String>,
    /// Cap on prompt context (files and diff) in bytes. Default 200000.
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

impl ConsultConfig {
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_MAX_BYTES)
    }
}

const REVIEW_INSTRUCTIONS: &str = "You are a strict code reviewer. Review the diff below for bugs, \
security problems and missing tests. Reply with exactly one first line: `APPROVE` if the change \
is safe to merge, or `REJECT: <short reason>` otherwise. After that line you may list findings.";

/// Split `files` into (allowed, dropped-as-protected).
pub fn split_protected(cfg: &GuardConfig, files: &[String]) -> (Vec<String>, Vec<String>) {
    let dropped = guard::violations(cfg, files);
    let allowed = files
        .iter()
        .filter(|f| !dropped.contains(f))
        .cloned()
        .collect();
    (allowed, dropped)
}

/// Cut `text` to at most `max` bytes on a char boundary, noting the cut.
pub fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[... truncated at {max} bytes ...]\n", &text[..end])
}

/// Assemble the prompt. `files` are (path, content) pairs.
pub fn build_prompt(
    review: bool,
    question: &str,
    files: &[(String, String)],
    diff: Option<&str>,
) -> String {
    let mut out = String::new();
    if review {
        out.push_str(REVIEW_INSTRUCTIONS);
        out.push_str("\n\n");
    }
    if !question.trim().is_empty() {
        out.push_str(question.trim());
        out.push_str("\n\n");
    }
    for (path, content) in files {
        out.push_str(&format!("### File: {path}\n```\n{content}\n```\n\n"));
    }
    if let Some(d) = diff {
        out.push_str(&format!("### Diff\n```diff\n{d}\n```\n"));
    }
    out
}

/// True when the first non-empty line of `answer` starts with APPROVE.
pub fn approved(answer: &str) -> bool {
    answer
        .lines()
        .map(|l| l.trim().trim_matches(|c| c == '`' || c == '*'))
        .find(|l| !l.is_empty())
        .is_some_and(|l| l.to_ascii_uppercase().starts_with("APPROVE"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_reads_first_nonempty_line_only() {
        assert!(approved("APPROVE\nlooks fine"));
        assert!(approved("\n  `approve`\n"));
        assert!(approved("**APPROVE**"));
        assert!(!approved("REJECT: bug\nAPPROVE"));
        assert!(!approved("I would approve this"));
        assert!(!approved(""));
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("abc", 10), "abc");
        let t = truncate("zażółć", 4);
        assert!(t.starts_with("za") && t.contains("truncated"));
    }

    #[test]
    fn protected_files_are_split_off() {
        let cfg = GuardConfig {
            protected_paths: vec![".env*".into()],
            ..Default::default()
        };
        let (ok, dropped) = split_protected(&cfg, &["a.rs".into(), "x/.env.local".into()]);
        assert_eq!(ok, vec!["a.rs"]);
        assert_eq!(dropped, vec!["x/.env.local"]);
    }

    #[test]
    fn prompt_includes_review_header_files_and_diff() {
        let p = build_prompt(
            true,
            "why?",
            &[("a.rs".into(), "fn x(){}".into())],
            Some("+1"),
        );
        assert!(p.contains("APPROVE") && p.contains("why?"));
        assert!(p.contains("### File: a.rs") && p.contains("```diff\n+1"));
        let q = build_prompt(false, "", &[], None);
        assert!(q.is_empty());
    }
}
