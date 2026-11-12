//! `workmux enforce`: install git hooks that block commits and pushes on
//! protected branches, independent of workmux.

use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::enforce;

fn hooks_dir() -> Result<PathBuf> {
    let common = crate::git::get_git_common_dir_in(None)?;
    if let Ok(cfg) = std::fs::read_to_string(common.join("config"))
        && cfg.to_lowercase().contains("hookspath")
    {
        bail!("core.hooksPath is set in this repo; install the hooks there by hand");
    }
    Ok(common.join("hooks"))
}

pub fn install(branches: Vec<String>) -> Result<()> {
    let branches = if branches.is_empty() {
        let mut v = vec!["main".to_string(), "master".to_string()];
        if let Ok(d) = crate::git::get_default_branch()
            && !v.contains(&d)
        {
            v.push(d);
        }
        v
    } else {
        branches
    };
    enforce::validate_branches(&branches).map_err(anyhow::Error::msg)?;
    let dir = hooks_dir()?;
    // Check everything first so a refusal leaves the repo untouched.
    for hook in enforce::HOOKS {
        let path = dir.join(hook);
        if let Ok(text) = std::fs::read_to_string(&path)
            && !enforce::is_ours(&text)
        {
            bail!(
                "{} already exists and is not managed by workmux; merge by hand or remove it first",
                path.display()
            );
        }
    }
    std::fs::create_dir_all(&dir)?;
    for hook in enforce::HOOKS {
        let path = dir.join(hook);
        std::fs::write(&path, enforce::script(hook, &branches))?;
        set_executable(&path)?;
    }
    println!(
        "Installed {} hooks for: {}",
        enforce::HOOKS.len(),
        branches.join(", ")
    );
    println!(
        "Bypass once with {}=1; `git --no-verify` also skips them.",
        enforce::BYPASS_ENV
    );
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let dir = hooks_dir()?;
    let mut removed = 0;
    for hook in enforce::HOOKS {
        let path = dir.join(hook);
        if std::fs::read_to_string(&path).is_ok_and(|t| enforce::is_ours(&t)) {
            std::fs::remove_file(&path)?;
            removed += 1;
        }
    }
    println!("Removed {removed} workmux hooks.");
    Ok(())
}

pub fn status(json: bool) -> Result<()> {
    let dir = hooks_dir()?;
    let mut rows = Vec::new();
    for hook in enforce::HOOKS {
        let text = std::fs::read_to_string(dir.join(hook)).ok();
        let state = match &text {
            None => "missing",
            Some(t) if enforce::is_ours(t) => "workmux",
            Some(_) => "other",
        };
        let branches = text
            .as_deref()
            .and_then(enforce::branches_of)
            .unwrap_or_default();
        rows.push((hook, state, branches));
    }
    if json {
        let v: Vec<_> = rows
            .iter()
            .map(|(h, s, b)| serde_json::json!({"hook": h, "state": s, "branches": b}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        for (h, s, b) in &rows {
            println!(
                "{h}: {s}{}",
                if b.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", b.join(", "))
                }
            );
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_: &std::path::Path) -> Result<()> {
    Ok(())
}
