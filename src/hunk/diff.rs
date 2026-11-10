//! Parsing of `git diff` output into addressable hunks.

use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct DiffHunk {
    /// Display path. Prefers the new-side path, falls back to the old side for deletions.
    pub file: String,
    /// The `--- a/...` and `+++ b/...` lines.
    pub file_header: String,
    /// The `@@ -12,4 +12,6 @@ fn main` line.
    pub header: String,
    /// All lines of the hunk (context, `+`, `-`).
    pub lines: Vec<String>,
    /// Preamble metadata that hunk-level operations cannot express (rename, mode change...).
    pub unsupported_metadata: Option<String>,
}

const DIFF_FORMAT_ARGS: &[&str] = &[
    "--no-color",
    "--no-ext-diff",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// A hardened, non-interactive git process pinned to the current repository
/// (hooks, GPG signing, pagers and ambient GIT_* overrides are disabled).
pub fn git_cmd() -> Result<Command> {
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    crate::git::unattended_git(Some(&cwd))
}

pub fn run_git(cmd: &mut Command) -> Result<String> {
    let output = cmd.output().context("failed to run git")?;
    if !output.status.success() {
        anyhow::bail!(
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn git_diff(staged: bool, file: Option<&str>) -> Result<String> {
    let mut cmd = git_cmd()?;
    cmd.arg("diff").args(DIFF_FORMAT_ARGS);
    if staged {
        cmd.arg("--cached");
    }
    if let Some(f) = file {
        cmd.arg("--").arg(f);
    }
    run_git(&mut cmd)
}

pub fn git_diff_commit(commit: &str, file: Option<&str>) -> Result<String> {
    let mut cmd = git_cmd()?;
    cmd.args(["show", "--pretty="])
        .args(DIFF_FORMAT_ARGS)
        .arg(commit);
    if let Some(f) = file {
        cmd.arg("--").arg(f);
    }
    run_git(&mut cmd)
}

fn strip_diff_prefix(line: &str) -> &str {
    line.strip_prefix("--- a/")
        .or_else(|| line.strip_prefix("+++ b/"))
        .or_else(|| line.strip_prefix("--- /"))
        .or_else(|| line.strip_prefix("+++ /"))
        .or_else(|| line.strip_prefix("--- "))
        .or_else(|| line.strip_prefix("+++ "))
        .unwrap_or(line)
}

const UNSUPPORTED_PREAMBLE_PREFIXES: &[&str] = &[
    "rename from ",
    "rename to ",
    "copy from ",
    "copy to ",
    "old mode ",
    "new mode ",
    "similarity index ",
    "dissimilarity index ",
];

#[derive(Default)]
struct Current {
    old_file: String,
    new_file: String,
    file_header: String,
    header: Option<String>,
    lines: Vec<String>,
    unsupported: Option<String>,
}

impl Current {
    fn flush(&mut self, out: &mut Vec<DiffHunk>) {
        if let Some(header) = self.header.take() {
            out.push(DiffHunk {
                file: display_file(&self.old_file, &self.new_file),
                file_header: self.file_header.clone(),
                header,
                lines: std::mem::take(&mut self.lines),
                unsupported_metadata: self.unsupported.clone(),
            });
        }
    }
}

pub fn parse_diff(input: &str) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut cur = Current::default();

    for line in input.lines() {
        if line.starts_with("diff --git") {
            cur.flush(&mut hunks);
            cur = Current::default();
            continue;
        }
        if cur.header.is_none()
            && cur.unsupported.is_none()
            && let Some(prefix) = UNSUPPORTED_PREAMBLE_PREFIXES
                .iter()
                .find(|p| line.starts_with(*p))
        {
            cur.unsupported = Some(prefix.trim().to_string());
        }

        if cur.header.is_none() && line.starts_with("--- ") {
            cur.file_header = line.to_string();
            cur.old_file = strip_diff_prefix(line).to_string();
        } else if cur.header.is_none() && line.starts_with("+++ ") {
            cur.file_header.push('\n');
            cur.file_header.push_str(line);
            cur.new_file = strip_diff_prefix(line).to_string();
        } else if line.starts_with("@@ ") {
            cur.flush(&mut hunks);
            cur.header = Some(line.to_string());
        } else if cur.header.is_some() {
            cur.lines.push(line.to_string());
        }
    }
    cur.flush(&mut hunks);
    hunks
}

fn display_file(old: &str, new: &str) -> String {
    if new == "dev/null" || new.is_empty() {
        old.to_string()
    } else {
        new.to_string()
    }
}

pub fn check_supported(hunk: &DiffHunk, id: &str) -> Result<()> {
    if let Some(metadata) = &hunk.unsupported_metadata {
        anyhow::bail!(
            "hunk {id} involves '{metadata}' which is not supported for hunk-level operations"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@ fn x\n-one\n+uno\n two\n@@ -10 +10 @@\n-ten\n+diez\ndiff --git a/b.txt b/b.txt\nnew file mode 100644\n--- /dev/null\n+++ b/b.txt\n@@ -0,0 +1 @@\n+hi\n";

    #[test]
    fn parses_multiple_hunks_and_files() {
        let hunks = parse_diff(SAMPLE);
        assert_eq!(hunks.len(), 3);
        assert_eq!(hunks[0].file, "a.txt");
        assert_eq!(hunks[0].lines, vec!["-one", "+uno", " two"]);
        assert_eq!(hunks[1].header, "@@ -10 +10 @@");
        assert_eq!(hunks[2].file, "b.txt");
        assert!(hunks[2].unsupported_metadata.is_none());
    }

    #[test]
    fn flags_rename_as_unsupported() {
        let diff = "diff --git a/a b/c\nsimilarity index 90%\nrename from a\nrename to c\n--- a/a\n+++ b/c\n@@ -1 +1 @@\n-x\n+y\n";
        let hunks = parse_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert!(check_supported(&hunks[0], "abc").is_err());
    }

    #[test]
    fn content_lines_starting_with_dashes_stay_in_hunk() {
        let diff =
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1,2 +1,2 @@\n--- old\n+++ new\n ctx\n";
        let hunks = parse_diff(diff);
        assert_eq!(hunks[0].lines, vec!["--- old", "+++ new", " ctx"]);
    }
}
