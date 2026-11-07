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
