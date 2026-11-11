//! `workmux history`: search and read local Claude Code conversations.

use anyhow::{Result, bail};
use serde::Serialize;

use crate::history::{self, SessionFile};

#[derive(Serialize)]
struct Row {
    id: String,
    project: String,
    age: String,
    messages: usize,
    matches: Option<usize>,
    first_prompt: String,
    snippet: Option<String>,
}

fn row(s: &SessionFile, query: Option<&str>) -> Option<Row> {
    let msgs = history::read_messages(&s.path);
    let (matches, snippet) = match query {
        Some(q) => {
            let n = history::matches(&msgs, q);
            if n == 0 {
                return None;
            }
            let sn = msgs.iter().find_map(|m| history::snippet(&m.text, q, 50));
            (Some(n), sn)
        }
        None => (None, None),
    };
    let first = msgs
        .iter()
        .find(|m| m.role == "user")
        .map(|m| m.text.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    Some(Row {
        id: s.id.clone(),
        project: s.project.clone(),
        age: history::age(s.modified),
        messages: msgs.len(),
        matches,
        first_prompt: history::clip(&first, 70),
        snippet,
    })
}

pub fn list(query: Option<String>, all: bool, limit: usize, json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let q = query.as_deref().filter(|q| !q.trim().is_empty());
    let rows: Vec<Row> = history::collect(&cwd, all)
        .iter()
        .filter_map(|s| row(s, q))
        .take(limit)
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("No conversations found.");
        return Ok(());
    }
    for r in &rows {
        let hits = r.matches.map(|n| format!(" {n} hits")).unwrap_or_default();
        let proj = if all {
            format!(" [{}]", r.project)
        } else {
            String::new()
        };
        println!(
            "{}  {:>4} ago  {:>4} msgs{}{}  {}",
            &r.id[..r.id.len().min(8)],
            r.age,
            r.messages,
            hits,
            proj,
            r.first_prompt
        );
        if let Some(sn) = &r.snippet {
            println!("      {sn}");
        }
    }
    Ok(())
}

pub fn show(id: &str, all: bool, last: usize, json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let found: Vec<SessionFile> = history::collect(&cwd, all)
        .into_iter()
        .filter(|s| s.id.starts_with(id))
        .collect();
    let s = match found.as_slice() {
        [] => bail!("no conversation with id starting '{id}' (try --all)"),
        [one] => one,
        _ => bail!("id prefix '{id}' is ambiguous ({} matches)", found.len()),
    };
    let msgs = history::read_messages(&s.path);
    let skip = msgs.len().saturating_sub(last);
    let msgs = &msgs[skip..];
    if json {
        println!("{}", serde_json::to_string_pretty(msgs)?);
        return Ok(());
    }
    println!("# {} ({} ago)\n", s.id, history::age(s.modified));
    for m in msgs {
        println!("[{}] {}\n", m.role, history::clip(&m.text, 2000));
    }
    Ok(())
}
