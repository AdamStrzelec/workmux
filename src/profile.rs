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
}
