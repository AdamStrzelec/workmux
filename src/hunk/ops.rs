use anyhow::{Context, Result};
use std::io::Write;
use std::process::Stdio;

use super::blame::{Source, blame_hunk};
use super::diff::{
    DiffHunk, check_supported, git_cmd, git_diff, git_diff_commit, parse_diff, run_git,
};
use super::id::assign_ids;
use super::patch::{ApplyMode, apply_patch, apply_patch_to_index, build_patch, slice_hunk};

/// A resolved selection: hunk ID, optional line ranges, the hunk itself.
type Entry<'a> = (String, Vec<(usize, usize)>, &'a DiffHunk);

const MAX_PREVIEW_LINES: usize = 4;

fn func_context(header: &str) -> &str {
    header
        .strip_prefix("@@ ")
        .and_then(|rest| rest.find("@@ ").map(|e| rest[e + 3..].trim()))
        .unwrap_or("")
}

pub fn list(
    staged: bool,
    file: Option<&str>,
    commit: Option<&str>,
    full: bool,
    blame: bool,
) -> Result<()> {
    let output = match commit {
        Some(c) => git_diff_commit(c, file)?,
        None => git_diff(staged, file)?,
    };
    let hunks = parse_diff(&output);
    for (id, hunk) in assign_ids(&hunks) {
        let adds = hunk.lines.iter().filter(|l| l.starts_with('+')).count();
        let dels = hunk.lines.iter().filter(|l| l.starts_with('-')).count();
        let ctx = func_context(&hunk.header);
        let ctx = if ctx.is_empty() {
            String::new()
        } else {
            format!(" {ctx}")
        };
        println!("{id} {}{ctx} (+{adds} -{dels})", hunk.file);

        if blame {
            let source = match (commit, staged) {
                (Some(c), _) => Source::Commit(c),
                (None, true) => Source::Staged,
                (None, false) => Source::Unstaged,
            };
            for (line, hash) in hunk.lines.iter().zip(blame_hunk(hunk, &source)) {
                match hash {
                    Some(hash) => println!("  {hash} {line}"),
                    None => println!("  {line}"),
                }
            }
        } else if full {
            print_numbered(hunk);
        } else {
            let changed: Vec<&String> = hunk
                .lines
                .iter()
                .filter(|l| l.starts_with('+') || l.starts_with('-'))
                .collect();
            for line in changed.iter().take(MAX_PREVIEW_LINES) {
                println!("  {line}");
            }
            if changed.len() > MAX_PREVIEW_LINES {
                println!("  ... (+{} more lines)", changed.len() - MAX_PREVIEW_LINES);
            }
        }
        println!();
    }
    Ok(())
}

fn print_numbered(hunk: &DiffHunk) {
    let width = hunk.lines.len().to_string().len();
    for (i, line) in hunk.lines.iter().enumerate() {
        println!("{:>width$}:{line}", i + 1);
    }
}

pub fn show(id: &str, commit: Option<&str>) -> Result<()> {
    let hunk = match commit {
        Some(c) => find(id, &parse_diff(&git_diff_commit(c, None)?))
            .with_context(|| format!("hunk {id} not found in commit {c}"))?,
        None => find(id, &parse_diff(&git_diff(false, None)?))
            .or_else(|_| find(id, &parse_diff(&git_diff(true, None)?)))?,
    };
    println!("{}", hunk.header);
    print_numbered(&hunk);
    Ok(())
}

fn find(id: &str, hunks: &[DiffHunk]) -> Result<DiffHunk> {
    assign_ids(hunks)
        .into_iter()
        .find(|(hunk_id, _)| hunk_id == id)
        .map(|(_, h)| h.clone())
        .ok_or_else(|| anyhow::anyhow!("hunk {id} not found (re-run 'workmux hunk list')"))
}

/// Parse `id`, `id:5`, `id:1-11`, `id:2,5-6,34`. Empty ranges mean the whole hunk.
fn parse_id_range(raw: &str) -> Result<(&str, Vec<(usize, usize)>)> {
    let Some((id, range_str)) = raw.split_once(':') else {
        return Ok((raw, Vec::new()));
    };
    let mut ranges = Vec::new();
    for part in range_str
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        let parse = |s: &str| -> Result<usize> {
            s.parse()
                .map_err(|_| anyhow::anyhow!("invalid line number in '{raw}'"))
        };
        let (start, end) = match part.split_once('-') {
            Some((a, b)) => (parse(a)?, parse(b)?),
            None => (parse(part)?, parse(part)?),
        };
        if start == 0 || start > end {
            anyhow::bail!("range must be 1-based and start <= end in '{raw}'");
        }
        ranges.push((start, end));
    }
    Ok((id, ranges))
}

pub fn parse_line_range(s: &str) -> Result<(usize, usize), String> {
    parse_id_range(&format!("x:{s}"))
        .map_err(|e| e.to_string())
        .and_then(|(_, r)| r.first().copied().ok_or_else(|| "empty range".to_string()))
}

struct Resolved<'a> {
    /// (id, line ranges, hunk)
    entries: Vec<Entry<'a>>,
}

impl Resolved<'_> {
    fn ids(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(id, _, _)| id)
    }

    fn patch(&self, reverse: bool) -> Result<String> {
        let mut out = String::new();
        for (_, ranges, hunk) in &self.entries {
            if ranges.is_empty() {
                out.push_str(&build_patch(hunk));
            } else {
                out.push_str(&build_patch(&slice_hunk(hunk, ranges, reverse)?));
            }
        }
        Ok(out)
    }
}

fn resolve<'a>(ids: &[String], identified: &'a [(String, &'a DiffHunk)]) -> Result<Resolved<'a>> {
    if ids.is_empty() {
        anyhow::bail!("no hunk IDs given");
    }
    let mut entries: Vec<Entry<'_>> = Vec::new();
    for raw in ids {
        let (id, ranges) = parse_id_range(raw)?;
        if let Some(entry) = entries.iter_mut().find(|(eid, _, _)| eid == id) {
            entry.1.extend(ranges);
            continue;
        }
        let (_, hunk) = identified
            .iter()
            .find(|(hunk_id, _)| hunk_id == id)
            .ok_or_else(|| anyhow::anyhow!("hunk {id} not found (re-run 'workmux hunk list')"))?;
        check_supported(hunk, id)?;
        entries.push((id.to_string(), ranges, hunk));
    }
    Ok(Resolved { entries })
}

fn confirm(ids: impl IntoIterator<Item = impl std::fmt::Display>) {
    for id in ids {
        eprintln!("{id}");
    }
}

fn apply(ids: &[String], mode: ApplyMode, lines: Option<(usize, usize)>) -> Result<()> {
    if lines.is_some() && ids.len() != 1 {
        anyhow::bail!("--lines requires exactly one hunk ID");
    }
    if let Some(id) = ids.iter().find(|id| id.contains(':')) {
        anyhow::bail!("inline ranges are not supported here: {id}; use --lines START-END");
    }
    let output = git_diff(mode == ApplyMode::Unstage, None)?;
    let hunks = parse_diff(&output);
    let identified = assign_ids(&hunks);
    let mut resolved = resolve(ids, &identified)?;
    if let Some(range) = lines {
        resolved.entries[0].1 = vec![range];
    }
    let reverse = mode != ApplyMode::Stage;
    apply_patch(&resolved.patch(reverse)?, mode)?;
    confirm(resolved.ids());
    Ok(())
}

/// Reverse-apply hunks of an earlier commit onto the working tree.
pub fn undo(ids: &[String], from: &str, lines: Option<(usize, usize)>) -> Result<()> {
    if lines.is_some() && ids.len() != 1 {
        anyhow::bail!("--lines requires exactly one hunk ID");
    }
    if let Some(id) = ids.iter().find(|id| id.contains(':')) {
        anyhow::bail!("inline ranges are not supported here: {id}; use --lines START-END");
    }
    let hunks = parse_diff(&git_diff_commit(from, None)?);
    let identified = assign_ids(&hunks);
    let mut resolved =
        resolve(ids, &identified).map_err(|e| anyhow::anyhow!("{e} in commit {from}"))?;
    if let Some(range) = lines {
        resolved.entries[0].1 = vec![range];
    }
    apply_patch(&resolved.patch(true)?, ApplyMode::Discard)
        .context("patch no longer applies cleanly to the working tree")?;
    confirm(resolved.ids());
    Ok(())
}

pub fn stage(ids: &[String], lines: Option<(usize, usize)>) -> Result<()> {
    apply(ids, ApplyMode::Stage, lines)
}

pub fn unstage(ids: &[String], lines: Option<(usize, usize)>) -> Result<()> {
    apply(ids, ApplyMode::Unstage, lines)
}

pub fn discard(ids: &[String], lines: Option<(usize, usize)>) -> Result<()> {
    apply(ids, ApplyMode::Discard, lines)
}

pub(super) fn require_clean_index() -> Result<()> {
    let status = git_cmd()?
        .args(["diff", "--cached", "--quiet"])
        .status()
        .context("failed to check staged changes")?;
    if !status.success() {
        anyhow::bail!("index already contains staged changes; commit or unstage them first");
    }
    Ok(())
}

/// Branches that hunk commits must never target: `main`, `master` and the repo's default branch.
/// `hunk` runs git without hooks, so the check lives here instead of in a pre-commit hook.
pub(super) fn is_protected(branch: &str) -> bool {
    if matches!(branch, "main" | "master") {
        return true;
    }
    crate::git::get_default_branch().is_ok_and(|default| default == branch)
}

fn require_unprotected(branch: &str) -> Result<()> {
    if is_protected(branch) {
        anyhow::bail!(
            "refusing to commit hunks to protected branch '{branch}'; \
             work on a feature branch and merge after review"
        );
    }
    Ok(())
}

/// Stage the selected hunks and commit them. Restores the index if the commit fails.
pub fn commit(ids: &[String], message: &str) -> Result<()> {
    let head = git_cmd()?
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .output()
        .context("failed to check current branch")?;
    if head.status.success() {
        require_unprotected(String::from_utf8_lossy(&head.stdout).trim())?;
    }
    require_clean_index()?;
    let output = git_diff(false, None)?;
    let hunks = parse_diff(&output);
    let identified = assign_ids(&hunks);
    let resolved = resolve(ids, &identified)?;
    let patch = resolved.patch(false)?;

    apply_patch(&patch, ApplyMode::Stage)?;
    let result = git_cmd()?
        .args(["commit", "-m", message])
        .output()
        .context("failed to run git commit")?;
    if !result.status.success() {
        let _ = apply_patch(&patch, ApplyMode::Unstage);
        anyhow::bail!(
            "git commit failed: {}{}",
            String::from_utf8_lossy(&result.stdout).trim(),
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }
    confirm(resolved.ids());
    Ok(())
}

/// Commit the selected working-tree hunks to another branch without checking it out,
/// then remove them from the working tree.
pub fn commit_to(branch: &str, ids: &[String], message: &str) -> Result<()> {
    require_unprotected(branch)?;
    require_clean_index()?;

    let target_ref = format!("refs/heads/{branch}");
    let target_sha = run_git(git_cmd()?.args(["rev-parse", "--verify", &target_ref]))
        .with_context(|| format!("branch '{branch}' not found"))?
        .trim()
        .to_string();

    let current = git_cmd()?
        .args(["symbolic-ref", "--quiet", "HEAD"])
        .output()
        .context("failed to check current branch")?;
    if current.status.success() && String::from_utf8_lossy(&current.stdout).trim() == target_ref {
        anyhow::bail!("target branch '{branch}' is currently checked out; use 'commit' instead");
    }

    let output = git_diff(false, None)?;
    let hunks = parse_diff(&output);
    let identified = assign_ids(&hunks);
    let resolved = resolve(ids, &identified)?;
    let stage_patch = resolved.patch(false)?;
    let discard_patch = resolved.patch(true)?;

    let git_dir = run_git(git_cmd()?.args(["rev-parse", "--git-dir"]))?;
    let tmp_index = std::path::PathBuf::from(git_dir.trim())
        .join(format!("workmux-hunk-index-{}", std::process::id()));

    let result = commit_with_index(
        branch,
        &target_ref,
        &target_sha,
        &stage_patch,
        message,
        &tmp_index,
    );
    let _ = std::fs::remove_file(&tmp_index);
    let sha = result?;
    let short = &sha[..7.min(sha.len())];

    apply_patch(&discard_patch, ApplyMode::Discard).with_context(|| {
        format!(
            "committed to {branch} ({short}) but failed to discard local hunks; \
             the changes are still in your working tree"
        )
    })?;
    eprintln!("committed to {branch}: {short}");
    confirm(resolved.ids());
    Ok(())
}

fn commit_with_index(
    branch: &str,
    target_ref: &str,
    target_sha: &str,
    patch: &str,
    message: &str,
    tmp_index: &std::path::Path,
) -> Result<String> {
    let status = git_cmd()?
        .env("GIT_INDEX_FILE", tmp_index)
        .args(["read-tree", target_sha])
        .status()
        .context("failed to read target branch tree")?;
    if !status.success() {
        anyhow::bail!("git read-tree failed for {branch}");
    }

    apply_patch_to_index(patch, ApplyMode::Stage, tmp_index).with_context(|| {
        format!("failed to apply patch to branch '{branch}'; changes may be incompatible with it")
    })?;

    let tree = run_git(
        git_cmd()?
            .env("GIT_INDEX_FILE", tmp_index)
            .arg("write-tree"),
    )?;

    let mut child = git_cmd()?
        .args(["commit-tree", tree.trim(), "-p", target_sha, "-F", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to run git commit-tree")?;
    child
        .stdin
        .take()
        .context("git commit-tree stdin unavailable")?
        .write_all(message.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        anyhow::bail!(
            "git commit-tree failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();

    // Compare-and-swap so a concurrent move of the branch is not overwritten.
    run_git(git_cmd()?.args(["update-ref", target_ref, &sha, target_sha]))
        .context("git update-ref failed (branch may have moved)")?;
    Ok(sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_ranges() {
        assert_eq!(parse_id_range("abc").unwrap(), ("abc", vec![]));
        assert_eq!(parse_id_range("abc:5").unwrap(), ("abc", vec![(5, 5)]));
        assert_eq!(
            parse_id_range("abc:2,5-6,34").unwrap(),
            ("abc", vec![(2, 2), (5, 6), (34, 34)])
        );
    }

    #[test]
    fn rejects_bad_ranges() {
        assert!(parse_id_range("abc:0").is_err());
        assert!(parse_id_range("abc:6-5").is_err());
        assert!(parse_id_range("abc:x").is_err());
    }

    #[test]
    fn line_range_flag_parser() {
        assert_eq!(parse_line_range("5-30").unwrap(), (5, 30));
        assert_eq!(parse_line_range("7").unwrap(), (7, 7));
        assert!(parse_line_range("0").is_err());
    }
}
