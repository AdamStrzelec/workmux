//! Git hooks that enforce protected branches outside workmux: refuse
//! commits, merge commits and pushes to the listed branches. Plain POSIX sh,
//! marked so `uninstall` never touches hooks that are not ours. Anyone with
//! write access to the repo can bypass them (`--no-verify`, deleting the
//! hook), so this is a guard against mistakes, not a security boundary.

pub const MARKER: &str = "# workmux-enforce v1";
pub const HOOKS: [&str; 3] = ["pre-commit", "pre-merge-commit", "pre-push"];
/// Set to any value to let one command through.
pub const BYPASS_ENV: &str = "WORKMUX_ALLOW_PROTECTED";

/// Branch names are baked into a shell script, so allow only a safe charset.
pub fn validate_branches(branches: &[String]) -> Result<(), String> {
    if branches.is_empty() {
        return Err("no branches given".into());
    }
    for b in branches {
        let ok = !b.is_empty()
            && !b.starts_with('-')
            && b.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'));
        if !ok {
            return Err(format!("unsupported branch name '{b}'"));
        }
    }
    Ok(())
}

/// Script text of `hook` (one of [`HOOKS`]) for the given branches.
pub fn script(hook: &str, branches: &[String]) -> String {
    let list = branches.join(" ");
    let head =
        format!("#!/bin/sh\n{MARKER}\n[ -n \"${BYPASS_ENV}\" ] && exit 0\nbranches=\"{list}\"\n");
    let body = if hook == "pre-push" {
        r#"while read -r _ _ remote_ref _; do
  for p in $branches; do
    if [ "$remote_ref" = "refs/heads/$p" ]; then
      echo "workmux: push to protected branch '$p' is blocked (set WORKMUX_ALLOW_PROTECTED=1 to override)" >&2
      exit 1
    fi
  done
done
exit 0
"#
    } else {
        r#"current=$(git symbolic-ref --short -q HEAD)
for p in $branches; do
  if [ "$current" = "$p" ]; then
    echo "workmux: commits on protected branch '$p' are blocked (set WORKMUX_ALLOW_PROTECTED=1 to override)" >&2
    exit 1
  fi
done
exit 0
"#
    };
    format!("{head}{body}")
}

pub fn is_ours(text: &str) -> bool {
    text.lines().take(3).any(|l| l.trim() == MARKER)
}

/// Branches listed in an installed hook, if it is ours.
pub fn branches_of(text: &str) -> Option<Vec<String>> {
    if !is_ours(text) {
        return None;
    }
    let line = text.lines().find(|l| l.starts_with("branches=\""))?;
    let inner = line.trim_start_matches("branches=\"").trim_end_matches('"');
    Some(inner.split_whitespace().map(String::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn validates_branch_names() {
        assert!(validate_branches(&b(&["main", "release/1.0"])).is_ok());
        assert!(validate_branches(&b(&[])).is_err());
        assert!(validate_branches(&b(&["a;rm -rf /"])).is_err());
        assert!(validate_branches(&b(&["$(x)"])).is_err());
        assert!(validate_branches(&b(&["-x"])).is_err());
    }

    #[test]
    fn script_round_trips_branches_and_marker() {
        for hook in HOOKS {
            let s = script(hook, &b(&["main", "master"]));
            assert!(s.starts_with("#!/bin/sh\n"));
            assert!(is_ours(&s));
            assert_eq!(branches_of(&s), Some(b(&["main", "master"])));
        }
        assert!(script("pre-push", &b(&["main"])).contains("remote_ref"));
        assert!(!is_ours("#!/bin/sh\necho hi\n"));
        assert_eq!(branches_of("#!/bin/sh\n"), None);
    }
}
