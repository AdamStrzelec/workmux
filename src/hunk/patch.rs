//! Building and applying patches for selected hunks (or line ranges inside them).

use anyhow::{Context, Result};
use std::io::Write;
use std::process::Stdio;

use super::diff::{DiffHunk, git_cmd};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyMode {
    Stage,
    Unstage,
    Discard,
}

/// Keep only changes inside the given 1-based line ranges of the hunk.
/// Excluded `+` lines are dropped, excluded `-` lines become context
/// (reverse patches flip that so discard/unstage stay valid).
pub fn slice_hunk(hunk: &DiffHunk, ranges: &[(usize, usize)], reverse: bool) -> Result<DiffHunk> {
    let in_range = |idx: usize| ranges.iter().any(|(s, e)| idx >= *s && idx <= *e);

    let mut lines = Vec::new();
    for (i, line) in hunk.lines.iter().enumerate() {
        let selected = in_range(i + 1);
        if let Some(rest) = line.strip_prefix('+') {
            if selected {
                lines.push(line.clone());
            } else if reverse {
                lines.push(format!(" {rest}"));
            }
        } else if let Some(rest) = line.strip_prefix('-') {
            if selected {
                lines.push(line.clone());
            } else if !reverse {
                lines.push(format!(" {rest}"));
            }
        } else {
            lines.push(line.clone());
        }
    }

    if !lines
        .iter()
        .any(|l| l.starts_with('+') || l.starts_with('-'))
    {
        anyhow::bail!("selected line range contains no changes");
    }

    let old_count = lines
        .iter()
        .filter(|l| l.starts_with('-') || l.starts_with(' '))
        .count();
    let new_count = lines
        .iter()
        .filter(|l| l.starts_with('+') || l.starts_with(' '))
        .count();
    let (old_start, new_start) = parse_hunk_starts(&hunk.header)?;
    let func_ctx = hunk
        .header
        .strip_prefix("@@ ")
        .and_then(|rest| rest.find("@@").map(|e| &rest[e + 2..]))
        .unwrap_or("");

    Ok(DiffHunk {
        header: format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@{func_ctx}"),
        lines,
        ..hunk.clone()
    })
}

fn parse_hunk_starts(header: &str) -> Result<(usize, usize)> {
    let content = header
        .trim_start_matches("@@ ")
        .split(" @@")
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid hunk header"))?;
    let mut parts = content.split_whitespace();
    let mut start = |sign: char| -> Result<usize> {
        parts
            .next()
            .and_then(|s| s.strip_prefix(sign))
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("cannot parse hunk header '{header}'"))
    };
    Ok((start('-')?, start('+')?))
}

pub fn build_patch(hunk: &DiffHunk) -> String {
    let mut patch = String::new();
    patch.push_str(&hunk.file_header);
    patch.push('\n');
    patch.push_str(&hunk.header);
    patch.push('\n');
    for line in &hunk.lines {
        patch.push_str(line);
        patch.push('\n');
    }
    patch
}

pub fn apply_patch(patch: &str, mode: ApplyMode) -> Result<()> {
    apply_patch_impl(patch, mode, None)
}

pub fn apply_patch_to_index(patch: &str, mode: ApplyMode, index: &std::path::Path) -> Result<()> {
    apply_patch_impl(patch, mode, Some(index))
}

fn apply_patch_impl(patch: &str, mode: ApplyMode, index: Option<&std::path::Path>) -> Result<()> {
    let mut cmd = git_cmd()?;
    cmd.arg("apply");
    if let Some(idx) = index {
        cmd.env("GIT_INDEX_FILE", idx);
    }
    match mode {
        ApplyMode::Stage => cmd.arg("--cached"),
        ApplyMode::Unstage => cmd.args(["--cached", "--reverse"]),
        ApplyMode::Discard => cmd.arg("--reverse"),
    };
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().context("failed to run git apply")?;
    child
        .stdin
        .take()
        .context("git apply stdin unavailable")?
        .write_all(patch.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git apply failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk() -> DiffHunk {
        DiffHunk {
            file: "f".into(),
            file_header: "--- a/f\n+++ b/f".into(),
            header: "@@ -3,3 +3,3 @@ fn x".into(),
            lines: vec![" c".into(), "-old".into(), "+new".into(), "+extra".into()],
            unsupported_metadata: None,
        }
    }

    #[test]
    fn slicing_drops_unselected_additions_and_keeps_deletion_as_context() {
        let sliced = slice_hunk(&hunk(), &[(3, 3)], false).unwrap();
        assert_eq!(sliced.lines, vec![" c", " old", "+new"]);
        assert_eq!(sliced.header, "@@ -3,2 +3,3 @@ fn x");
    }

    #[test]
    fn reverse_slicing_keeps_unselected_additions_as_context() {
        let sliced = slice_hunk(&hunk(), &[(2, 2)], true).unwrap();
        assert_eq!(sliced.lines, vec![" c", "-old", " new", " extra"]);
    }

    #[test]
    fn range_without_changes_is_rejected() {
        assert!(slice_hunk(&hunk(), &[(1, 1)], false).is_err());
    }

    #[test]
    fn build_patch_has_header_hunk_and_lines() {
        let patch = build_patch(&hunk());
        assert!(patch.starts_with("--- a/f\n+++ b/f\n@@ -3,3 +3,3 @@ fn x\n"));
        assert!(patch.ends_with("+extra\n"));
    }
}
