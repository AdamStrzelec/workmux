//! `workmux consult`: ask another model about files and/or the branch diff.

use std::io::{IsTerminal, Read, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::config::Config;
use crate::{consult, git, guard};

pub fn run(question: Option<String>, files: Vec<String>, diff: bool, review: bool) -> Result<()> {
    let cfg = Config::global_consult_config();
    let Some(cmdline) = cfg
        .command
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    else {
        bail!(
            "no model command: set `consult.command` in the global config (e.g. `llm -m gemini`)"
        );
    };
    let guard_cfg = Config::global_guard_config();
    let max = cfg.max_bytes();
    let cwd = std::env::current_dir()?;
    let root = git::get_repo_root_in(Some(&cwd))?;

    // Piped stdin is the question, or in review mode possibly the diff.
    let mut piped = String::new();
    if !std::io::stdin().is_terminal() {
        std::io::stdin().read_to_string(&mut piped)?;
    }

    let mut contents = Vec::new();
    let mut skipped = Vec::new();
    for f in &files {
        let abs = cwd.join(f);
        let canon = abs
            .canonicalize()
            .with_context(|| format!("cannot read {f}"))?;
        let Ok(rel) = canon.strip_prefix(root.canonicalize()?) else {
            bail!("{f} is outside the repository");
        };
        let rel = rel.to_string_lossy().to_string();
        let (allowed, dropped) = consult::split_protected(&guard_cfg, std::slice::from_ref(&rel));
        if allowed.is_empty() {
            skipped.extend(dropped);
            continue;
        }
        let text = std::fs::read_to_string(&canon).with_context(|| format!("cannot read {f}"))?;
        contents.push((rel, text));
    }

    let mut diff_text = None;
    if diff || review {
        diff_text = Some(if review && !piped.trim().is_empty() {
            std::mem::take(&mut piped)
        } else {
            branch_diff(&cwd, &guard_cfg, &mut skipped)?
        });
    }
    if review && diff_text.as_deref().is_none_or(|d| d.trim().is_empty()) {
        bail!("nothing to review: the branch has no diff");
    }

    // Spend the byte budget over the context parts, files first.
    let mut budget = max;
    for (_, c) in contents.iter_mut() {
        *c = consult::truncate(c, budget);
        budget = budget.saturating_sub(c.len());
    }
    let diff_text = diff_text.map(|d| consult::truncate(&d, budget));

    let question = question.unwrap_or_default();
    let question = if question.is_empty() { piped } else { question };
    let prompt = consult::build_prompt(review, &question, &contents, diff_text.as_deref());
    if prompt.trim().is_empty() {
        bail!("nothing to send: give a question, --file, or --diff");
    }

    let program = shlex::split(cmdline)
        .and_then(|p| p.first().cloned())
        .unwrap_or_default();
    eprintln!(
        "Sending {} bytes ({} files{}) to `{program}`.",
        prompt.len(),
        contents.len(),
        if diff_text.is_some() { " + diff" } else { "" }
    );
    if !skipped.is_empty() {
        eprintln!("Left out as protected: {}", skipped.join(", "));
    }

    let answer = ask(cmdline, &prompt)?;
    println!("{}", answer.trim_end());
    if review && !consult::approved(&answer) {
        crate::audit::record_action("consult review rejected".into(), false);
        std::process::exit(1);
    }
    Ok(())
}

fn branch_diff(
    cwd: &std::path::Path,
    guard_cfg: &guard::GuardConfig,
    skipped: &mut Vec<String>,
) -> Result<String> {
    let default = git::get_default_branch()?;
    let base = git::get_merge_base_in(Some(cwd), &default)?;
    let range = format!("{base}...HEAD");
    let changed = guard::git_lines(cwd, &["diff", "--name-only", &range])?;
    let (allowed, dropped) = consult::split_protected(guard_cfg, &changed);
    skipped.extend(dropped);
    if allowed.is_empty() {
        return Ok(String::new());
    }
    let mut cmd = git::unattended_git(Some(cwd))?;
    cmd.args(["diff", "--no-ext-diff", &range, "--"])
        .args(&allowed);
    let out = cmd.output().context("failed to run git diff")?;
    if !out.status.success() {
        bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn ask(cmdline: &str, prompt: &str) -> Result<String> {
    let parts = shlex::split(cmdline).context("mismatched quotes in consult.command")?;
    let (program, args) = parts.split_first().context("consult.command is empty")?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start `{program}`"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes());
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!(
            "`{program}` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
