//! History restructuring: fold, squash, move and split commits.
//!
//! `fold` and `move` rebase with a todo list that workmux writes itself (no
//! editor). `squash` and `split` build the new commits with git plumbing, so
//! the working tree is never touched.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use super::diff::{DiffHunk, check_supported, git_cmd, git_diff_commit, parse_diff, run_git};
use super::history::{
    commits_after, git_out, has_staged_changes, rebase_in_progress, resolve_rewrite_target, short,
};
use super::id::assign_ids;
use super::ops::parse_id_range;
use super::patch::{ApplyMode, apply_patch_to_index, build_patch, slice_hunk_with_state};

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

// ---------------------------------------------------------------------------
// rebase todo editing
// ---------------------------------------------------------------------------

/// `pick` lines for `commits`, oldest first, with `sources` removed and re-added
/// as `fixup` right after `target` (in their original order).
fn fold_todo(commits: &[String], target: &str, sources: &[String]) -> Result<Vec<String>> {
    let target_idx = commits
        .iter()
        .position(|c| c == target)
        .context("target commit not found in the rebase range")?;
    let mut folded = Vec::new();
    let mut todo = Vec::new();
    for (i, sha) in commits.iter().enumerate() {
        if sources.contains(sha) {
            if i <= target_idx {
                anyhow::bail!("{} is not newer than the target", short(sha));
            }
            folded.push(format!("fixup {sha}"));
        } else {
            todo.push(format!("pick {sha}"));
        }
    }
    if folded.len() != sources.len() {
        anyhow::bail!("a source commit is not between the target and HEAD");
    }
    let insert_at = todo
        .iter()
        .position(|l| l == &format!("pick {target}"))
        .context("target commit not found in todo")?
        + 1;
    todo.splice(insert_at..insert_at, folded);
    Ok(todo)
}

/// `pick` lines with `source` moved directly after (or before) `target`.
fn move_todo(commits: &[String], source: &str, target: &str, after: bool) -> Result<Vec<String>> {
    let mut rest: Vec<&String> = commits.iter().filter(|c| c.as_str() != source).collect();
    let target_idx = rest
        .iter()
        .position(|c| c.as_str() == target)
        .context("target commit not found in the rebase range")?;
    if !commits.iter().any(|c| c == source) {
        anyhow::bail!("source commit not found in the rebase range");
    }
    let source = commits.iter().find(|c| c.as_str() == source).unwrap();
    rest.insert(if after { target_idx + 1 } else { target_idx }, source);
    Ok(rest.iter().map(|c| format!("pick {c}")).collect())
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn git_dir() -> Result<PathBuf> {
    Ok(PathBuf::from(git_out(&["rev-parse", "--git-dir"])?))
}

fn range_commits(oldest: &str) -> Result<Vec<String>> {
    let has_parent = git_cmd()?
        .args(["rev-parse", "--verify", "--quiet", &format!("{oldest}^")])
        .output()?
        .status
        .success();
    let range = if has_parent {
        format!("{oldest}^..HEAD")
    } else {
        "HEAD".to_string()
    };
    Ok(git_out(&["rev-list", "--reverse", "--topo-order", &range])?
        .lines()
        .map(String::from)
        .collect())
}

/// Rebase from `oldest` with a todo list written by workmux.
fn rebase_with_todo(oldest: &str, todo: &[String], what: &str) -> Result<()> {
    let todo_file = git_dir()?.join(format!("workmux-hunk-todo-{}", std::process::id()));
    std::fs::write(&todo_file, todo.join("\n") + "\n").context("failed to write rebase todo")?;
    let has_parent = git_cmd()?
        .args(["rev-parse", "--verify", "--quiet", &format!("{oldest}^")])
        .output()?
        .status
        .success();
    let mut cmd = git_cmd()?;
    cmd.args(["rebase", "-i", "--autostash"]);
    if has_parent {
        cmd.arg(format!("{oldest}~1"));
    } else {
        cmd.arg("--root");
    }
    cmd.env(
        "GIT_SEQUENCE_EDITOR",
        format!("cp {}", shell_quote(&todo_file)),
    );
    let output = cmd.output();
    let _ = std::fs::remove_file(&todo_file);
    let output = output.context("failed to run git rebase")?;
    if output.status.success() {
        return Ok(());
    }
    rebase_failed(what, &String::from_utf8_lossy(&output.stderr))
}

fn rebase_failed(what: &str, stderr: &str) -> Result<()> {
    if rebase_in_progress()? {
        anyhow::bail!(
            "conflict while {what}; resolve it and run 'git rebase --continue', \
             or abort with 'git rebase --abort'\n{}",
            stderr.trim()
        );
    }
    anyhow::bail!("rebase failed: {}", stderr.trim())
}

fn is_ancestor(older: &str, newer: &str) -> Result<bool> {
    Ok(git_cmd()?
        .args(["merge-base", "--is-ancestor", older, newer])
        .status()
        .context("failed to check ancestry")?
        .success())
}

fn log_order(oldest: &str) -> Result<()> {
    let range = if git_cmd()?
        .args(["rev-parse", "--verify", "--quiet", &format!("{oldest}^")])
        .output()?
        .status
        .success()
    {
        format!("{oldest}~1..HEAD")
    } else {
        "HEAD".to_string()
    };
    eprintln!("new order:");
    for line in git_out(&["log", "--oneline", "--reverse", &range])?.lines() {
        eprintln!("  {line}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// fold / move
// ---------------------------------------------------------------------------

/// Fold existing commits (default: HEAD) into an earlier target commit.
pub fn fold(target: &str, sources: &[String], allow_pushed: bool) -> Result<()> {
    if has_staged_changes()? {
        anyhow::bail!(
            "index has staged changes; 'fold' folds existing commits. \
             Use 'workmux hunk amend {target}' to fold staged changes"
        );
    }
    let target_sha = resolve_rewrite_target(target, allow_pushed, false)?;
    let mut source_shas: Vec<String> = Vec::new();
    for source in if sources.is_empty() {
        vec!["HEAD".to_string()]
    } else {
        sources.to_vec()
    } {
        let sha = git_out(&["rev-parse", "--verify", &format!("{source}^{{commit}}")])
            .with_context(|| format!("commit '{source}' not found"))?;
        if sha == target_sha {
            anyhow::bail!("target and source are the same commit");
        }
        if !is_ancestor(&target_sha, &sha)? || !is_ancestor(&sha, "HEAD")? {
            anyhow::bail!("{source} must be between {target} and HEAD");
        }
        if !source_shas.contains(&sha) {
            source_shas.push(sha);
        }
    }
    let commits = range_commits(&target_sha)?;
    let todo = fold_todo(&commits, &target_sha, &source_shas)?;
    rebase_with_todo(
        &target_sha,
        &todo,
        &format!("folding into {}", short(&target_sha)),
    )?;
    eprintln!(
        "folded {} commit(s) into {}",
        source_shas.len(),
        git_out(&[
            "log",
            "-1",
            "--format=%h %s",
            &format!("HEAD~{}", commits.len() - 1 - source_shas.len())
        ])
        .unwrap_or_default()
    );
    Ok(())
}

/// Where `move` puts the commit.
pub enum MoveTo<'a> {
    After(&'a str),
    Before(&'a str),
    End,
}

/// Move a commit to a different position in history.
pub fn move_commit(commit: &str, to: MoveTo, allow_pushed: bool) -> Result<()> {
    let source = git_out(&["rev-parse", "--verify", &format!("{commit}^{{commit}}")])
        .with_context(|| format!("commit '{commit}' not found"))?;
    let head = git_out(&["rev-parse", "HEAD"])?;
    let (target, after) = match to {
        MoveTo::End => (head.clone(), true),
        MoveTo::After(r) | MoveTo::Before(r) => (
            git_out(&["rev-parse", "--verify", &format!("{r}^{{commit}}")])
                .with_context(|| format!("commit '{r}' not found"))?,
            matches!(to, MoveTo::After(_)),
        ),
    };
    if source == target {
        if matches!(to, MoveTo::End) {
            eprintln!("commit is already at HEAD");
            return Ok(());
        }
        anyhow::bail!("source and target are the same commit");
    }
    let oldest = if is_ancestor(&source, &target)? {
        source.clone()
    } else {
        target.clone()
    };
    for sha in [&source, &target] {
        if !is_ancestor(sha, "HEAD")? {
            anyhow::bail!("{} is not in the current branch", short(sha));
        }
    }
    resolve_rewrite_target(&oldest, allow_pushed, false)?;
    let commits = range_commits(&oldest)?;
    let subject = git_out(&["log", "-1", "--format=%s", &source])?;
    let todo = move_todo(&commits, &source, &target, after)?;
    rebase_with_todo(&oldest, &todo, &format!("moving {}", short(&source)))?;
    eprintln!("moved '{subject}'");
    log_order(&oldest_after_rebase(&commits)?)
}

/// After a rebase the SHAs change; the range start is the commit at the same depth.
fn oldest_after_rebase(commits: &[String]) -> Result<String> {
    git_out(&["rev-parse", &format!("HEAD~{}", commits.len() - 1)])
}

// ---------------------------------------------------------------------------
// plumbing helpers
// ---------------------------------------------------------------------------

struct Author {
    name: String,
    email: String,
    date: String,
}

fn author_of(sha: &str) -> Result<Author> {
    let out = git_out(&["log", "-1", "--format=%an%x00%ae%x00%aI", sha])?;
    let mut parts = out.split('\0');
    Ok(Author {
        name: parts.next().unwrap_or_default().to_string(),
        email: parts.next().unwrap_or_default().to_string(),
        date: parts.next().unwrap_or_default().to_string(),
    })
}

fn commit_tree(
    tree: &str,
    parents: &[String],
    message: &str,
    author: Option<&Author>,
) -> Result<String> {
    let mut cmd = git_cmd()?;
    cmd.arg("commit-tree").arg(tree);
    for parent in parents {
        cmd.args(["-p", parent]);
    }
    cmd.args(["-F", "-"]);
    if let Some(a) = author {
        cmd.env("GIT_AUTHOR_NAME", &a.name)
            .env("GIT_AUTHOR_EMAIL", &a.email)
            .env("GIT_AUTHOR_DATE", &a.date);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().context("failed to run git commit-tree")?;
    child
        .stdin
        .take()
        .context("git commit-tree stdin unavailable")?
        .write_all(format!("{}\n", message.trim_end()).as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git commit-tree failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Point the current branch at `new`, but only if it still points at `old`.
fn move_head(new: &str, old: &str, reason: &str) -> Result<()> {
    run_git(git_cmd()?.args(["update-ref", "-m", reason, "HEAD", new, old]))
        .context("failed to update the branch (it moved while rewriting?)")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// squash
// ---------------------------------------------------------------------------

/// Combine `commit` through HEAD into one commit with the final tree.
pub fn squash(
    commit: &str,
    message: &[String],
    force: bool,
    preserve_author: bool,
    allow_pushed: bool,
) -> Result<()> {
    if message.is_empty() {
        anyhow::bail!("no message given; use -m");
    }
    let target = resolve_rewrite_target(commit, allow_pushed, force)?;
    let count = commits_after(&target)? + 1;
    if count < 2 {
        anyhow::bail!("nothing to squash: {commit} is HEAD");
    }
    let head = git_out(&["rev-parse", "HEAD"])?;
    let tree = git_out(&["rev-parse", "HEAD^{tree}"])?;
    let parents: Vec<String> = git_out(&["rev-list", "--parents", "-n1", &target])?
        .split_whitespace()
        .skip(1)
        .map(String::from)
        .collect();
    let author = if preserve_author {
        Some(author_of(&target)?)
    } else {
        None
    };
    let new = commit_tree(&tree, &parents, &message.join("\n\n"), author.as_ref())?;
    move_head(&new, &head, "workmux hunk squash")?;
    eprintln!(
        "squashed {count} commits into {}",
        git_out(&["log", "-1", "--format=%h %s", "HEAD"])?
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// split
// ---------------------------------------------------------------------------

/// One new commit of a split: hunk IDs (optionally with `:ranges`) and a message.
#[derive(Debug, PartialEq, Eq)]
pub struct PickGroup {
    pub ids: Vec<String>,
    pub message: Vec<String>,
}

/// Parsed `split` arguments after the commit.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SplitArgs {
    pub groups: Vec<PickGroup>,
    pub rest_message: Option<Vec<String>>,
    pub allow_pushed: bool,
}

/// Parse `--pick ID... -m MSG [-m BODY] ... --rest-message MSG [--allow-pushed]`.
pub fn parse_split_args(args: &[String]) -> Result<SplitArgs> {
    let mut out = SplitArgs::default();
    let mut i = 0;
    let value = |i: usize, flag: &str| -> Result<String> {
        args.get(i + 1)
            .cloned()
            .with_context(|| format!("{flag} needs a value"))
    };
    while i < args.len() {
        match args[i].as_str() {
            "--allow-pushed" => out.allow_pushed = true,
            "--pick" => out.groups.push(PickGroup {
                ids: Vec::new(),
                message: Vec::new(),
            }),
            "-m" | "--message" => {
                let group = out
                    .groups
                    .last_mut()
                    .context("-m must come after a --pick group")?;
                group.message.push(value(i, "-m")?);
                i += 1;
            }
            "--rest-message" => {
                out.rest_message
                    .get_or_insert_with(Vec::new)
                    .push(value(i, "--rest-message")?);
                i += 1;
            }
            flag if flag.starts_with('-') => anyhow::bail!("unknown option '{flag}'"),
            id => out
                .groups
                .last_mut()
                .context("hunk IDs must come after --pick")?
                .ids
                .push(id.to_string()),
        }
        i += 1;
    }
    if out.groups.is_empty() {
        anyhow::bail!("no --pick group given");
    }
    for group in &out.groups {
        if group.ids.is_empty() {
            anyhow::bail!("a --pick group has no hunk IDs");
        }
        if group.message.is_empty() {
            anyhow::bail!("a --pick group has no -m message");
        }
    }
    Ok(out)
}

struct HunkState<'a> {
    id: String,
    hunk: &'a DiffHunk,
    picked: Vec<bool>,
}

fn is_change(line: &str) -> bool {
    line.starts_with('+') || line.starts_with('-')
}

/// Patch for one group, marking the picked lines in `states`.
fn group_patch(group: &PickGroup, states: &mut [HunkState]) -> Result<String> {
    let mut wanted: Vec<(String, Vec<(usize, usize)>)> = Vec::new();
    for raw in &group.ids {
        let (id, ranges) = parse_id_range(raw)?;
        match wanted.iter_mut().find(|(wid, _)| wid == id) {
            Some(entry) => entry.1.extend(ranges),
            None => wanted.push((id.to_string(), ranges)),
        }
    }
    let mut patch = String::new();
    for (id, ranges) in wanted {
        let state = states
            .iter_mut()
            .find(|s| s.id == id)
            .with_context(|| format!("hunk {id} not found in the commit"))?;
        check_supported(state.hunk, &id)?;
        let len = state.hunk.lines.len();
        let mut selected = vec![false; len];
        if ranges.is_empty() {
            for (i, line) in state.hunk.lines.iter().enumerate() {
                selected[i] = is_change(line) && !state.picked[i];
            }
        } else {
            for (start, end) in ranges {
                if end > len {
                    anyhow::bail!("line range {start}-{end} exceeds hunk {id} ({len} lines)");
                }
                for (i, line) in state.hunk.lines[..end].iter().enumerate().skip(start - 1) {
                    if !is_change(line) {
                        continue;
                    }
                    if state.picked[i] {
                        anyhow::bail!(
                            "line {} of hunk {id} was already picked by an earlier group",
                            i + 1
                        );
                    }
                    selected[i] = true;
                }
            }
        }
        if !selected.iter().any(|&s| s) {
            continue;
        }
        patch.push_str(&build_patch(&slice_hunk_with_state(
            state.hunk,
            &state.picked,
            &selected,
        )?));
        for (i, sel) in selected.iter().enumerate() {
            state.picked[i] |= *sel;
        }
    }
    if patch.is_empty() {
        anyhow::bail!("a --pick group selects no changes");
    }
    Ok(patch)
}

/// Split a commit into several commits. Remaining changes go into a last commit.
pub fn split(commit: &str, args: &SplitArgs) -> Result<()> {
    let target = resolve_rewrite_target(commit, args.allow_pushed, false)?;
    let parents: Vec<String> = git_out(&["rev-list", "--parents", "-n1", &target])?
        .split_whitespace()
        .skip(1)
        .map(String::from)
        .collect();
    if parents.len() > 1 {
        anyhow::bail!("cannot split a merge commit");
    }
    let base_tree = match parents.first() {
        Some(p) => git_out(&["rev-parse", &format!("{p}^{{tree}}")])?,
        None => EMPTY_TREE.to_string(),
    };
    let target_tree = git_out(&["rev-parse", &format!("{target}^{{tree}}")])?;

    let diff = git_diff_commit(&target, None)?;
    let hunks = parse_diff(&diff);
    let identified = assign_ids(&hunks);
    let mut states: Vec<HunkState> = identified
        .iter()
        .map(|(id, hunk)| HunkState {
            id: id.clone(),
            hunk,
            picked: vec![false; hunk.lines.len()],
        })
        .collect();
    // Validate everything before any object is written.
    let patches = args
        .groups
        .iter()
        .map(|group| group_patch(group, &mut states))
        .collect::<Result<Vec<_>>>()?;

    let original_message = git_out(&["log", "-1", "--format=%B", &target])?;
    let author = author_of(&target)?;
    let tmp_index = git_dir()?.join(format!("workmux-hunk-split-{}", std::process::id()));
    let built = build_chain(
        &base_tree,
        &target_tree,
        &parents,
        args,
        &patches,
        &original_message,
        &author,
        &tmp_index,
    );
    let _ = std::fs::remove_file(&tmp_index);
    let (tip, created) = built?;

    if commits_after(&target)? == 0 {
        let head = git_out(&["rev-parse", "HEAD"])?;
        move_head(&tip, &head, "workmux hunk split")?;
    } else {
        let output = git_cmd()?
            .args(["rebase", "--onto", &tip, &target, "--autostash"])
            .output()
            .context("failed to run git rebase")?;
        if !output.status.success() {
            rebase_failed(
                &format!("replaying commits after the split of {}", short(&target)),
                &String::from_utf8_lossy(&output.stderr),
            )?;
        }
    }
    eprintln!("split {} into {} commits:", short(&target), created.len());
    for subject in created {
        eprintln!("  {subject}");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_chain(
    base_tree: &str,
    target_tree: &str,
    parents: &[String],
    args: &SplitArgs,
    patches: &[String],
    original_message: &str,
    author: &Author,
    tmp_index: &Path,
) -> Result<(String, Vec<String>)> {
    let index_cmd = |sub: &[&str]| -> Result<String> {
        run_git(git_cmd()?.env("GIT_INDEX_FILE", tmp_index).args(sub)).map(|s| s.trim().to_string())
    };
    index_cmd(&["read-tree", base_tree])?;
    let mut prev: Vec<String> = parents.to_vec();
    let mut created = Vec::new();
    let mut tree = base_tree.to_string();
    for (group, patch) in args.groups.iter().zip(patches) {
        apply_patch_to_index(patch, ApplyMode::Stage, tmp_index)
            .context("a picked hunk does not apply on top of the previous commit")?;
        tree = index_cmd(&["write-tree"])?;
        let message = group.message.join("\n\n");
        let sha = commit_tree(&tree, &prev, &message, Some(author))?;
        created.push(format!(
            "{} {}",
            short(&sha),
            message.lines().next().unwrap_or("")
        ));
        prev = vec![sha];
    }
    if tree != target_tree {
        let message = match &args.rest_message {
            Some(parts) => parts.join("\n\n"),
            None => original_message.to_string(),
        };
        let sha = commit_tree(target_tree, &prev, &message, Some(author))?;
        created.push(format!(
            "{} {}",
            short(&sha),
            message.lines().next().unwrap_or("")
        ));
        prev = vec![sha];
    }
    Ok((prev.remove(0), created))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fold_todo_moves_sources_after_target_as_fixups() {
        let commits = v(&["a", "b", "c", "d"]);
        let todo = fold_todo(&commits, "a", &v(&["c", "d"])).unwrap();
        assert_eq!(todo, ["pick a", "fixup c", "fixup d", "pick b"]);
    }

    #[test]
    fn fold_todo_rejects_source_older_than_target() {
        assert!(fold_todo(&v(&["a", "b"]), "b", &v(&["a"])).is_err());
    }

    #[test]
    fn move_todo_after_and_before() {
        let commits = v(&["a", "b", "c"]);
        assert_eq!(
            move_todo(&commits, "a", "c", true).unwrap(),
            ["pick b", "pick c", "pick a"]
        );
        assert_eq!(
            move_todo(&commits, "c", "a", false).unwrap(),
            ["pick c", "pick a", "pick b"]
        );
    }

    #[test]
    fn parses_split_arguments() {
        let args = v(&[
            "--pick",
            "aaa",
            "bbb:1-3",
            "-m",
            "first",
            "-m",
            "body",
            "--pick",
            "ccc",
            "-m",
            "second",
            "--rest-message",
            "rest",
            "--allow-pushed",
        ]);
        let parsed = parse_split_args(&args).unwrap();
        assert_eq!(parsed.groups.len(), 2);
        assert_eq!(parsed.groups[0].ids, ["aaa", "bbb:1-3"]);
        assert_eq!(parsed.groups[0].message, ["first", "body"]);
        assert_eq!(parsed.rest_message, Some(v(&["rest"])));
        assert!(parsed.allow_pushed);
    }

    #[test]
    fn rejects_malformed_split_arguments() {
        assert!(parse_split_args(&v(&[])).is_err());
        assert!(parse_split_args(&v(&["--pick", "-m", "x"])).is_err());
        assert!(parse_split_args(&v(&["--pick", "aaa"])).is_err());
        assert!(parse_split_args(&v(&["aaa"])).is_err());
        assert!(parse_split_args(&v(&["--pick", "aaa", "-m"])).is_err());
    }
}
