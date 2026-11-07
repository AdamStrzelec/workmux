//! PR action submenu for the worktrees tab: push / draft PR, ready toggle,
//! merge, merge + remove, close. Planning is pure; execution runs in a
//! background thread and reports back through `AppEvent::PrActionResult`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::git;
use crate::github::{CheckState, PrSummary};
use crate::multiplexer::Multiplexer;
use crate::workflow;

use super::App;
use super::types::*;

/// Actions offered by the PR submenu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrActionKind {
    Push,
    ToggleReady,
    Merge,
    MergeRemove,
    Close,
}

impl PrActionKind {
    pub const ALL: [PrActionKind; 5] = [
        PrActionKind::Push,
        PrActionKind::ToggleReady,
        PrActionKind::Merge,
        PrActionKind::MergeRemove,
        PrActionKind::Close,
    ];

    pub fn key(self) -> char {
        match self {
            PrActionKind::Push => 'p',
            PrActionKind::ToggleReady => 'd',
            PrActionKind::Merge => 'm',
            PrActionKind::MergeRemove => 'M',
            PrActionKind::Close => 'c',
        }
    }

    pub fn from_key(c: char) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == c)
    }

    pub fn label(self) -> &'static str {
        match self {
            PrActionKind::Push => "push / create draft PR",
            PrActionKind::ToggleReady => "toggle draft <-> ready",
            PrActionKind::Merge => "merge PR",
            PrActionKind::MergeRemove => "merge PR + remove worktree",
            PrActionKind::Close => "close PR",
        }
    }
}

/// Snapshot of the worktree the PR menu acts on.
#[derive(Debug, Clone)]
pub struct PrTarget {
    pub handle: String,
    pub branch: String,
    pub path: PathBuf,
    /// Base branch for a new PR (stored workmux base, else default branch).
    pub base: Option<String>,
    pub pr: Option<PrSummary>,
    pub dirty: bool,
    pub has_gh: bool,
    pub prs_loading: bool,
}

/// Open PR submenu.
pub struct PrMenu {
    pub target: PrTarget,
}

impl PrMenu {
    /// Entries with their disabled reason (None = enabled).
    pub fn entries(&self) -> Vec<(PrActionKind, Option<&'static str>)> {
        let t = &self.target;
        PrActionKind::ALL
            .into_iter()
            .map(|k| {
                (
                    k,
                    pr_menu_state(k, t.pr.as_ref(), t.prs_loading, t.has_gh, t.dirty),
                )
            })
            .collect()
    }
}

/// Planned action: human readable description plus the commands to run.
#[derive(Debug, Clone, PartialEq)]
pub struct PrPlan {
    pub title: &'static str,
    pub lines: Vec<String>,
    pub steps: Vec<Vec<String>>,
    /// Remove the worktree (workmux remove) after all steps succeed.
    pub remove_after: bool,
    pub warning: Option<String>,
}

/// Confirmation popup state.
pub struct PrConfirm {
    pub target: PrTarget,
    pub plan: PrPlan,
}

/// Why merging is refused (first) or merely risky (second).
pub fn merge_block(pr: &PrSummary) -> (Option<String>, Option<String>) {
    if pr.state != "OPEN" {
        return (
            Some(format!(
                "PR #{} is not open ({})",
                pr.number,
                pr.state.to_lowercase()
            )),
            None,
        );
    }
    if pr.is_draft {
        return (
            Some(format!("PR #{} is a draft; mark it ready first", pr.number)),
            None,
        );
    }
    match pr.mergeable.as_deref() {
        Some("CONFLICTING") => {
            return (
                Some(format!("PR #{} has conflicts with its base", pr.number)),
                None,
            );
        }
        Some("MERGEABLE") | None => {}
        Some(_) => {
            return (
                Some("GitHub is still computing mergeability; retry shortly".to_string()),
                None,
            );
        }
    }
    match pr.checks {
        Some(CheckState::Failure { .. }) => (Some("CI is failing".to_string()), None),
        Some(CheckState::Pending { .. }) => (
            None,
            Some("CI is still running; GitHub may reject the merge".to_string()),
        ),
        _ => (None, None),
    }
}

/// Short disabled reason for a submenu entry (None = enabled). Mirrors
/// `plan_pr_action` refusals so a disabled entry never opens a confirmation.
pub fn pr_menu_state(
    kind: PrActionKind,
    pr: Option<&PrSummary>,
    prs_loading: bool,
    has_gh: bool,
    dirty: bool,
) -> Option<&'static str> {
    if has_gh && prs_loading {
        return Some("PRs loading");
    }
    if kind == PrActionKind::Push {
        return None;
    }
    if !has_gh {
        return Some("no gh");
    }
    let Some(pr) = pr else {
        return Some("no PR");
    };
    if pr.state != "OPEN" {
        return Some("PR not open");
    }
    match kind {
        PrActionKind::ToggleReady | PrActionKind::Close | PrActionKind::Push => None,
        PrActionKind::Merge | PrActionKind::MergeRemove => {
            if merge_block(pr).0.is_some() {
                return Some(if pr.is_draft {
                    "draft -> ready first"
                } else if pr.mergeable.as_deref() == Some("CONFLICTING") {
                    "conflicting"
                } else if matches!(pr.checks, Some(CheckState::Failure { .. })) {
                    "CI red"
                } else {
                    "mergeable unknown"
                });
            }
            if kind == PrActionKind::MergeRemove && dirty {
                return Some("uncommitted changes");
            }
            None
        }
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Plan an action. Pure: returns the plan or a refusal message.
pub fn plan_pr_action(kind: PrActionKind, t: &PrTarget) -> Result<PrPlan, String> {
    if let Some(reason) = pr_menu_state(kind, t.pr.as_ref(), t.prs_loading, t.has_gh, t.dirty) {
        return Err(format!("{}: {reason}", kind.label()));
    }
    let branch = t.branch.as_str();
    if kind == PrActionKind::Push {
        let mut steps = vec![argv(&["git", "push", "-u", "origin", branch])];
        let mut lines = vec![format!("Push '{branch}' to origin (no force).")];
        let has_open_pr = t.pr.as_ref().is_some_and(|pr| pr.state == "OPEN");
        if !t.has_gh {
            lines.push("gh not found: push only, no PR.".to_string());
        } else if let (true, Some(pr)) = (has_open_pr, t.pr.as_ref()) {
            lines.push(format!("PR #{} already open: push only.", pr.number));
        } else {
            let Some(base) = t.base.as_deref() else {
                return Err("No base branch known for the PR".to_string());
            };
            steps.push(argv(&[
                "gh", "pr", "create", "--draft", "--base", base, "--head", branch, "--title",
                branch, "--body", "",
            ]));
            lines.push(format!("Create draft PR '{branch}' -> '{base}'."));
        }
        return Ok(PrPlan {
            title: "Push / PR",
            lines,
            steps,
            remove_after: false,
            warning: None,
        });
    }

    // pr_menu_state guarantees an open PR from here on.
    let pr = t.pr.as_ref().ok_or("No PR for this branch")?;
    let num = pr.number.to_string();
    let plan = match kind {
        PrActionKind::ToggleReady => {
            let mut step = argv(&["gh", "pr", "ready", &num]);
            if !pr.is_draft {
                step.push("--undo".to_string());
            }
            let what = if pr.is_draft {
                "ready for review"
            } else {
                "draft"
            };
            PrPlan {
                title: "PR status",
                lines: vec![format!("Mark PR #{num} as {what}.")],
                steps: vec![step],
                remove_after: false,
                warning: None,
            }
        }
        PrActionKind::Close => PrPlan {
            title: "Close PR",
            lines: vec![format!(
                "Close PR #{num} without merging (branch and worktree stay)."
            )],
            steps: vec![argv(&["gh", "pr", "close", &num])],
            remove_after: false,
            warning: None,
        },
        PrActionKind::Merge | PrActionKind::MergeRemove => {
            let warning = merge_block(pr).1;
            let remove = kind == PrActionKind::MergeRemove;
            let mut lines = vec![format!(
                "Merge PR #{num} (merge commit, remote branch kept)."
            )];
            if remove {
                lines.push(format!(
                    "Then remove worktree '{}' and its local branch.",
                    t.handle
                ));
            }
            PrPlan {
                title: if remove {
                    "Merge PR + remove"
                } else {
                    "Merge PR"
                },
                lines,
                steps: vec![argv(&["gh", "pr", "merge", &num, "--merge"])],
                remove_after: remove,
                warning,
            }
        }
        PrActionKind::Push => unreachable!(),
    };
    Ok(plan)
}

/// Shell-quoted command line for display.
pub fn display_cmd(step: &[String]) -> String {
    step.iter()
        .map(|p| {
            if !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./:=@+#".contains(c))
            {
                p.clone()
            } else {
                format!("'{}'", p.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn run_steps(steps: &[Vec<String>], cwd: &Path) -> Result<String, String> {
    let mut last = String::new();
    for step in steps {
        let output = Command::new(&step[0])
            .args(&step[1..])
            .current_dir(cwd)
            .output()
            .map_err(|e| format!("{}: {e}", step[0]))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = stderr
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("");
            return Err(format!("`{}` failed: {}", display_cmd(step), msg.trim()));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(line) = stdout.lines().rev().find(|l| !l.trim().is_empty()) {
            last = line.trim().to_string();
        }
    }
    Ok(last)
}

fn remove_worktree(
    handle: &str,
    path: &Path,
    mux: Arc<dyn Multiplexer>,
) -> anyhow::Result<workflow::types::RemoveResult> {
    let (config, config_location) = crate::config::Config::load_with_location_from(path, None)?;
    let ctx = workflow::WorkflowContext::new_in(path, config, mux, config_location)?;
    workflow::remove_quiet(handle, true, false, &ctx)
}

/// Outcome of a background PR action.
pub struct PrActionOutcome {
    pub title: &'static str,
    pub path: PathBuf,
    pub result: Result<String, String>,
    /// Present when the plan removed the worktree after the steps.
    pub removal: Option<Result<workflow::types::RemoveResult, String>>,
}

impl App {
    fn selected_pr_target(&self) -> Option<PrTarget> {
        let wt = self.worktrees.get(self.worktree_table_state.selected()?)?;
        if wt.is_main || wt.branch == "(detached)" {
            return None;
        }
        let base = wt
            .base_branch
            .clone()
            .or_else(|| git::get_default_branch_in(Some(&wt.path)).ok());
        let pr = wt.pr_info.clone();
        Some(PrTarget {
            handle: wt.handle.clone(),
            branch: wt.branch.clone(),
            path: wt.path.clone(),
            base,
            pr,
            dirty: git::has_uncommitted_changes(&wt.path).unwrap_or(true),
            has_gh: which::which("gh").is_ok(),
            prs_loading: !self.prs_loaded && self.is_pr_fetching(),
        })
    }

    /// Open the PR submenu for the selected worktree.
    pub fn show_pr_menu(&mut self) {
        let Some(target) = self.selected_pr_target() else {
            return;
        };
        if self.removals.contains(&target.path) {
            return;
        }
        self.pending_pr_menu = Some(PrMenu { target });
    }

    /// Pick a submenu entry: open the confirmation or report the refusal.
    pub fn pr_menu_select(&mut self, kind: PrActionKind) {
        let Some(menu) = self.pending_pr_menu.take() else {
            return;
        };
        match plan_pr_action(kind, &menu.target) {
            Ok(plan) => {
                self.pending_pr_confirm = Some(PrConfirm {
                    target: menu.target,
                    plan,
                })
            }
            Err(msg) => {
                // Keep the menu open on a disabled entry, explain in the status line.
                self.pending_pr_menu = Some(menu);
                self.status_message = Some((msg, std::time::Instant::now()));
            }
        }
    }

    /// Run the confirmed plan in a background thread.
    pub fn confirm_pr_action(&mut self) {
        let Some(confirm) = self.pending_pr_confirm.take() else {
            return;
        };
        let PrConfirm { target, plan } = confirm;
        if plan.remove_after && !self.removals.start(target.path.clone()) {
            return;
        }
        self.status_message = Some((
            format!("{}: running...", plan.title),
            std::time::Instant::now(),
        ));
        let mux = self.mux.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let mut result = if plan.remove_after
                && git::has_uncommitted_changes(&target.path).unwrap_or(true)
            {
                Err("worktree has uncommitted changes; nothing merged".to_string())
            } else {
                run_steps(&plan.steps, &target.path)
            };
            let mut removal = None;
            if plan.remove_after {
                removal = Some(match &result {
                    Ok(_) => remove_worktree(&target.handle, &target.path, mux)
                        .map_err(|e| format!("{e:#}")),
                    Err(e) => Err(e.clone()),
                });
                if let Some(Err(e)) = &removal
                    && result.is_ok()
                {
                    result = Err(format!("merged, but removing the worktree failed: {e}"));
                }
            }
            let _ = tx.send(AppEvent::PrActionResult(PrActionOutcome {
                title: plan.title,
                path: target.path,
                result,
                removal,
            }));
        });
    }

    pub(super) fn handle_pr_action_result(&mut self, outcome: PrActionOutcome) {
        let PrActionOutcome {
            title,
            path,
            result,
            removal,
        } = outcome;
        if let Some(removal) = removal {
            match removal {
                Ok(r) => self.handle_remove_worktree_result(path, Ok(r)),
                // The worktree was not touched: just release the removal slot.
                Err(_) => {
                    self.removals.finish(&path, false);
                    self.trigger_worktree_refetch();
                }
            }
        }
        let msg = match result {
            Ok(out) if out.is_empty() => format!("{title}: done"),
            Ok(out) => format!("{title}: done ({out})"),
            Err(e) => format!("{title} failed: {e}"),
        };
        self.status_message = Some((msg, std::time::Instant::now()));
        self.trigger_pr_refetch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(
        state: &str,
        draft: bool,
        mergeable: Option<&str>,
        checks: Option<CheckState>,
    ) -> PrSummary {
        PrSummary {
            number: 7,
            title: "t".into(),
            state: state.into(),
            is_draft: draft,
            mergeable: mergeable.map(Into::into),
            checks,
            check_meta: None,
            url: None,
            closing_issues: Vec::new(),
        }
    }

    fn target(pr: Option<PrSummary>) -> PrTarget {
        PrTarget {
            handle: "feat".into(),
            branch: "feat".into(),
            path: PathBuf::from("/tmp/feat"),
            base: Some("main".into()),
            pr,
            dirty: false,
            has_gh: true,
            prs_loading: false,
        }
    }

    fn state(t: &PrTarget, k: PrActionKind) -> Option<&'static str> {
        pr_menu_state(k, t.pr.as_ref(), t.prs_loading, t.has_gh, t.dirty)
    }

    #[test]
    fn keys_round_trip() {
        for k in PrActionKind::ALL {
            assert_eq!(PrActionKind::from_key(k.key()), Some(k));
        }
        assert_eq!(PrActionKind::from_key('x'), None);
    }

    #[test]
    fn loading_disables_everything() {
        let mut t = target(None);
        t.prs_loading = true;
        for k in PrActionKind::ALL {
            assert_eq!(state(&t, k), Some("PRs loading"));
        }
    }

    #[test]
    fn no_gh_allows_push_only() {
        let mut t = target(None);
        t.has_gh = false;
        assert_eq!(state(&t, PrActionKind::Push), None);
        assert_eq!(state(&t, PrActionKind::Merge), Some("no gh"));
        let plan = plan_pr_action(PrActionKind::Push, &t).unwrap();
        assert_eq!(
            plan.steps,
            vec![argv(&["git", "push", "-u", "origin", "feat"])]
        );
    }

    #[test]
    fn no_pr_push_creates_draft() {
        let t = target(None);
        assert_eq!(state(&t, PrActionKind::ToggleReady), Some("no PR"));
        let plan = plan_pr_action(PrActionKind::Push, &t).unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(
            plan.steps[1],
            argv(&[
                "gh", "pr", "create", "--draft", "--base", "main", "--head", "feat", "--title",
                "feat", "--body", ""
            ])
        );
    }

    #[test]
    fn open_pr_push_only() {
        let t = target(Some(pr("OPEN", true, None, None)));
        let plan = plan_pr_action(PrActionKind::Push, &t).unwrap();
        assert_eq!(plan.steps.len(), 1);
    }

    #[test]
    fn closed_pr_disables_actions() {
        let t = target(Some(pr("MERGED", false, None, None)));
        assert_eq!(state(&t, PrActionKind::Close), Some("PR not open"));
        assert!(plan_pr_action(PrActionKind::Merge, &t).is_err());
        // A new draft PR can be created when the old one is not open.
        assert_eq!(
            plan_pr_action(PrActionKind::Push, &t).unwrap().steps.len(),
            2
        );
    }

    #[test]
    fn draft_toggle_direction() {
        let t = target(Some(pr("OPEN", true, Some("MERGEABLE"), None)));
        let plan = plan_pr_action(PrActionKind::ToggleReady, &t).unwrap();
        assert_eq!(plan.steps, vec![argv(&["gh", "pr", "ready", "7"])]);
        assert_eq!(state(&t, PrActionKind::Merge), Some("draft -> ready first"));
        let t = target(Some(pr("OPEN", false, Some("MERGEABLE"), None)));
        let plan = plan_pr_action(PrActionKind::ToggleReady, &t).unwrap();
        assert_eq!(
            plan.steps,
            vec![argv(&["gh", "pr", "ready", "7", "--undo"])]
        );
    }

    #[test]
    fn merge_refusals() {
        let t = target(Some(pr("OPEN", false, Some("CONFLICTING"), None)));
        assert_eq!(state(&t, PrActionKind::Merge), Some("conflicting"));
        let failing = CheckState::Failure {
            passed: 1,
            total: 2,
        };
        let t = target(Some(pr("OPEN", false, Some("MERGEABLE"), Some(failing))));
        assert_eq!(state(&t, PrActionKind::MergeRemove), Some("CI red"));
        let t = target(Some(pr("OPEN", false, Some("UNKNOWN"), None)));
        assert_eq!(state(&t, PrActionKind::Merge), Some("mergeable unknown"));
    }

    #[test]
    fn merge_and_remove_plan() {
        let pending = CheckState::Pending {
            passed: 0,
            total: 1,
        };
        let mut t = target(Some(pr("OPEN", false, Some("MERGEABLE"), Some(pending))));
        let plan = plan_pr_action(PrActionKind::MergeRemove, &t).unwrap();
        assert!(plan.remove_after);
        assert!(plan.warning.is_some());
        assert_eq!(
            plan.steps,
            vec![argv(&["gh", "pr", "merge", "7", "--merge"])]
        );
        t.dirty = true;
        assert_eq!(
            state(&t, PrActionKind::MergeRemove),
            Some("uncommitted changes")
        );
        assert_eq!(state(&t, PrActionKind::Merge), None);
    }

    #[test]
    fn close_plan() {
        let t = target(Some(pr("OPEN", false, None, None)));
        let plan = plan_pr_action(PrActionKind::Close, &t).unwrap();
        assert_eq!(plan.steps, vec![argv(&["gh", "pr", "close", "7"])]);
    }

    #[test]
    fn display_cmd_quotes() {
        assert_eq!(
            display_cmd(&argv(&[
                "gh", "pr", "create", "--title", "a b", "--body", ""
            ])),
            "gh pr create --title 'a b' --body ''"
        );
    }
}
