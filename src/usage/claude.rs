//! Claude usage probe (OAuth usage endpoint).

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::time::Duration;

use super::{ScopedWindow, Snapshot, Window, now_unix};

const API_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA_HEADER: &str = "oauth-2025-04-20";
const TIMEOUT: Duration = Duration::from_secs(3);

// --- Credentials ---

#[derive(Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<OAuthEntry>,
    #[serde(rename = "claude.ai:oauth")]
    oauth_legacy: Option<OAuthEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthEntry {
    access_token: String,
    /// Milliseconds since epoch.
    expires_at: Option<f64>,
}

fn load_token() -> Result<String> {
    #[cfg(target_os = "macos")]
    if let Ok(raw) = load_from_keychain()
        && let Ok(token) = extract_token(&raw, now_unix())
    {
        return Ok(token);
    }

    let path = credentials_path()?;
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    extract_token(&raw, now_unix())
}

fn credentials_path() -> Result<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        let candidate = std::path::PathBuf::from(dir).join(".credentials.json");
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    let home = home::home_dir().ok_or_else(|| anyhow!("no home directory"))?;
    Ok(home.join(".claude/.credentials.json"))
}

#[cfg(target_os = "macos")]
fn load_from_keychain() -> Result<String> {
    let user = std::env::var("USER").context("no USER env var")?;
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-a",
            &user,
            "-w",
        ])
        .output()
        .context("running security command")?;
    if !output.status.success() {
        bail!("no keychain entry found");
    }
    String::from_utf8(output.stdout).context("keychain output not utf8")
}

fn extract_token(raw: &str, now: i64) -> Result<String> {
    let creds: CredentialsFile = serde_json::from_str(raw).context("parsing credentials")?;
    let entry = creds
        .oauth
        .or(creds.oauth_legacy)
        .ok_or_else(|| anyhow!("no claudeAiOauth entry in credentials"))?;

    let token = entry.access_token.trim().to_string();
    if token.is_empty() {
        bail!("empty access token");
    }
    if let Some(expires_ms) = entry.expires_at
        && now >= (expires_ms / 1000.0) as i64
    {
        bail!("access token expired (run `claude` to refresh it)");
    }
    Ok(token)
}

// --- API response ---

#[derive(Deserialize)]
struct UsageResponse {
    five_hour: Option<UsageWindow>,
    seven_day: Option<UsageWindow>,
    #[serde(default)]
    limits: Vec<LimitEntry>,
}

#[derive(Deserialize)]
struct UsageWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

/// One entry of the `limits` array. Per-model weekly counters arrive here
/// because the dedicated `seven_day_opus`/`seven_day_sonnet` fields can be null.
#[derive(Deserialize)]
struct LimitEntry {
    kind: Option<String>,
    percent: Option<f64>,
    resets_at: Option<String>,
    scope: Option<LimitScope>,
}

#[derive(Deserialize)]
struct LimitScope {
    model: Option<LimitModel>,
}

#[derive(Deserialize)]
struct LimitModel {
    display_name: Option<String>,
}

/// Fetch a fresh snapshot from the API.
pub fn probe() -> Result<Snapshot> {
    let token = load_token()?;
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(TIMEOUT))
            .build(),
    );
    let body = agent
        .get(API_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", BETA_HEADER)
        .call()
        .context("usage API request failed")?
        .body_mut()
        .read_to_string()
        .context("reading usage response")?;
    parse_response(&body, now_unix())
}

pub(crate) fn parse_response(body: &str, now: i64) -> Result<Snapshot> {
    let resp: UsageResponse = serde_json::from_str(body).context("parsing usage response")?;
    Ok(Snapshot {
        session: resp.five_hour.map(|w| to_window(w, 300)),
        weekly: resp.seven_day.map(|w| to_window(w, 10080)),
        scoped: to_scoped(resp.limits),
        observed_at_unix: now,
    })
}

/// Only `weekly_scoped` entries are taken: `session` and `weekly_all` restate
/// the `five_hour` and `seven_day` windows, so including them would double up.
fn to_scoped(limits: Vec<LimitEntry>) -> Vec<ScopedWindow> {
    limits
        .into_iter()
        .filter(|l| l.kind.as_deref() == Some("weekly_scoped"))
        .filter_map(|l| {
            let label = l.scope?.model?.display_name?;
            Some(ScopedWindow {
                window: Window {
                    used_percent: l.percent.map(clamp_percent),
                    window_minutes: Some(10080),
                    resets_at_unix: l.resets_at.as_deref().and_then(parse_iso8601),
                },
                label,
            })
        })
        .collect()
}

fn to_window(w: UsageWindow, window_minutes: u32) -> Window {
    Window {
        used_percent: w.utilization.map(clamp_percent),
        window_minutes: Some(window_minutes),
        resets_at_unix: w.resets_at.as_deref().and_then(parse_iso8601),
    }
}

fn clamp_percent(p: f64) -> u8 {
    p.round().clamp(0.0, 100.0) as u8
}

/// Parse `YYYY-MM-DDTHH:MM:SS[.fff](Z|+00:00)` into a unix timestamp (UTC).
fn parse_iso8601(s: &str) -> Option<i64> {
    let s = s.trim();
    let s = s
        .strip_suffix('Z')
        .or_else(|| s.strip_suffix("+00:00"))
        .unwrap_or(s);
    let (date_part, time_part) = s.split_once('T')?;
    let mut date = date_part.split('-');
    let year: i64 = date.next()?.parse().ok()?;
    let month: i64 = date.next()?.parse().ok()?;
    let day: i64 = date.next()?.parse().ok()?;

    let mut time = time_part.split('.').next()?.split(':');
    let hour: i64 = time.next()?.parse().ok()?;
    let min: i64 = time.next()?.parse().ok()?;
    let sec: i64 = time.next().and_then(|v| v.parse().ok()).unwrap_or(0);

    Some(days_from_civil(year, month, day) * 86400 + hour * 3600 + min * 60 + sec)
}

/// Howard Hinnant's days_from_civil algorithm (public domain).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_from_camel_case_credentials() {
        let json = r#"{"claudeAiOauth":{"accessToken":"tok","expiresAt":4102444800000}}"#;
        assert_eq!(extract_token(json, 1_000).unwrap(), "tok");
    }

    #[test]
    fn token_from_legacy_key() {
        let json = r#"{"claude.ai:oauth":{"accessToken":"old","expiresAt":4102444800000}}"#;
        assert_eq!(extract_token(json, 1_000).unwrap(), "old");
    }

    #[test]
    fn token_missing_entry_errors() {
        assert!(extract_token("{}", 1_000).is_err());
    }

    #[test]
    fn token_expired_errors() {
        let json = r#"{"claudeAiOauth":{"accessToken":"tok","expiresAt":5000}}"#;
        let err = extract_token(json, 10).unwrap_err().to_string();
        assert!(err.contains("expired"), "{err}");
    }

    #[test]
    fn full_response() {
        let json = r#"{
            "five_hour": {"utilization": 28.0, "resets_at": "2026-04-04T15:00:00Z"},
            "seven_day": {"utilization": 63.0, "resets_at": "2026-04-10T00:00:00Z"}
        }"#;
        let snap = parse_response(json, 42).unwrap();
        let s = snap.session.unwrap();
        assert_eq!(s.used_percent, Some(28));
        assert_eq!(s.window_minutes, Some(300));
        assert!(s.resets_at_unix.is_some());
        assert_eq!(snap.weekly.unwrap().used_percent, Some(63));
        assert_eq!(snap.observed_at_unix, 42);
    }

    #[test]
    fn partial_response() {
        let snap = parse_response(r#"{"five_hour": {"utilization": 50.0}}"#, 0).unwrap();
        assert!(snap.weekly.is_none());
        assert_eq!(snap.session.unwrap().resets_at_unix, None);
    }

    #[test]
    fn invalid_json_errors() {
        assert!(parse_response("not json", 0).is_err());
    }

    #[test]
    fn iso8601_basic_and_fractional() {
        assert_eq!(parse_iso8601("2026-01-01T00:00:00Z"), Some(1767225600));
        assert_eq!(
            parse_iso8601("2026-01-01T00:00:00.489883+00:00"),
            Some(1767225600)
        );
    }

    #[test]
    fn scoped_limits_only_weekly_scoped() {
        let json = r#"{
            "limits": [
                {"kind": "session", "percent": 26, "scope": null},
                {"kind": "weekly_all", "percent": 25, "scope": null},
                {"kind": "weekly_scoped", "percent": 26.6,
                 "resets_at": "2026-08-13T02:00:00+00:00",
                 "scope": {"model": {"display_name": "Fable"}}},
                {"kind": "weekly_scoped", "percent": 140.0,
                 "scope": {"model": {"display_name": "Opus"}}},
                {"kind": "weekly_scoped", "percent": 40, "scope": {"surface": "code"}}
            ]
        }"#;
        let snap = parse_response(json, 0).unwrap();
        assert_eq!(snap.scoped.len(), 2);
        assert_eq!(snap.scoped[0].label, "Fable");
        assert_eq!(snap.scoped[0].window.used_percent, Some(27));
        assert_eq!(snap.scoped[1].window.used_percent, Some(100));
    }
}
