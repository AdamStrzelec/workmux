//! Alert rules: "lots of quota left but the window is about to reset".

use serde::{Deserialize, Serialize};

use super::format::until_reset;
use super::{Snapshot, Window};

/// Which usage window a rule watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowKind {
    Session,
    Weekly,
}

/// Fire when at least `min_remaining_percent` of the window is unused and the
/// window resets within `within_minutes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertRule {
    pub window: WindowKind,
    pub min_remaining_percent: u8,
    pub within_minutes: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub window: WindowKind,
    pub remaining_percent: u8,
    pub resets_in_secs: i64,
    /// Identifies the window instance so a rule fires once per reset cycle.
    pub resets_at_unix: i64,
}

impl Alert {
    pub fn message(&self) -> String {
        let label = match self.window {
            WindowKind::Session => "5h session",
            WindowKind::Weekly => "weekly",
        };
        format!(
            "Claude {label} limit: {}% unused, resets in {}",
            self.remaining_percent,
            super::format::format_duration(self.resets_in_secs)
        )
    }
}

fn window_of(snap: &Snapshot, kind: WindowKind) -> Option<&Window> {
    match kind {
        WindowKind::Session => snap.session.as_ref(),
        WindowKind::Weekly => snap.weekly.as_ref(),
    }
}

/// Evaluate one rule against a snapshot.
pub fn evaluate(rule: &AlertRule, snap: &Snapshot, now: i64) -> Option<Alert> {
    let w = window_of(snap, rule.window)?;
    let used = w.used_percent?;
    let resets_at = w.resets_at_unix?;
    let remaining = 100u8.saturating_sub(used);
    let left = until_reset(w, now)?;
    (remaining >= rule.min_remaining_percent && left <= i64::from(rule.within_minutes) * 60)
        .then_some(Alert {
            window: rule.window,
            remaining_percent: remaining,
            resets_in_secs: left,
            resets_at_unix: resets_at,
        })
}

/// Rules that fire now and were not already delivered for this reset cycle.
/// `delivered` holds `(window, resets_at_unix)` pairs already notified.
pub fn due(
    rules: &[AlertRule],
    snap: &Snapshot,
    now: i64,
    delivered: &[(WindowKind, i64)],
) -> Vec<Alert> {
    let mut out: Vec<Alert> = Vec::new();
    for rule in rules {
        if let Some(a) = evaluate(rule, snap, now)
            && !delivered.contains(&(a.window, a.resets_at_unix))
            && !out.iter().any(|o| o.window == a.window)
        {
            out.push(a);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(session_used: u8, resets_at: i64) -> Snapshot {
        Snapshot {
            session: Some(Window {
                used_percent: Some(session_used),
                window_minutes: Some(300),
                resets_at_unix: Some(resets_at),
            }),
            weekly: None,
            scoped: vec![],
            observed_at_unix: 0,
        }
    }

    fn rule() -> AlertRule {
        AlertRule {
            window: WindowKind::Session,
            min_remaining_percent: 40,
            within_minutes: 30,
        }
    }

    #[test]
    fn fires_when_quota_left_and_reset_near() {
        let a = evaluate(&rule(), &snap(30, 1000 + 20 * 60), 1000).unwrap();
        assert_eq!(a.remaining_percent, 70);
        assert_eq!(a.resets_in_secs, 1200);
    }

    #[test]
    fn silent_when_reset_far() {
        assert!(evaluate(&rule(), &snap(30, 1000 + 120 * 60), 1000).is_none());
    }

    #[test]
    fn silent_when_quota_nearly_used() {
        assert!(evaluate(&rule(), &snap(80, 1000 + 10 * 60), 1000).is_none());
    }

    #[test]
    fn silent_when_already_reset_or_unknown() {
        assert!(evaluate(&rule(), &snap(30, 500), 1000).is_none());
        let mut s = snap(30, 0);
        s.session.as_mut().unwrap().resets_at_unix = None;
        assert!(evaluate(&rule(), &s, 1000).is_none());
    }

    #[test]
    fn fires_once_per_cycle() {
        let s = snap(30, 1000 + 600);
        let first = due(&[rule()], &s, 1000, &[]);
        assert_eq!(first.len(), 1);
        let delivered = vec![(first[0].window, first[0].resets_at_unix)];
        assert!(due(&[rule()], &s, 1100, &delivered).is_empty());
        // Next cycle (different reset timestamp) fires again.
        let next = snap(30, 1000 + 5 * 3600 + 600);
        assert!(due(&[rule()], &next, 1000 + 5 * 3600, &delivered).len() == 1);
    }

    #[test]
    fn message_text() {
        let a = evaluate(&rule(), &snap(30, 1000 + 20 * 60), 1000).unwrap();
        assert_eq!(
            a.message(),
            "Claude 5h session limit: 70% unused, resets in 20m"
        );
    }
}
