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
workmux hunk list [--staged] [--file PATH] [--commit REV] [--full]
workmux hunk show ID [--commit REV]
workmux hunk stage ID... [--lines START-END]
workmux hunk unstage ID... [--lines START-END]
workmux hunk discard ID... [--lines START-END]
workmux hunk commit -m MESSAGE ID...
workmux hunk commit-to BRANCH -m MESSAGE ID...
```

## Partial hunks

`--lines START-END` (single hunk) and inline ranges for `commit` and
`commit-to` (`ID:5`, `ID:1-11`, `ID:2,5-6,34`) select only some changed lines
of a hunk. Line numbers are the hunk-relative ones printed by `show` and
`list --full`.

## Behavior

- `commit` and `commit-to` refuse to run when the index already has staged
  changes.
- `commit` restores the index if the commit fails (for example a hook).
- `commit-to` builds the commit in a temporary index, so the target branch is
  not checked out. It refuses the branch you are on, updates the ref only if
  it has not moved, and then removes the hunks from the working tree.
- `discard` throws the changes away and cannot be undone.
- Git runs through workmux's hardened constructor: hooks, GPG signing and
  pagers are disabled, so `commit` skips pre-commit hooks and does not sign.
- Renames, copies and mode changes are not supported at hunk level.
