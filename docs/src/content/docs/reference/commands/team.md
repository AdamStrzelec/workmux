---
title: "team"
description: See what everyone sharing a directory is working on
---

Shows which agents each user runs, on which project and branch, and when they
were last active. Each user publishes a small JSON snapshot into a shared
directory; `workmux team` reads all of them. There is no daemon and no network
involved, so it works for Linux users on one machine (or hosts sharing a mount).

## Setup

Create the directory once, writable by the group that should see each other:

```bash
sudo mkdir -p /var/lib/workmux
sudo chgrp team /var/lib/workmux
sudo chmod 2775 /var/lib/workmux
```

Then, in each user's global config (`~/.config/workmux/config.yaml`):

```yaml
team:
  dir: /var/lib/workmux
  name: ania # optional, defaults to $USER
  stale_after_secs: 300 # optional
```

The `team` section is read from the global config only.

## Usage

```bash
workmux team              # publish my snapshot, then show everyone
workmux team --no-publish # only read
workmux team --json
```

Each user writes only their own `<name>.json`. A snapshot older than
`stale_after_secs` is marked `(stale ...)`, which usually means that user's
workmux has not run recently.

## What is shared

Project, worktree, branch, agent status, tmux session and window name, pane
title and last activity. Pane titles are session summaries written by the
agent, so they can describe what someone is working on.
