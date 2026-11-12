//! End-to-end tests for `workmux hunk` against throwaway git repositories.

use std::path::Path;
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn hunk(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_workmux"))
        .current_dir(dir)
        .arg("hunk")
        .args(args)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn ids_for(dir: &Path, file: &str) -> Vec<String> {
    stdout(&hunk(dir, &["list"]))
        .lines()
        .filter(|l| l.split_whitespace().nth(1) == Some(file))
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .collect()
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "t@example.com"]);
    git(p, &["config", "user.name", "t"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    let numbers: String = (1..=40).map(|n| format!("{n}\n")).collect();
    std::fs::write(p.join("f.txt"), numbers).unwrap();
    std::fs::write(p.join("g.txt"), "a\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "init"]);
    git(p, &["branch", "other"]);
    git(p, &["checkout", "-q", "-b", "work"]);
    let edited = std::fs::read_to_string(p.join("f.txt"))
        .unwrap()
        .replace("\n3\n", "\nthree\n")
        .replace("\n30\n", "\nthirty\n");
    std::fs::write(p.join("f.txt"), edited).unwrap();
    std::fs::write(p.join("g.txt"), "a\nb\n").unwrap();
    dir
}

#[test]
fn stage_and_unstage_a_single_hunk() {
    let dir = repo();
    let p = dir.path();
    let ids = ids_for(p, "f.txt");
    assert_eq!(ids.len(), 2);

    assert!(hunk(p, &["stage", &ids[1]]).status.success());
    let staged = git(p, &["diff", "--cached"]);
    assert!(staged.contains("+thirty") && !staged.contains("+three\n"));

    assert!(hunk(p, &["unstage", &ids[1]]).status.success());
    assert!(git(p, &["diff", "--cached"]).is_empty());
}

#[test]
fn commit_only_selected_hunk() {
    let dir = repo();
    let p = dir.path();
    let ids = ids_for(p, "f.txt");
    assert!(
        hunk(p, &["commit", "-m", "second", &ids[1]])
            .status
            .success()
    );
    assert_eq!(git(p, &["log", "-1", "--format=%s"]).trim(), "second");
    assert!(git(p, &["show", "HEAD"]).contains("+thirty"));
    assert!(!git(p, &["show", "HEAD"]).contains("+three\n"));
    // The other hunk is still an uncommitted working-tree change.
    assert!(git(p, &["diff"]).contains("+three"));
}

#[test]
fn commit_refuses_when_index_is_dirty() {
    let dir = repo();
    let p = dir.path();
    git(p, &["add", "g.txt"]);
    let ids = ids_for(p, "f.txt");
    let out = hunk(p, &["commit", "-m", "x", &ids[0]]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("staged changes"));
}

#[test]
fn commit_to_other_branch_moves_hunk_out_of_worktree() {
    let dir = repo();
    let p = dir.path();
    let before = git(p, &["rev-parse", "HEAD"]);
    let ids = ids_for(p, "g.txt");
    let out = hunk(p, &["commit-to", "other", "-m", "g to other", &ids[0]]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        git(p, &["rev-parse", "HEAD"]),
        before,
        "current branch untouched"
    );
    assert_eq!(
        git(p, &["log", "-1", "--format=%s", "other"]).trim(),
        "g to other"
    );
    assert_eq!(git(p, &["show", "other:g.txt"]), "a\nb\n");
    assert!(!git(p, &["status", "--short"]).contains("g.txt"));
    assert!(git(p, &["status", "--short"]).contains("f.txt"));
}

#[test]
fn commit_to_rejects_checked_out_branch() {
    let dir = repo();
    let p = dir.path();
    let ids = ids_for(p, "g.txt");
    let out = hunk(p, &["commit-to", "work", "-m", "x", &ids[0]]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("currently checked out"));
}

#[test]
fn commit_refuses_on_main() {
    let dir = repo();
    let p = dir.path();
    let ids = ids_for(p, "g.txt");
    git(p, &["checkout", "-q", "main"]);
    let out = hunk(p, &["commit", "-m", "x", &ids[0]]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("protected branch"));
    assert_eq!(git(p, &["log", "-1", "--format=%s"]).trim(), "init");
}

#[test]
fn commit_to_refuses_protected_target() {
    let dir = repo();
    let p = dir.path();
    let ids = ids_for(p, "g.txt");
    let out = hunk(p, &["commit-to", "main", "-m", "x", &ids[0]]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("protected branch"));
    assert_eq!(git(p, &["log", "-1", "--format=%s", "main"]).trim(), "init");
}

#[test]
fn inline_line_range_stages_part_of_a_hunk() {
    let dir = repo();
    let p = dir.path();
    // Make one hunk with two separate changes close together.
    std::fs::write(p.join("g.txt"), "A\nb\n").unwrap();
    let ids = ids_for(p, "g.txt");
    let shown = stdout(&hunk(p, &["show", &ids[0]]));
    assert!(shown.contains("-a") && shown.contains("+A"), "{shown}");
    // Line 2 of the hunk is "-a"; staging only it leaves "+A" and "+b" unstaged.
    let out = hunk(p, &["stage", "--lines", "1-1", &ids[0]]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let staged = git(p, &["diff", "--cached"]);
    assert!(staged.contains("-a") && !staged.contains("+A"), "{staged}");
}

#[test]
fn unknown_id_fails_cleanly() {
    let dir = repo();
    let out = hunk(dir.path(), &["stage", "deadbee"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not found"));
}

#[test]
fn works_inside_a_linked_worktree() {
    let dir = repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "--", "."]);
    let wt = dir.path().join("wt");
    git(
        p,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            wt.to_str().unwrap(),
        ],
    );
    std::fs::write(wt.join("g.txt"), "a\nb\n").unwrap();
    let ids = ids_for(&wt, "g.txt");
    assert_eq!(ids.len(), 1);
    let out = hunk(&wt, &["commit", "-m", "in worktree", &ids[0]]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s"]).trim(),
        "in worktree"
    );
    assert_eq!(git(p, &["log", "-1", "--format=%s", "main"]).trim(), "init");
}

#[test]
fn list_blame_shows_origin_commit_of_each_line() {
    let dir = repo();
    let p = dir.path();
    let init = git(p, &["rev-parse", "--short=7", "HEAD"])
        .trim()
        .to_string();
    // Commit one hunk of f.txt on the current branch, then change g.txt more.
    git(p, &["checkout", "-q", "--", "g.txt"]);
    git(p, &["commit", "-qam", "second"]);
    let second = git(p, &["rev-parse", "--short=7", "HEAD"])
        .trim()
        .to_string();
    std::fs::write(p.join("g.txt"), "a\nc\n").unwrap();

    let out = stdout(&hunk(p, &["list", "--blame", "--file", "g.txt"]));
    assert!(
        out.contains(&format!("{init} -")) || out.contains(&format!("{init}  ")),
        "{out}"
    );
    assert!(out.contains("0000000 +c"), "{out}");

    let ids = ids_for(p, "g.txt");
    assert_eq!(ids.len(), 1);
    let by_commit = stdout(&hunk(
        p,
        &["list", "--blame", "--commit", &second, "--file", "f.txt"],
    ));
    assert!(by_commit.contains(&format!("{second} +")), "{by_commit}");
}

/// Repo on branch `work` with three commits: one.txt, two.txt, three.txt.
fn history_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "t@example.com"]);
    git(p, &["config", "user.name", "t"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    std::fs::write(p.join("base.txt"), "base\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "base"]);
    git(p, &["checkout", "-q", "-b", "work"]);
    for name in ["one", "two", "three"] {
        std::fs::write(p.join(format!("{name}.txt")), format!("{name}\n")).unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-qm", name]);
    }
    dir
}

fn subjects(p: &std::path::Path) -> Vec<String> {
    git(p, &["log", "--format=%s", "main..HEAD"])
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn amend_head_folds_staged_hunks() {
    let dir = history_repo();
    let p = dir.path();
    std::fs::write(p.join("three.txt"), "three\nmore\n").unwrap();
    let ids = ids_for(p, "three.txt");
    assert!(hunk(p, &["stage", &ids[0]]).status.success());
    let out = hunk(p, &["amend", "HEAD"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(subjects(p), ["three", "two", "one"]);
    assert_eq!(git(p, &["show", "HEAD:three.txt"]), "three\nmore\n");
}

#[test]
fn amend_older_commit_rewrites_it_and_keeps_later_ones() {
    let dir = history_repo();
    let p = dir.path();
    let target = git(p, &["rev-parse", "HEAD~2"]).trim().to_string();
    std::fs::write(p.join("one.txt"), "one\nfixed\n").unwrap();
    std::fs::write(p.join("three.txt"), "three\nwip\n").unwrap();
    let ids = ids_for(p, "one.txt");
    assert!(hunk(p, &["stage", &ids[0]]).status.success());
    let out = hunk(p, &["amend", &target]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(subjects(p), ["three", "two", "one"]);
    assert_eq!(git(p, &["show", "HEAD~2:one.txt"]), "one\nfixed\n");
    assert_eq!(git(p, &["show", "HEAD:three.txt"]), "three\n");
    // Unstaged work survives the rebase.
    assert_eq!(
        std::fs::read_to_string(p.join("three.txt")).unwrap(),
        "three\nwip\n"
    );
    assert!(git(p, &["diff", "--cached"]).is_empty());
}

#[test]
fn amend_without_staged_changes_fails() {
    let dir = history_repo();
    let out = hunk(dir.path(), &["amend", "HEAD"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no staged changes"));
}

#[test]
fn reword_older_commit_changes_only_the_message() {
    let dir = history_repo();
    let p = dir.path();
    let tree_before = git(p, &["rev-parse", "HEAD^{tree}"]);
    let target = git(p, &["rev-parse", "HEAD~1"]).trim().to_string();
    let out = hunk(p, &["reword", &target, "-m", "second", "-m", "body text"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(subjects(p), ["three", "second", "one"]);
    assert_eq!(
        git(p, &["log", "-1", "--format=%b", "HEAD~1"]).trim(),
        "body text"
    );
    assert_eq!(git(p, &["rev-parse", "HEAD^{tree}"]), tree_before);
}

#[test]
fn reword_head() {
    let dir = history_repo();
    let p = dir.path();
    let out = hunk(p, &["reword", "HEAD", "-m", "third"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(subjects(p), ["third", "two", "one"]);
}

#[test]
fn history_rewrites_refuse_protected_branch() {
    let dir = history_repo();
    let p = dir.path();
    git(p, &["checkout", "-q", "main"]);
    let out = hunk(p, &["reword", "HEAD", "-m", "x"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("protected branch"));
    assert_eq!(git(p, &["log", "-1", "--format=%s"]).trim(), "base");
}

#[test]
fn history_rewrites_refuse_pushed_commits_unless_allowed() {
    let dir = history_repo();
    let p = dir.path();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(
        p,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(p, &["push", "-q", "origin", "work"]);
    let out = hunk(p, &["reword", "HEAD", "-m", "x"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--allow-pushed"));
    let out = hunk(p, &["reword", "HEAD", "-m", "x", "--allow-pushed"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(git(p, &["log", "-1", "--format=%s"]).trim(), "x");
}

#[test]
fn undo_reverse_applies_a_hunk_of_an_earlier_commit() {
    let dir = history_repo();
    let p = dir.path();
    std::fs::write(p.join("two.txt"), "two\nextra\n").unwrap();
    git(p, &["commit", "-qam", "extra"]);
    git(p, &["commit", "-q", "--allow-empty", "-m", "noise"]);
    let listing = stdout(&hunk(p, &["list", "--commit", "HEAD~1"]));
    let id = listing.split_whitespace().next().unwrap().to_string();
    let out = hunk(p, &["undo", &id, "--from", "HEAD~1"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read_to_string(p.join("two.txt")).unwrap(), "two\n");
    assert!(git(p, &["diff", "--cached"]).is_empty());
    let bad = hunk(p, &["undo", "deadbee", "--from", "HEAD~1"]);
    assert!(!bad.status.success());
}

fn ok(out: &std::process::Output) {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn squash_combines_commits_and_keeps_oldest_author() {
    let dir = history_repo();
    let p = dir.path();
    let tree = git(p, &["rev-parse", "HEAD^{tree}"]);
    git(p, &["config", "user.name", "someone else"]);
    ok(&hunk(
        p,
        &["squash", "HEAD~1", "-m", "two and three", "-m", "body"],
    ));
    assert_eq!(subjects(p), ["two and three", "one"]);
    assert_eq!(git(p, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(git(p, &["log", "-1", "--format=%an"]).trim(), "t");
    assert_eq!(git(p, &["log", "-1", "--format=%b"]).trim(), "body");
    let out = hunk(p, &["squash", "HEAD", "-m", "x"]);
    assert!(!out.status.success());
}

#[test]
fn fold_moves_commits_into_an_earlier_one() {
    let dir = history_repo();
    let p = dir.path();
    let one = git(p, &["rev-parse", "HEAD~2"]).trim().to_string();
    let tree = git(p, &["rev-parse", "HEAD^{tree}"]);
    ok(&hunk(p, &["fold", &one]));
    assert_eq!(subjects(p), ["two", "one"]);
    assert_eq!(git(p, &["rev-parse", "HEAD^{tree}"]), tree);
    assert!(git(p, &["show", "--stat", "--format=", "HEAD~1"]).contains("three.txt"));
}

#[test]
fn fold_with_from_picks_a_middle_commit() {
    let dir = history_repo();
    let p = dir.path();
    let one = git(p, &["rev-parse", "HEAD~2"]).trim().to_string();
    let two = git(p, &["rev-parse", "HEAD~1"]).trim().to_string();
    ok(&hunk(p, &["fold", &one, "--from", &two]));
    assert_eq!(subjects(p), ["three", "one"]);
}

#[test]
fn move_reorders_commits() {
    let dir = history_repo();
    let p = dir.path();
    let one = git(p, &["rev-parse", "HEAD~2"]).trim().to_string();
    ok(&hunk(p, &["move", &one, "--to-end"]));
    assert_eq!(subjects(p), ["one", "three", "two"]);
    let three = git(p, &["rev-parse", "HEAD~1"]).trim().to_string();
    let two = git(p, &["rev-parse", "HEAD~2"]).trim().to_string();
    ok(&hunk(p, &["move", &three, "--before", &two]));
    assert_eq!(subjects(p), ["one", "two", "three"]);
}

/// History repo whose last commits change two separate regions of one file.
fn split_repo() -> tempfile::TempDir {
    let dir = history_repo();
    let p = dir.path();
    let numbers: String = (1..=40).map(|n| format!("{n}\n")).collect();
    std::fs::write(p.join("n.txt"), &numbers).unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "add n"]);
    let edited = numbers
        .replace("\n3\n", "\nthree\n")
        .replace("\n30\n", "\nthirty\n");
    std::fs::write(p.join("n.txt"), edited).unwrap();
    std::fs::write(p.join("new.txt"), "brand new\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "big change"]);
    std::fs::write(p.join("later.txt"), "later\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "later"]);
    dir
}

fn first_id(p: &std::path::Path, rev: &str) -> String {
    let listing = stdout(&hunk(p, &["list", "--commit", rev, "--file", "n.txt"]));
    listing.split_whitespace().next().unwrap().to_string()
}

#[test]
fn split_head_creates_picked_commit_and_rest() {
    let dir = split_repo();
    let p = dir.path();
    git(p, &["reset", "-q", "--hard", "HEAD~1"]);
    let tree = git(p, &["rev-parse", "HEAD^{tree}"]);
    let id = first_id(p, "HEAD");
    ok(&hunk(
        p,
        &[
            "split",
            "HEAD",
            "--pick",
            &id,
            "-m",
            "first region",
            "--rest-message",
            "the rest",
        ],
    ));
    assert_eq!(subjects(p)[..2], ["the rest", "first region"]);
    assert_eq!(
        git(p, &["rev-parse", "HEAD^{tree}"]),
        tree,
        "final tree unchanged"
    );
    let first = git(p, &["show", "HEAD~1:n.txt"]);
    assert!(
        first.contains("\nthree\n") && !first.contains("thirty"),
        "{first}"
    );
    assert!(git(p, &["status", "--short"]).is_empty());
}

#[test]
fn split_older_commit_replays_later_commits() {
    let dir = split_repo();
    let p = dir.path();
    let big = git(p, &["rev-parse", "HEAD~1"]).trim().to_string();
    let tree = git(p, &["rev-parse", "HEAD^{tree}"]);
    let id = first_id(p, &big);
    ok(&hunk(
        p,
        &["split", &big, "--pick", &id, "-m", "first region"],
    ));
    assert_eq!(subjects(p)[..3], ["later", "big change", "first region"]);
    assert_eq!(git(p, &["rev-parse", "HEAD^{tree}"]), tree);
}

#[test]
fn split_with_line_ranges_across_groups() {
    let dir = split_repo();
    let p = dir.path();
    let big = git(p, &["rev-parse", "HEAD~1"]).trim().to_string();
    let tree = git(p, &["rev-parse", "HEAD^{tree}"]);
    let id = first_id(p, &big);
    let shown = stdout(&hunk(p, &["show", &id, "--commit", &big]));
    assert!(shown.contains("-3") && shown.contains("+three"), "{shown}");
    let minus = shown
        .lines()
        .find(|l| l.ends_with(":-3"))
        .unwrap()
        .split(':')
        .next()
        .unwrap()
        .trim()
        .to_string();
    ok(&hunk(
        p,
        &[
            "split",
            &big,
            "--pick",
            &format!("{id}:{minus}"),
            "-m",
            "remove 3",
            "--pick",
            &id,
            "-m",
            "add three",
        ],
    ));
    assert_eq!(
        subjects(p)[..4],
        ["later", "big change", "add three", "remove 3"]
    );
    assert_eq!(git(p, &["rev-parse", "HEAD^{tree}"]), tree);
}

#[test]
fn split_rejects_unknown_hunk_without_touching_history() {
    let dir = split_repo();
    let p = dir.path();
    let head = git(p, &["rev-parse", "HEAD"]);
    let out = hunk(p, &["split", "HEAD~1", "--pick", "deadbee", "-m", "x"]);
    assert!(!out.status.success());
    assert_eq!(git(p, &["rev-parse", "HEAD"]), head);
}
