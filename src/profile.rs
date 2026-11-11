//! Safety profiles: limit which state-changing commands this user's workmux
//! may run. Configured per OS user in the global config (`safety_profile`).
//! Not a security boundary against the user themselves: anyone who can edit
//! their own config can change it.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    /// No restrictions (default).
    #[default]
    Trusted,
    /// No history rewriting or discarding; `merge` cannot use `--allow-protected`.
    Safe,
    /// Like `safe`, plus no `merge`, `remove` or `rebase`.
    ReviewOnly,
}

/// Profile to enforce given the result of loading the global config. An
/// unreadable or invalid config fails closed to `review-only`; the note says why.
pub fn resolve(loaded: Result<Option<Profile>, String>) -> (Profile, Option<String>) {
    match loaded {
        Ok(p) => (p.unwrap_or_default(), None),
        Err(e) => (
            Profile::ReviewOnly,
            Some(format!(
                "global config is invalid, so the strictest profile applies: {e}"
            )),
        ),
    }
}

/// Like `check`, adding `note` (why the profile was chosen) to a refusal.
pub fn check_noted(profile: Profile, note: Option<&str>, args: &[String]) -> Result<()> {
    check(profile, args).map_err(|e| match note {
        Some(n) => e.context(n.to_string()),
        None => e,
    })
}

/// Hunk subcommands that discard work or rewrite history.
const HUNK_DESTRUCTIVE: &[&str] = &[
    "discard", "undo", "amend", "reword", "fold", "squash", "move", "split",
];

/// Err when `args` (full argv) is not allowed under `profile`.
pub fn check(profile: Profile, args: &[String]) -> Result<()> {
    if profile == Profile::Trusted {
        return Ok(());
    }
    let Some(label) = crate::audit::action_label(args) else {
        return Ok(());
    };
    let name = match profile {
        Profile::Safe => "safe",
        _ => "review-only",
    };
    let mut words = label.split(' ');
    let cmd = words.next().unwrap_or("");
    let sub = words.next();
    let denied = match (cmd, sub) {
        ("hunk", Some(s)) => HUNK_DESTRUCTIVE.contains(&s),
        ("merge", _) => {
            profile == Profile::ReviewOnly || args.iter().any(|a| a == "--allow-protected")
        }
        ("remove" | "rebase", _) => profile == Profile::ReviewOnly,
        _ => false,
    };
    if denied {
        crate::audit::record_action(format!("blocked by profile {name}: {label}"), false);
        bail!("'{label}' is not allowed under safety profile '{name}'");
    }
    Ok(())
}

/// Raw git/gh commands a restricted profile denies to Claude, so work goes
/// through workmux (which applies this profile and writes the audit log).
const CLAUDE_DENY_SAFE: &[&str] = &[
    "Bash(git push:*)",
    "Bash(git merge:*)",
    "Bash(git rebase:*)",
    "Bash(git reset --hard:*)",
    "Bash(git clean:*)",
    "Bash(gh pr merge:*)",
];

/// Extra denials for `review-only`: commits must go through `workmux hunk commit`.
const CLAUDE_DENY_REVIEW_ONLY: &[&str] = &["Bash(git commit:*)", "Bash(git checkout:*)"];

/// Extra `claude` flags enforced by a profile. None for `trusted`.
/// `protected` are the configured `guard.protected_paths`.
///
/// `safe`: no bypass permissions, no raw push/merge/rebase/hard reset/clean,
/// no Edit/Write of protected paths. `review-only`: additionally no raw
/// `git commit`/`checkout`. Claude-side only: it does not stop Bash from
/// writing files by other means, and it is not a boundary against the user.
pub fn claude_args(profile: Profile, protected: &[String]) -> Vec<String> {
    if profile == Profile::Trusted {
        return Vec::new();
    }
    let mut deny: Vec<String> = CLAUDE_DENY_SAFE.iter().map(|s| s.to_string()).collect();
    if profile == Profile::ReviewOnly {
        deny.extend(CLAUDE_DENY_REVIEW_ONLY.iter().map(|s| s.to_string()));
    }
    for pat in protected {
        let pat = pat.trim();
        if pat.is_empty() {
            continue;
        }
        let scoped = if pat.contains('/') {
            pat.to_string()
        } else {
            format!("**/{pat}")
        };
        deny.push(format!("Edit({scoped})"));
        deny.push(format!("Write({scoped})"));
    }
    let mut args = vec!["--disallowedTools".to_string()];
    args.extend(deny);
    args
}

/// Drops bypass-permission switches from an existing claude argv: the skip
/// flag is removed and `--permission-mode bypassPermissions` becomes
/// `acceptEdits`. Used when the profile is not `trusted`.
pub fn strip_bypass(args: &mut Vec<String>) {
    args.retain(|a| a != "--dangerously-skip-permissions");
    for i in 0..args.len().saturating_sub(1) {
        if args[i] == "--permission-mode" && args[i + 1] == "bypassPermissions" {
            args[i + 1] = "acceptEdits".to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &str) -> Vec<String> {
        std::iter::once("workmux")
            .chain(s.split(' '))
            .map(String::from)
            .collect()
    }

    #[test]
    fn trusted_allows_everything() {
        assert!(check(Profile::Trusted, &a("hunk amend")).is_ok());
        assert!(check(Profile::Trusted, &a("merge x")).is_ok());
    }

    #[test]
    fn safe_blocks_rewrites_and_forced_merge() {
        assert!(check(Profile::Safe, &a("hunk squash")).is_err());
        assert!(check(Profile::Safe, &a("merge x --allow-protected")).is_err());
        assert!(check(Profile::Safe, &a("merge x")).is_ok());
        assert!(check(Profile::Safe, &a("hunk commit -m x")).is_ok());
        assert!(check(Profile::Safe, &a("remove x")).is_ok());
    }

    #[test]
    fn review_only_blocks_merge_remove_rebase() {
        for c in ["merge x", "remove x", "rm x", "rebase x", "hunk discard"] {
            assert!(check(Profile::ReviewOnly, &a(c)).is_err(), "{c}");
        }
        assert!(check(Profile::ReviewOnly, &a("add feat")).is_ok());
        assert!(check(Profile::ReviewOnly, &a("hunk stage a")).is_ok());
        assert!(check(Profile::ReviewOnly, &a("list")).is_ok());
    }

    #[test]
    fn invalid_config_fails_closed() {
        let (p, note) = resolve(Err("bad yaml".into()));
        assert_eq!(p, Profile::ReviewOnly);
        assert!(note.unwrap().contains("bad yaml"));
        assert_eq!(resolve(Ok(None)), (Profile::Trusted, None));
        assert_eq!(resolve(Ok(Some(Profile::Safe))), (Profile::Safe, None));
    }

    #[test]
    fn claude_args_by_profile() {
        assert!(claude_args(Profile::Trusted, &[".env*".into()]).is_empty());
        let safe = claude_args(Profile::Safe, &[".env*".into(), "secrets/**".into()]);
        assert_eq!(safe[0], "--disallowedTools");
        assert!(safe.contains(&"Bash(git push:*)".to_string()));
        assert!(!safe.contains(&"Bash(git commit:*)".to_string()));
        assert!(safe.contains(&"Edit(**/.env*)".to_string()));
        assert!(safe.contains(&"Write(secrets/**)".to_string()));
        let ro = claude_args(Profile::ReviewOnly, &[]);
        assert!(ro.contains(&"Bash(git commit:*)".to_string()));
    }

    #[test]
    fn strip_bypass_removes_skip_and_downgrades_mode() {
        let mut args: Vec<String> = [
            "--dangerously-skip-permissions",
            "--permission-mode",
            "bypassPermissions",
            "--model",
            "opus",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        strip_bypass(&mut args);
        assert_eq!(
            args,
            ["--permission-mode", "acceptEdits", "--model", "opus"]
        );
    }
}
