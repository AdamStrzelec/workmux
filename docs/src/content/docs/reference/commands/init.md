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

## Global config (`--global`)

`workmux init --global` writes the default global config
(`~/.config/workmux/config.yaml`): `nerdfont`, `theme`, `mode: session` and
`split_panes: false` (every pane in its own tab).

- No file yet: it is created from the template.
- File exists: only missing defaults (`nerdfont`, `mode`, `split_panes`) are
  appended; every other line stays as is.
- `--global --force`: rewrites the file from the template, the old one is
  saved as `config.yaml.bak`.
