//! Listening TCP ports: which process holds them, in which worktree and
//! which tmux window (run tool) it runs. Used by `workmux ports`, the
//! dashboard and the busy-port warning when a run tool starts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listener {
    pub port: u16,
    pub pid: u32,
    pub command: String,
    /// Working directory of the process.
    pub cwd: Option<PathBuf>,
    /// Git worktree root containing `cwd`.
    pub worktree: Option<PathBuf>,
    /// tmux `(session, window)` the process runs in.
    pub session: Option<String>,
    pub window: Option<String>,
}

impl Listener {
    /// Tool name when the window is a run tool window (`<handle>/<tool>`),
    /// else the window name.
    pub fn tool(&self) -> Option<&str> {
        let window = self.window.as_deref()?;
        Some(
            window
                .rsplit_once(crate::claude_launch::TOOL_WINDOW_SEP)
                .map_or(window, |(_, t)| t),
        )
    }
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    // lsof exits 1 when nothing matches; output is still valid.
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse `lsof -F pcn` output into `(pid, command, port)`, deduplicated.
fn parse_listen(output: &str) -> Vec<(u32, String, u16)> {
    let mut found = Vec::new();
    let (mut pid, mut command) = (0u32, String::new());
    for line in output.lines() {
        let (tag, value) = line.split_at(line.len().min(1));
        match tag {
            "p" => pid = value.parse().unwrap_or(0),
            "c" => command = value.to_string(),
            "n" => {
                if let Some(port) = value.rsplit(':').next().and_then(|p| p.parse().ok())
                    && pid != 0
                {
                    let entry = (pid, command.clone(), port);
                    if !found.contains(&entry) {
                        found.push(entry);
                    }
                }
            }
            _ => {}
        }
    }
    found
}

/// Parse `lsof -F pn` output (cwd query) into pid -> cwd.
fn parse_cwd(output: &str) -> HashMap<u32, PathBuf> {
    let mut map = HashMap::new();
    let mut pid = 0u32;
    for line in output.lines() {
        let (tag, value) = line.split_at(line.len().min(1));
        match tag {
            "p" => pid = value.parse().unwrap_or(0),
            "n" if pid != 0 => {
                map.insert(pid, PathBuf::from(value));
            }
            _ => {}
        }
    }
    map
}

/// Nearest ancestor of `dir` containing `.git` (file or directory).
pub fn worktree_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

/// pid -> parent pid for every process.
fn parent_map() -> HashMap<u32, u32> {
    run("ps", &["-A", "-o", "pid=,ppid="])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
        })
        .collect()
}

/// pane pid -> (session, window) for every tmux pane.
fn tmux_panes() -> HashMap<u32, (String, String)> {
    run(
        "tmux",
        &[
            "list-panes",
            "-a",
            "-F",
            "#{pane_pid}\t#{session_name}\t#{window_name}",
        ],
    )
    .unwrap_or_default()
    .lines()
    .filter_map(|l| {
        let mut it = l.splitn(3, '\t');
        Some((
            it.next()?.parse().ok()?,
            (it.next()?.to_string(), it.next()?.to_string()),
        ))
    })
    .collect()
}

fn pane_of(
    pid: u32,
    parents: &HashMap<u32, u32>,
    panes: &HashMap<u32, (String, String)>,
) -> Option<(String, String)> {
    let mut current = pid;
    for _ in 0..64 {
        if let Some(pane) = panes.get(&current) {
            return Some(pane.clone());
        }
        current = *parents.get(&current)?;
        if current <= 1 {
            return None;
        }
    }
    None
}

/// Every listening TCP port on this machine, sorted by port.
pub fn scan() -> Vec<Listener> {
    let listen = parse_listen(
        &run("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"]).unwrap_or_default(),
    );
    if listen.is_empty() {
        return Vec::new();
    }
    let mut pids: Vec<String> = listen.iter().map(|(p, _, _)| p.to_string()).collect();
    pids.sort();
    pids.dedup();
    let cwds = parse_cwd(
        &run(
            "lsof",
            &["-a", "-d", "cwd", "-F", "pn", "-p", &pids.join(",")],
        )
        .unwrap_or_default(),
    );
    let parents = parent_map();
    let panes = tmux_panes();

    let mut listeners: Vec<Listener> = listen
        .into_iter()
        .map(|(pid, command, port)| {
            let cwd = cwds.get(&pid).cloned();
            let worktree = cwd.as_deref().and_then(worktree_root);
            let (session, window) = pane_of(pid, &parents, &panes).unzip();
            Listener {
                port,
                pid,
                command,
                cwd,
                worktree,
                session,
                window,
            }
        })
        .collect();
    listeners.sort_by_key(|l| (l.port, l.pid));
    listeners
}

/// Who holds `port`, if anyone.
pub fn holder(listeners: &[Listener], port: u16) -> Option<&Listener> {
    listeners.iter().find(|l| l.port == port)
}

/// Warning text when `port` (needed by `tool` in `worktree`) is already taken.
pub fn busy_warning(listeners: &[Listener], port: u16, tool: &str) -> Option<String> {
    let l = holder(listeners, port)?;
    let place = match (&l.worktree, &l.window) {
        (Some(wt), _) => format!(
            " in worktree '{}'",
            wt.file_name().map_or_else(
                || wt.display().to_string(),
                |n| n.to_string_lossy().into_owned()
            )
        ),
        (None, Some(w)) => format!(" in window '{w}'"),
        _ => String::new(),
    };
    Some(format!(
        "Port {port} for '{tool}' is already in use by {} (pid {}){place}",
        l.command, l.pid
    ))
}

/// Ports currently held by a listener, plus ports a bind attempt rejects.
pub fn taken_ports(listeners: &[Listener]) -> std::collections::HashSet<u16> {
    listeners.iter().map(|l| l.port).collect()
}

/// First port from `preferred` upward (at most 100 tries) that is not in
/// `taken` and can be bound on localhost.
pub fn pick_free(preferred: u16, taken: &std::collections::HashSet<u16>) -> Option<u16> {
    (preferred..=preferred.saturating_add(100))
        .find(|p| !taken.contains(p) && std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
}

/// Command to run for a tool whose preferred port is `wanted` and chosen port
/// is `chosen`: `{port}` is replaced; when the port moved, `PORT=<chosen>` is
/// also set so frameworks that read it follow.
pub fn apply_port(command: &str, wanted: u16, chosen: u16) -> String {
    let replaced = command.replace("{port}", &chosen.to_string());
    if chosen == wanted && replaced == command {
        return replaced;
    }
    format!("PORT={chosen} {replaced}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lsof_listen_and_dedupes_ipv4_ipv6() {
        let out = "p4123\ncnode\nn*:3000\nn[::1]:3000\np99\ncpostgres\nn127.0.0.1:5432\n";
        assert_eq!(
            parse_listen(out),
            vec![
                (4123, "node".to_string(), 3000),
                (99, "postgres".to_string(), 5432)
            ]
        );
    }

    #[test]
    fn parses_cwd() {
        let out = "p4123\nfcwd\nn/tmp/a\np5\nfcwd\nn/tmp/b\n";
        let map = parse_cwd(out);
        assert_eq!(map.get(&4123), Some(&PathBuf::from("/tmp/a")));
        assert_eq!(map.get(&5), Some(&PathBuf::from("/tmp/b")));
    }

    fn listener(port: u16, window: Option<&str>, wt: Option<&str>) -> Listener {
        Listener {
            port,
            pid: 7,
            command: "node".into(),
            cwd: None,
            worktree: wt.map(PathBuf::from),
            session: None,
            window: window.map(String::from),
        }
    }

    #[test]
    fn tool_name_from_window() {
        assert_eq!(listener(1, Some("wm-feat/dev"), None).tool(), Some("dev"));
        assert_eq!(listener(1, Some("agent"), None).tool(), Some("agent"));
        assert_eq!(listener(1, None, None).tool(), None);
    }

    #[test]
    fn busy_warning_names_holder() {
        let ls = vec![listener(3000, Some("x/dev"), Some("/r/feat"))];
        assert_eq!(
            busy_warning(&ls, 3000, "dev").as_deref(),
            Some("Port 3000 for 'dev' is already in use by node (pid 7) in worktree 'feat'")
        );
        assert!(busy_warning(&ls, 3001, "dev").is_none());
    }

    #[test]
    fn pane_lookup_walks_parents() {
        let parents = HashMap::from([(30, 20), (20, 10), (10, 1)]);
        let panes = HashMap::from([(10, ("s".to_string(), "w".to_string()))]);
        assert_eq!(
            pane_of(30, &parents, &panes),
            Some(("s".to_string(), "w".to_string()))
        );
        assert_eq!(pane_of(40, &parents, &panes), None);
    }

    #[test]
    fn pick_free_skips_taken_ports() {
        let taken: std::collections::HashSet<u16> = [40100, 40101].into_iter().collect();
        let p = pick_free(40100, &taken).unwrap();
        assert!(p >= 40102);
        assert_eq!(
            pick_free(40200, &Default::default()).map(|p| p >= 40200),
            Some(true)
        );
    }

    #[test]
    fn apply_port_substitutes_and_exports() {
        assert_eq!(apply_port("npm run dev", 3000, 3000), "npm run dev");
        assert_eq!(
            apply_port("npm run dev", 3000, 3002),
            "PORT=3002 npm run dev"
        );
        assert_eq!(
            apply_port("vite --port {port}", 3000, 3000),
            "PORT=3000 vite --port 3000"
        );
    }
}
