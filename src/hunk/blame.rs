//! `git blame` annotations for hunk lines.

use anyhow::Result;

use super::diff::{DiffHunk, git_cmd};

const UNCOMMITTED: &str = "0000000";
const UNKNOWN: &str = "-------";

/// Which two states of the file a diff compares.
pub enum Source<'a> {
    /// Index vs working tree. The old side is the index, which only matches
    /// `HEAD` when the file has no staged changes.
    Unstaged,
    /// `HEAD` vs index. The new side has no commit to blame yet.
    Staged,
    /// `<commit>^` vs `<commit>`.
    Commit(&'a str),
}

/// Parse `@@ -a,b +c,d @@` into `(old_from, old_count, new_from, new_count)`.
fn parse_header(header: &str) -> Option<(usize, usize, usize, usize)> {
    let rest = header.strip_prefix("@@ ")?;
    let range = &rest[..rest.find(" @@")?];
    let mut parts = range.split_whitespace();
    let (of, oc) = parse_range(parts.next()?.strip_prefix('-')?)?;
    let (nf, nc) = parse_range(parts.next()?.strip_prefix('+')?)?;
    Some((of, oc, nf, nc))
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    match s.split_once(',') {
        Some((from, count)) => Some((from.parse().ok()?, count.parse().ok()?)),
        None => Some((s.parse().ok()?, 1)),
    }
}

/// 7-char commit hashes, one per line of `file[from..from+count]` at `rev`
/// (`None` blames the working tree). Empty when blame is unavailable.
fn blame(file: &str, from: usize, count: usize, rev: Option<&str>) -> Result<Vec<String>> {
    if count == 0 || from == 0 {
        return Ok(Vec::new());
    }
    let mut cmd = git_cmd()?;
    cmd.args([
        "blame",
        "--line-porcelain",
        "-L",
        &format!("{from},+{count}"),
    ]);
    if let Some(rev) = rev {
        cmd.arg(rev);
    }
    cmd.arg("--").arg(file);
    let output = cmd.output()?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(parse_porcelain(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_porcelain(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.starts_with('\t'))
        .filter_map(|line| {
            let token = line.split_whitespace().next()?.trim_start_matches('^');
            let is_hash =
                token.len() >= 40 && token.chars().take(40).all(|c| c.is_ascii_hexdigit());
            is_hash.then(|| token[..7].to_string())
        })
        .collect()
}

fn has_staged_changes(file: &str) -> bool {
    git_cmd()
        .and_then(|mut cmd| {
            cmd.args(["diff", "--cached", "--quiet", "--"])
                .arg(file)
                .status()
                .map_err(Into::into)
        })
        .map(|status| !status.success())
        .unwrap_or(true)
}

/// Blame hash for every hunk line, in order. `None` for lines without a
/// blame column (such as `\ No newline at end of file`).
pub fn blame_hunk(hunk: &DiffHunk, source: &Source) -> Vec<Option<String>> {
    let (old_from, old_count, new_from, new_count) =
        parse_header(&hunk.header).unwrap_or((0, 0, 0, 0));
    let old_exists = !hunk
        .file_header
        .lines()
        .any(|l| l.starts_with("--- /dev/null"));
    let new_exists = !hunk
        .file_header
        .lines()
        .any(|l| l.starts_with("+++ /dev/null"));

    let (old_rev, new_rev, old_ok, new_ok): (String, Option<&str>, bool, bool) = match source {
        Source::Unstaged => ("HEAD".into(), None, !has_staged_changes(&hunk.file), true),
        Source::Staged => ("HEAD".into(), None, true, false),
        Source::Commit(c) => (format!("{c}^"), Some(c), true, true),
    };

    let old = if old_exists && old_ok {
        blame(&hunk.file, old_from, old_count, Some(&old_rev)).unwrap_or_default()
    } else {
        Vec::new()
    };
    let new = if new_exists && new_ok {
        blame(&hunk.file, new_from, new_count, new_rev).unwrap_or_default()
    } else {
        Vec::new()
    };
    annotate(&hunk.lines, &old, &new, matches!(source, Source::Staged))
}

/// Pair hunk lines with blame hashes. Context lines prefer the new side;
/// removed lines use the old side; added lines use the new side, or
/// `0000000` when the new side is not committed.
fn annotate(
    lines: &[String],
    old: &[String],
    new: &[String],
    new_uncommitted: bool,
) -> Vec<Option<String>> {
    let (mut o, mut n) = (0usize, 0usize);
    lines
        .iter()
        .map(|line| {
            let old_hash = || old.get(o).cloned().unwrap_or_else(|| UNKNOWN.into());
            let new_hash = || new.get(n).cloned().unwrap_or_else(|| UNKNOWN.into());
            let hash = match line.chars().next() {
                Some(' ') => {
                    let h = if new.is_empty() {
                        old_hash()
                    } else {
                        new_hash()
                    };
                    o += 1;
                    n += 1;
                    h
                }
                Some('-') => {
                    let h = old_hash();
                    o += 1;
                    h
                }
                Some('+') => {
                    let h = if new_uncommitted {
                        UNCOMMITTED.into()
                    } else {
                        new_hash()
                    };
                    n += 1;
                    h
                }
                _ => return None,
            };
            Some(hash)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_headers() {
        assert_eq!(
            parse_header("@@ -10,3 +10,4 @@ fn main"),
            Some((10, 3, 10, 4))
        );
        assert_eq!(parse_header("@@ -5 +5 @@"), Some((5, 1, 5, 1)));
        assert_eq!(parse_header("@@ -0,0 +1,3 @@"), Some((0, 0, 1, 3)));
        assert_eq!(parse_header("nope"), None);
    }

    #[test]
    fn parses_porcelain_and_skips_content_lines() {
        let hash = "a".repeat(40);
        let boundary = format!("^{}", "b".repeat(39) + "c");
        let text = format!("{hash} 1 1 1\nauthor x\n\t{hash} fake\n{boundary} 2 2\n\tbody\n");
        assert_eq!(parse_porcelain(&text), vec!["aaaaaaa", "bbbbbbb"]);
    }

    #[test]
    fn annotates_context_removed_and_added() {
        let lines = s(&[" ctx", "-gone", "+new", "\\ No newline at end of file"]);
        let old = s(&["1111111", "2222222"]);
        let new = s(&["3333333", "4444444"]);
        let out = annotate(&lines, &old, &new, false);
        assert_eq!(
            out,
            vec![
                Some("3333333".into()),
                Some("2222222".into()),
                Some("4444444".into()),
                None
            ]
        );
    }

    #[test]
    fn staged_added_lines_are_uncommitted_and_context_uses_old_side() {
        let lines = s(&[" ctx", "+new"]);
        let out = annotate(&lines, &s(&["1111111"]), &[], true);
        assert_eq!(out, vec![Some("1111111".into()), Some(UNCOMMITTED.into())]);
    }

    #[test]
    fn missing_blame_is_marked_unknown() {
        let out = annotate(&s(&["-gone"]), &[], &[], false);
        assert_eq!(out, vec![Some(UNKNOWN.into())]);
    }
}
