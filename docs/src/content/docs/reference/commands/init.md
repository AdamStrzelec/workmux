---
title: "init"
description: Generate a .workmux.yaml configuration file
---

Generates `.workmux.yaml` with example configuration and `"<global>"` placeholder usage.

```bash
workmux init
workmux init --worktree-dir .worktrees   # or -w
```

With `--worktree-dir <DIR>` (`-w`), the generated file contains an active
`worktree_dir: <DIR>` setting, so new worktrees go to that directory.

## Existing `.workmux.yaml`

Running `init` in an already initialized project is safe and never fails:

- `workmux init` leaves the file untouched and prints the configured
  worktree directory and next steps.
- `workmux init -w <DIR>` updates only the `worktree_dir` key, keeping all
  other lines and comments. Existing worktrees are not moved (use `C`, then
  `m` in the dashboard).
- `workmux init --force` regenerates the file from scratch, backing up the
  old one to `.workmux.yaml.bak`.
