---
title: "ports"
description: List listening TCP ports with their process, worktree and tool window
---

Lists listening TCP ports on this machine and where they come from: the
process, the git worktree its working directory is in, and the tmux window
(run tool) it runs in. Works across all projects.

```bash
workmux ports [--json] [--all]
```

## Options

| Option   | Description                                                        |
| -------- | ------------------------------------------------------------------ |
| `--json` | Output as JSON                                                     |
| `--all`  | Show every listener, not only those inside git worktrees or tmux   |

## Example

```
PORT  PROCESS     WORKTREE   TOOL  PATH
3000  node 8267   test12     dev   /code/app__worktrees/test12
3001  node 46725  app        dev   /code/app
```

The dashboard shows the same data per worktree: a `ports` column, a `Ports`
line in the preview and the port next to running tools in the `x` menu.
