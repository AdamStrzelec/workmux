//! Claude plan usage limits (5 hour and weekly windows).
//!
//! Probes the Anthropic OAuth usage endpoint, caches the result on disk so
//! frequent callers (status bar, dashboard) do not hammer the API, and
//! exposes a small model that the CLI and dashboard render.

pub mod alert;
pub mod cache;
pub mod claude;
pub mod format;
pub mod notify;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Window {
    pub used_percent: Option<u8>,
    pub window_minutes: Option<u32>,
    pub resets_at_unix: Option<i64>,
}

/// A window metering a scoped part of the plan (e.g. a per-model weekly cap).
/// `label` is the provider's own name for the scope and is rendered verbatim.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScopedWindow {
    pub label: String,
    pub window: Window,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    /// Rolling 5 hour session window.
    pub session: Option<Window>,
    /// Plan-wide weekly window.
    pub weekly: Option<Window>,
    #[serde(default)]
    pub scoped: Vec<ScopedWindow>,
    pub observed_at_unix: i64,
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
