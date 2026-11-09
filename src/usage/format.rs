//! Rendering helpers for usage windows.

use super::{Snapshot, Window};

/// Utilization severity, used for colouring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Critical,
}

pub fn level(percent: u8) -> Level {
    match percent {
        0..=49 => Level::Ok,
        50..=79 => Level::Warn,
        _ => Level::Critical,
    }
}

/// Compact duration: `2d3h`, `4h12m`, `37m`, `<1m`.
pub fn format_duration(secs: i64) -> String {
    if secs < 60 {
        return "<1m".to_string();
    }
    let mins = secs / 60;
    let (days, hours, m) = (mins / 1440, (mins % 1440) / 60, mins % 60);
    if days > 0 {
        format!("{days}d{hours}h")
    } else if hours > 0 {
        format!("{hours}h{m:02}m")
    } else {
        format!("{m}m")
    }
}

/// Seconds until the window resets, if known and still in the future.
pub fn until_reset(w: &Window, now: i64) -> Option<i64> {
    w.resets_at_unix.map(|t| t - now).filter(|d| *d > 0)
}

/// `28% (resets in 2h13m)` or `n/a`.
pub fn describe(w: &Window, now: i64) -> String {
    let Some(p) = w.used_percent else {
        return "n/a".to_string();
    };
    match until_reset(w, now) {
        Some(d) => format!("{p}% (resets in {})", format_duration(d)),
        None => format!("{p}%"),
    }
}

fn tmux_color(l: Level) -> &'static str {
    match l {
        Level::Ok => "green",
        Level::Warn => "yellow",
        Level::Critical => "red",
    }
}

/// One-line string for the tmux status bar, e.g.
/// `#[fg=green]5h 28%#[default] #[fg=yellow]7d 63%#[default]`.
pub fn tmux_status(snap: &Snapshot) -> String {
    let mut parts = Vec::new();
    for (label, w) in [("5h", &snap.session), ("7d", &snap.weekly)] {
        if let Some(p) = w.as_ref().and_then(|w| w.used_percent) {
            parts.push(format!(
                "#[fg={}]{label} {p}%#[default]",
                tmux_color(level(p))
            ));
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(p: u8, resets: Option<i64>) -> Window {
        Window {
            used_percent: Some(p),
            window_minutes: None,
            resets_at_unix: resets,
        }
    }

    #[test]
    fn levels() {
        assert_eq!(level(49), Level::Ok);
        assert_eq!(level(50), Level::Warn);
        assert_eq!(level(79), Level::Warn);
        assert_eq!(level(80), Level::Critical);
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration(30), "<1m");
        assert_eq!(format_duration(37 * 60), "37m");
        assert_eq!(format_duration(4 * 3600 + 12 * 60), "4h12m");
        assert_eq!(format_duration(2 * 86400 + 3 * 3600), "2d3h");
    }

    #[test]
    fn describe_with_and_without_reset() {
        assert_eq!(
            describe(&win(28, Some(1000 + 3600)), 1000),
            "28% (resets in 1h00m)"
        );
        assert_eq!(describe(&win(28, Some(500)), 1000), "28%");
        assert_eq!(
            describe(
                &Window {
                    used_percent: None,
                    window_minutes: None,
                    resets_at_unix: None
                },
                0
            ),
            "n/a"
        );
    }

    #[test]
    fn tmux_status_colours() {
        let snap = Snapshot {
            session: Some(win(28, None)),
            weekly: Some(win(85, None)),
            scoped: vec![],
            observed_at_unix: 0,
        };
        assert_eq!(
            tmux_status(&snap),
            "#[fg=green]5h 28%#[default] #[fg=red]7d 85%#[default]"
        );
    }
}
