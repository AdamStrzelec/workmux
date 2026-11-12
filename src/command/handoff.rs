//! `workmux handoff`: note for the next person or agent taking over a worktree.

use anyhow::Result;
use serde::Serialize;

use crate::guard::{changed_files, git_lines};

#[derive(Debug, Serialize)]
pub struct Handoff {
    pub branch: String,
    pub base: String,
    pub commits: Vec<String>,
    pub uncommitted: Vec<String>,
    pub changed_files: Vec<String>,
    pub snapshots: usize,
    pub prompts: Vec<String>,
}

pub fn render(h: &Handoff) -> String {
    let mut out = format!("# Handoff: {}\n\nBase: {}\n", h.branch, h.base);
    let section = |out: &mut String, title: &str, items: &[String]| {
        out.push_str(&format!("\n## {title} ({})\n", items.len()));
        for i in items {
            out.push_str(&format!("- {i}\n"));
        }
    };
    section(&mut out, "Commits since base", &h.commits);
    section(&mut out, "Uncommitted changes", &h.uncommitted);
    section(&mut out, "All changed files", &h.changed_files);
    if !h.prompts.is_empty() {
        section(&mut out, "Recent prompts to the agent", &h.prompts);
    }
    out.push_str(&format!("\nSnapshots on this branch: {}\n", h.snapshots));
    out
}

pub fn run(json: bool, prompts: usize) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let branch = crate::git::get_current_branch_in(&cwd).unwrap_or_else(|_| "detached".into());
    let base = crate::git::get_default_branch_in(Some(&cwd))?;
    let range = format!("{base}..HEAD");
    let prompts = if prompts == 0 {
        Vec::new()
    } else {
        crate::history::collect(&cwd, false)
            .first()
            .map(|s| crate::history::read_messages(&s.path))
            .map(|m| crate::history::recent_prompts(&m, prompts))
            .unwrap_or_default()
            .iter()
            .map(|p| crate::history::clip(&p.split_whitespace().collect::<Vec<_>>().join(" "), 300))
            .collect()
    };
    let h = Handoff {
        prompts,
        commits: git_lines(&cwd, &["log", "--format=%h %s", &range])?,
        uncommitted: git_lines(&cwd, &["status", "--porcelain"])?,
        changed_files: changed_files(&cwd, &base)?,
        snapshots: crate::snapshot::list(&cwd).map(|s| s.len()).unwrap_or(0),
        branch,
        base,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&h)?);
    } else {
        print!("{}", render(&h));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_lists_sections() {
        let h = Handoff {
            branch: "feat".into(),
            base: "main".into(),
            commits: vec!["abc1234 add x".into()],
            uncommitted: vec![],
            changed_files: vec!["x.rs".into()],
            snapshots: 2,
            prompts: vec!["do x".into()],
        };
        let t = render(&h);
        assert!(t.contains("# Handoff: feat"));
        assert!(t.contains("## Commits since base (1)\n- abc1234 add x"));
        assert!(t.contains("## Uncommitted changes (0)"));
        assert!(t.contains("## Recent prompts to the agent (1)\n- do x"));
        assert!(t.contains("Snapshots on this branch: 2"));
    }
}
