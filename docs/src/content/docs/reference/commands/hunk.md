---
title: "hunk"
description: Stage, commit and move individual diff hunks without an interactive prompt
---

Non-interactive, hunk-level git operations, meant for AI agents (and humans
who want to commit part of a file). Ported from
[git-surgeon](https://github.com/raine/git-surgeon).

Every hunk has a short ID: a hash of the file path and the hunk body. It does
not depend on line numbers, so it survives edits elsewhere in the file.
Identical hunks get a `-2`, `-3` suffix. Re-run `list` after changing files.

Only changes to tracked files show up in the diff. Use `git add -N <file>` to
make a new file visible.

```bash
workmux hunk list [--staged] [--file PATH] [--commit REV] [--full] [--blame]
workmux hunk show ID [--commit REV]
workmux hunk stage ID... [--lines START-END]
workmux hunk unstage ID... [--lines START-END]
workmux hunk discard ID... [--lines START-END]
workmux hunk commit -m MESSAGE ID...
workmux hunk commit-to BRANCH -m MESSAGE ID...
workmux hunk undo ID... --from REV [--lines START-END]
workmux hunk amend REV [--allow-pushed]
workmux hunk reword REV -m SUBJECT [-m BODY]... [--allow-pushed]
```

## Blame

`list --blame` prefixes every line of each hunk with the 7-character hash of the
commit that introduced it:

```
bf4940f f (+1 -1)
  10fac99  a
  caa32fe  B
  10fac99 -c
  0000000 +X
```

- Context and removed lines show the commit that introduced them.
- Added lines show `0000000` while uncommitted; with `--commit` they show that
  commit.
- `-------` means blame was not available. This happens for removed lines of
  an unstaged diff when the file also has staged changes, because the old side
  is then the index and not `HEAD`.
- `--blame` takes precedence over `--full`.

## Undo, amend and reword

`undo` reverse-applies hunks of an earlier commit to the working tree, so you
can revert part of a commit without reverting all of it. Find IDs with
`list --commit REV`. The changes appear as unstaged modifications; it fails if
the surrounding lines have changed since.

`amend REV` folds the currently staged changes into an earlier commit (or HEAD).
`reword REV -m ...` changes a commit message without touching its content;
repeat `-m` for body paragraphs. Older commits are rewritten with a
non-interactive autosquash rebase, and unstaged changes are kept.

Safety checks, since these rewrite history and run without hooks:

- refused on `main`, `master` and the default branch, and on a detached HEAD;
- refused when the commit is not an ancestor of HEAD or merge commits lie
  between it and HEAD (the rebase would flatten them);
- refused when the commit is already on a remote branch, unless
  `--allow-pushed` (a force push is then needed);
- `reword` needs an empty index, `amend` needs staged changes.

On a rebase conflict the repository stays in the rebase state; resolve it and
run `git rebase --continue`, or `git rebase --abort`.

## Partial hunks

`--lines START-END` (single hunk) and inline ranges for `commit` and
`commit-to` (`ID:5`, `ID:1-11`, `ID:2,5-6,34`) select only some changed lines
of a hunk. Line numbers are the hunk-relative ones printed by `show` and
`list --full`.

## Behavior

- `commit` and `commit-to` refuse to run when the index already has staged
  changes.
- `commit` restores the index if the commit fails .
- `commit-to` builds the commit in a temporary index, so the target branch is
  not checked out. It refuses the branch you are on, updates the ref only if
  it has not moved, and then removes the hunks from the working tree.
- `discard` throws the changes away and cannot be undone.
- Git runs through workmux's hardened constructor: hooks, GPG signing and
  pagers are disabled, so `commit` skips pre-commit hooks and does not sign.
- Because hooks are skipped, `commit` and `commit-to` refuse to write to a
  protected branch themselves: `main`, `master` and the repository's default
  branch. Work on a feature branch and merge after review.
- Renames, copies and mode changes are not supported at hunk level.
