//! Claude usage data: reads the local OAuth token and fetches live limit
//! gauges from the same endpoint the Claude Code `/usage` view uses.
//! The network call is isolated in `fetch`; everything else is pure.
use crate::theme;
use ratatui::style::Color;
use serde::Deserialize;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct Creds {
    pub access_token: String,
    pub expires_at_ms: i64,
    pub subscription: String,
    pub tier: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CredsError {
    Missing,
    Expired,
    Malformed(String),
}

#[derive(Deserialize)]
struct RawCredsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<RawOauth>,
}

#[derive(Deserialize)]
struct RawOauth {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<i64>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
    #[serde(rename = "rateLimitTier")]
    rate_limit_tier: Option<String>,
}

/// Parse the credentials file body. `now_ms` is injected for testability.
pub fn parse_credentials(text: &str, now_ms: i64) -> Result<Creds, CredsError> {
    let raw: RawCredsFile =
        serde_json::from_str(text).map_err(|e| CredsError::Malformed(e.to_string()))?;
    let o = raw
        .claude_ai_oauth
        .ok_or_else(|| CredsError::Malformed("no claudeAiOauth block".to_string()))?;
    let access_token = o
        .access_token
        .ok_or_else(|| CredsError::Malformed("no accessToken".to_string()))?;
    let expires_at_ms = o.expires_at.unwrap_or(0);
    if expires_at_ms <= now_ms {
        return Err(CredsError::Expired);
    }
    Ok(Creds {
        access_token,
        expires_at_ms,
        subscription: o.subscription_type.unwrap_or_default(),
        tier: o.rate_limit_tier.unwrap_or_default(),
    })
}

/// Path to the Claude Code credentials file.
fn creds_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join(".credentials.json")
}

/// Read and parse the local credentials. Missing/unreadable file maps to
/// `CredsError::Missing`.
pub fn read_credentials(now_ms: i64) -> Result<Creds, CredsError> {
    let text = std::fs::read_to_string(creds_path()).map_err(|_| CredsError::Missing)?;
    parse_credentials(&text, now_ms)
}

/// Current wall-clock time in epoch milliseconds (0 if the clock is before the epoch).
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    FiveHour,
    SevenDay,
    SevenDayOpus,
    SevenDaySonnet,
    Overage,
}

impl WindowKind {
    pub fn label(self) -> &'static str {
        match self {
            WindowKind::FiveHour => "session",
            WindowKind::SevenDay => "weekly",
            WindowKind::SevenDayOpus => "opus",
            WindowKind::SevenDaySonnet => "sonnet",
            WindowKind::Overage => "overage",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub kind: WindowKind,
    pub utilization: f64,
    pub resets_at: Option<i64>, // epoch seconds, UTC
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageSnapshot {
    pub windows: Vec<Window>,
}

#[derive(Deserialize)]
struct RawWindow {
    utilization: Option<f64>,
    resets_at: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawUsage {
    five_hour: Option<RawWindow>,
    seven_day: Option<RawWindow>,
    seven_day_opus: Option<RawWindow>,
    seven_day_sonnet: Option<RawWindow>,
    overage: Option<RawWindow>,
    extra_usage: Option<RawWindow>,
}

/// Normalize a `resets_at` value (epoch number, numeric string, or RFC3339
/// string) to epoch SECONDS. Millisecond epochs are divided down.
fn normalize_reset(v: Option<&serde_json::Value>) -> Option<i64> {
    let to_secs = |x: i64| if x > 1_000_000_000_000 { x / 1000 } else { x };
    match v {
        Some(serde_json::Value::Number(n)) => n.as_i64().map(to_secs),
        Some(serde_json::Value::String(s)) => s
            .parse::<i64>()
            .map(to_secs)
            .ok()
            .or_else(|| parse_rfc3339_utc(s)),
        _ => None,
    }
}

/// Days since 1970-01-01 for a civil (proleptic Gregorian) date.
/// Howard Hinnant's algorithm.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Parse a `YYYY-MM-DDTHH:MM:SS...` prefix as UTC, returning epoch seconds.
/// Any timezone suffix is ignored (the endpoint emits UTC).
fn parse_rfc3339_utc(s: &str) -> Option<i64> {
    if s.len() < 19 {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: i64 = s.get(5..7)?.parse().ok()?;
    let d: i64 = s.get(8..10)?.parse().ok()?;
    let h: i64 = s.get(11..13)?.parse().ok()?;
    let mi: i64 = s.get(14..16)?.parse().ok()?;
    let se: i64 = s.get(17..19)?.parse().ok()?;
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se)
}

/// Parse the usage response body into an ordered snapshot. Windows the API
/// omits or sends as null are dropped; unknown window kinds are ignored
/// (forward-compatible).
pub fn parse_usage(bytes: &[u8]) -> Result<UsageSnapshot, String> {
    let raw: RawUsage = serde_json::from_slice(bytes).map_err(|e| format!("json: {e}"))?;
    let mut windows = Vec::new();
    let mut push = |kind: WindowKind, rw: Option<&RawWindow>| {
        if let Some(w) = rw {
            if let Some(util) = w.utilization {
                windows.push(Window {
                    kind,
                    utilization: util,
                    resets_at: normalize_reset(w.resets_at.as_ref()),
                });
            }
        }
    };
    push(WindowKind::FiveHour, raw.five_hour.as_ref());
    push(WindowKind::SevenDay, raw.seven_day.as_ref());
    push(WindowKind::SevenDayOpus, raw.seven_day_opus.as_ref());
    push(WindowKind::SevenDaySonnet, raw.seven_day_sonnet.as_ref());
    push(
        WindowKind::Overage,
        raw.overage.as_ref().or(raw.extra_usage.as_ref()),
    );
    Ok(UsageSnapshot { windows })
}

/// A fixed-width gauge bar: filled blocks for the used fraction, light blocks
/// for the rest. Utilization is a percent (0..=100), clamped.
pub fn bar_string(util: f64, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let frac = (util / 100.0).clamp(0.0, 1.0);
    let filled = ((frac * width as f64).round() as usize).min(width);
    let mut s = String::with_capacity(width * 3);
    for _ in 0..filled {
        s.push('█');
    }
    for _ in 0..(width - filled) {
        s.push('░');
    }
    s
}

/// Bar color by utilization: sage below 50%, amber from 50% through 80%,
/// magenta above 80%.
pub fn util_color(util: f64) -> Color {
    if util > 80.0 {
        theme::magenta()
    } else if util >= 50.0 {
        theme::amber()
    } else {
        theme::sage()
    }
}

/// Coarse countdown text. Non-positive means the window has reset.
pub fn fmt_reset(remaining_secs: i64) -> String {
    if remaining_secs <= 0 {
        return "now".to_string();
    }
    let d = remaining_secs / 86400;
    let h = (remaining_secs % 86400) / 3600;
    let m = (remaining_secs % 3600) / 60;
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/// Title text like "max · 20x" from the subscription type and rate-limit tier.
/// The tier's last underscore-separated segment is the human multiplier.
pub fn header_label(subscription: &str, tier: &str) -> String {
    let short = tier.rsplit('_').next().unwrap_or("").trim();
    match (subscription.is_empty(), short.is_empty()) {
        (false, false) => format!("{subscription} · {short}"),
        (false, true) => subscription.to_string(),
        (true, false) => short.to_string(),
        (true, true) => "claude".to_string(),
    }
}

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

/// Fetch the live usage report. Network is isolated here (curl subprocess,
/// same approach as the weather panel). `--fail` makes curl exit non-zero on
/// any HTTP 4xx/5xx (expired token, rate limit), surfacing as an Err.
/// The bearer token alone returns 200; no extra headers.
pub fn fetch(token: &str) -> Result<UsageSnapshot, String> {
    let auth = format!("Authorization: Bearer {token}");
    let out = Command::new("curl")
        .args(["-sf", "--max-time", "10", "-H", &auth, USAGE_URL])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl exited {}", out.status));
    }
    parse_usage(&out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREDS_OK: &str = r#"{"claudeAiOauth":{"accessToken":"tok-abc","expiresAt":2000,"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#;

    #[test]
    fn parse_credentials_reads_fields() {
        let c = parse_credentials(CREDS_OK, 1000).unwrap();
        assert_eq!(c.access_token, "tok-abc");
        assert_eq!(c.expires_at_ms, 2000);
        assert_eq!(c.subscription, "max");
        assert_eq!(c.tier, "default_claude_max_20x");
    }

    #[test]
    fn parse_credentials_expired_when_past() {
        assert_eq!(parse_credentials(CREDS_OK, 3000), Err(CredsError::Expired));
    }

    #[test]
    fn parse_credentials_malformed_json() {
        assert!(matches!(
            parse_credentials("{not json", 0),
            Err(CredsError::Malformed(_))
        ));
    }

    #[test]
    fn parse_credentials_missing_oauth_block() {
        assert!(matches!(
            parse_credentials("{}", 0),
            Err(CredsError::Malformed(_))
        ));
    }

    // Captured from GET /api/oauth/usage (Task 1). Real shape: ISO resets with
    // fractional seconds + offset; windows may be null; extra_usage carries no
    // `utilization` when disabled; dollar/`limits`/`spend` fields are ignored.
    const SAMPLE_USAGE: &str = r#"{
        "five_hour":        {"utilization": 41, "resets_at": "2026-06-20T18:00:00.513591+00:00", "limit_dollars": null},
        "seven_day":        {"utilization": 33, "resets_at": "2026-06-24T06:00:00.513611+00:00", "limit_dollars": null},
        "seven_day_opus":   null,
        "seven_day_sonnet": {"utilization": 7, "resets_at": "2026-06-24T06:00:00.513618+00:00", "limit_dollars": null},
        "extra_usage":      {"is_enabled": false, "utilization": null},
        "limits": [],
        "spend": {}
    }"#;

    #[test]
    fn parse_usage_reads_present_windows_in_order() {
        let snap = parse_usage(SAMPLE_USAGE.as_bytes()).unwrap();
        let kinds: Vec<WindowKind> = snap.windows.iter().map(|w| w.kind).collect();
        assert_eq!(
            kinds,
            vec![
                WindowKind::FiveHour,
                WindowKind::SevenDay,
                WindowKind::SevenDaySonnet,
            ]
        );
        assert_eq!(snap.windows[0].utilization, 41.0);
        // 2026-06-20T18:00:00Z == 1781978400 (fractional seconds + offset ignored).
        assert_eq!(snap.windows[0].resets_at, Some(1781978400));
    }

    #[test]
    fn parse_usage_maps_opus_when_present() {
        let json = r#"{"seven_day_opus":{"utilization":18,"resets_at":"2026-06-24T06:00:00Z"}}"#;
        let snap = parse_usage(json.as_bytes()).unwrap();
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].kind, WindowKind::SevenDayOpus);
        assert_eq!(snap.windows[0].utilization, 18.0);
    }

    #[test]
    fn parse_usage_maps_overage_from_extra_usage() {
        let json = r#"{"extra_usage":{"utilization":12,"resets_at":1781978400}}"#;
        let snap = parse_usage(json.as_bytes()).unwrap();
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].kind, WindowKind::Overage);
    }

    #[test]
    fn parse_usage_skips_null_and_unknown_windows() {
        let json = r#"{"five_hour":{"utilization":10,"resets_at":"2026-06-20T18:00:00Z"},"seven_day_opus":null,"some_future_window":{"utilization":99}}"#;
        let snap = parse_usage(json.as_bytes()).unwrap();
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].kind, WindowKind::FiveHour);
    }

    #[test]
    fn parse_usage_accepts_epoch_number_resets() {
        let json = r#"{"five_hour":{"utilization":5,"resets_at":1781978400}}"#;
        let snap = parse_usage(json.as_bytes()).unwrap();
        assert_eq!(snap.windows[0].resets_at, Some(1781978400));
    }

    #[test]
    fn parse_usage_malformed_json_errors() {
        assert!(parse_usage(b"{not json").is_err());
    }

    #[test]
    fn window_labels() {
        assert_eq!(WindowKind::FiveHour.label(), "session");
        assert_eq!(WindowKind::SevenDay.label(), "weekly");
        assert_eq!(WindowKind::SevenDayOpus.label(), "opus");
        assert_eq!(WindowKind::SevenDaySonnet.label(), "sonnet");
        assert_eq!(WindowKind::Overage.label(), "overage");
    }

    #[test]
    fn rfc3339_to_epoch() {
        assert_eq!(parse_rfc3339_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_utc("2000-01-01T00:00:00Z"), Some(946684800));
        assert_eq!(
            parse_rfc3339_utc("2026-06-20T18:00:00.513591+00:00"),
            Some(1781978400)
        );
        assert_eq!(parse_rfc3339_utc("nope"), None);
    }

    #[test]
    fn bar_string_fills_proportionally() {
        assert_eq!(bar_string(0.0, 20), "░".repeat(20));
        assert_eq!(bar_string(100.0, 20), "█".repeat(20));
        assert_eq!(
            bar_string(50.0, 20),
            format!("{}{}", "█".repeat(10), "░".repeat(10))
        );
        assert_eq!(bar_string(150.0, 10), "█".repeat(10));
        assert_eq!(bar_string(50.0, 0), "");
    }

    #[test]
    fn util_color_thresholds() {
        let sage: Color = crate::theme::sage();
        let amber: Color = crate::theme::amber();
        let magenta: Color = crate::theme::magenta();
        assert_eq!(util_color(49.0), sage);
        assert_eq!(util_color(50.0), amber);
        assert_eq!(util_color(80.0), amber);
        assert_eq!(util_color(81.0), magenta);
    }

    #[test]
    fn fmt_reset_formats() {
        assert_eq!(fmt_reset(3 * 3600 + 12 * 60), "3h 12m");
        assert_eq!(fmt_reset(4 * 86400 + 6 * 3600), "4d 6h");
        assert_eq!(fmt_reset(12 * 60), "12m");
        assert_eq!(fmt_reset(0), "now");
        assert_eq!(fmt_reset(-5), "now");
    }

    #[test]
    fn header_label_formats() {
        assert_eq!(header_label("max", "default_claude_max_20x"), "max · 20x");
        assert_eq!(header_label("max", ""), "max");
        assert_eq!(header_label("", "default_claude_max_5x"), "5x");
        assert_eq!(header_label("", ""), "claude");
    }

    #[test]
    #[ignore = "hits the network; run with --ignored while logged into Claude Code"]
    fn live_fetch_smoke() {
        let creds = read_credentials(now_ms()).expect("logged in");
        let snap = fetch(&creds.access_token).expect("fetch ok");
        assert!(!snap.windows.is_empty());
    }
}
