# Claude Usage Gauges Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `usage` glance panel and a standalone `usage` binary that show how much of your Claude Max limits you have burned (session 5-hour window, weekly window, per-model weekly windows), fetched live from the same endpoint the Claude Code `/usage` view uses.

**Architecture:** A pure data module (`src/usage.rs`) reads the local OAuth token from `~/.claude/.credentials.json` and parses the JSON from `GET https://api.anthropic.com/api/oauth/usage`, with the network call isolated in one `fetch` function. A panel (`src/panels/usage.rs`) wraps that in the established background-thread + mpsc pattern (copied from `weather.rs`), and a standalone binary (`src/bin/usage.rs`) reuses the panel full-screen (copied from `bin/music.rs`).

**Tech Stack:** Rust 2021, ratatui 0.29, crossterm 0.28, serde/serde_json (already deps), dirs (already a dep), `curl` subprocess (no HTTP crate, same as `weather`).

## Global Constraints

- No new third-party crate. Use `curl` for HTTP, `serde_json` for parsing, `dirs` for the home path. All are already present in `Cargo.toml`.
- Commit messages are plain, imperative, no trailers (no `Co-Authored-By`, no `Claude-Session`). Match the repo style: a `type(scope): subject` line plus an optional body.
- The OAuth access token is read-only. Never log it, never print it, never write it back. Never modify `~/.claude/.credentials.json`.
- No em dashes in any user-facing copy (panel text, help text, comments).
- Suite theme colors only: `theme::sage()` / `theme::amber()` / `theme::magenta()` for the gauge bars; `theme::dim()` / `theme::pane_header()` / `theme::pane_header_focused()` / `theme::historical()` for chrome.
- Window display order is fixed: session, weekly, opus, sonnet, overage. Only windows the API returns are rendered.
- Run all tests with `cargo test` from `/home/jane/projects/glance`.

---

### Task 1: Spike. Capture the live `/api/oauth/usage` response and lock headers + fixture

This is a discovery task (no TDD). Its deliverables are: (a) the exact `curl` header set that returns HTTP 200, recorded as a comment in `src/usage.rs`, and (b) a representative JSON fixture captured from the real response, pasted into the test module created in Task 3. The data model in later tasks is written to be tolerant of shape differences, so this task only has to pin the header set and confirm field names.

**Files:**
- Scratch only (a temp file under `$CLAUDE_JOB_DIR/tmp`). No source committed in this task.

- [ ] **Step 1: Extract the token into a shell variable without echoing it**

Run (this reads the local token; do NOT print `$TOK`):
```bash
TOK=$(python3 -c "import json;print(json.load(open('/home/jane/.claude/.credentials.json'))['claudeAiOauth']['accessToken'])")
test -n "$TOK" && echo "token loaded (${#TOK} chars)"
```
Expected: `token loaded (NNN chars)` with a non-zero length. If empty, the user is not logged in; stop and report.

- [ ] **Step 2: Hit the endpoint, trying header variants until one returns JSON**

Run (try the bearer alone first, then with the oauth beta header):
```bash
OUT="$CLAUDE_JOB_DIR/tmp/usage.json"
echo "--- attempt A: bearer only ---"
curl -s -o "$OUT" -w "%{http_code}\n" --max-time 10 -H "Authorization: Bearer $TOK" https://api.anthropic.com/api/oauth/usage
echo "--- attempt B: bearer + oauth beta ---"
curl -s -o "$OUT" -w "%{http_code}\n" --max-time 10 \
  -H "Authorization: Bearer $TOK" \
  -H "anthropic-beta: oauth-2025-04-20" \
  https://api.anthropic.com/api/oauth/usage
```
Expected: one attempt prints `200`. Record which header set produced 200. If both fail (e.g. 401/403), inspect the body (`cat "$OUT"`), and try adding `-H "anthropic-version: 2023-06-01"` and/or `-H "User-Agent: claude-cli/2.1.183"`. Keep the minimal header set that yields 200.

- [ ] **Step 3: Inspect the JSON shape**

Run:
```bash
python3 -m json.tool "$CLAUDE_JOB_DIR/tmp/usage.json" | head -60
```
Expected: an object with per-window entries. Confirm the top-level key names (expected: `five_hour`, `seven_day`, optionally `seven_day_opus`, `seven_day_sonnet`, `overage` or `extra_usage`), and for one window confirm the field carrying the percent (expected `utilization`) and the reset time (expected `resets_at`), and whether `resets_at` is an ISO-8601 string or an epoch number.

- [ ] **Step 4: Record findings for the next tasks**

Write down (you will paste these in later tasks):
1. The exact `-H` header list that returned 200 (used verbatim in Task 5's `fetch`).
2. The real JSON body, trimmed to a representative example with all window kinds, lightly sanitized if any field looks identifying (the numeric utilizations and reset times are not secret). This becomes `SAMPLE_USAGE` in Task 3.
3. Whether `resets_at` is a string or number (Task 3's `normalize_reset` already handles both, so this is just a confirmation).

No commit for this task.

---

### Task 2: Credential reading (`src/usage.rs` foundation)

**Files:**
- Create: `src/usage.rs`
- Modify: `src/lib.rs` (add `pub mod usage;`)
- Test: inline `#[cfg(test)]` module in `src/usage.rs`

**Interfaces:**
- Produces: `Creds { access_token: String, expires_at_ms: i64, subscription: String, tier: String }`; `enum CredsError { Missing, Expired, Malformed(String) }`; `parse_credentials(text: &str, now_ms: i64) -> Result<Creds, CredsError>`; `read_credentials(now_ms: i64) -> Result<Creds, CredsError>`; `now_ms() -> i64`.

- [ ] **Step 1: Register the module**

In `src/lib.rs`, add `pub mod usage;` to the module list (alongside the other `pub mod` lines, e.g. right after `pub mod tasks;`).

- [ ] **Step 2: Write the failing tests**

Create `src/usage.rs` with only the test module to start:
```rust
//! Claude usage data: reads the local OAuth token and fetches live limit
//! gauges from the same endpoint the Claude Code `/usage` view uses.
//! The network call is isolated in `fetch`; everything else is pure.

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
        assert!(matches!(parse_credentials("{not json", 0), Err(CredsError::Malformed(_))));
    }

    #[test]
    fn parse_credentials_missing_oauth_block() {
        assert!(matches!(parse_credentials("{}", 0), Err(CredsError::Malformed(_))));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib usage::tests 2>&1 | tail -20`
Expected: compile error (the functions and types do not exist yet).

- [ ] **Step 4: Write the implementation**

Prepend to `src/usage.rs` (above the test module):
```rust
use serde::Deserialize;

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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib usage::tests 2>&1 | tail -20`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add src/usage.rs src/lib.rs
git commit -m "feat(usage): read and validate the local Claude OAuth credentials"
```

---

### Task 3: Usage response parsing (`UsageSnapshot` / `Window` / `WindowKind`)

**Files:**
- Modify: `src/usage.rs`
- Test: inline `#[cfg(test)]` module in `src/usage.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `enum WindowKind { FiveHour, SevenDay, SevenDayOpus, SevenDaySonnet, Overage }` with `label(self) -> &'static str`; `struct Window { kind: WindowKind, utilization: f64, resets_at: Option<i64> }`; `struct UsageSnapshot { windows: Vec<Window> }`; `parse_usage(bytes: &[u8]) -> Result<UsageSnapshot, String>`. `resets_at` is normalized to epoch SECONDS.

- [ ] **Step 1: Write the failing tests**

Add these tests inside the existing `#[cfg(test)] mod tests` in `src/usage.rs`. Replace `SAMPLE_USAGE` with the real body captured in Task 1 if its shape differs (keep at least the five_hour/seven_day/opus/sonnet/overage windows so the assertions hold):
```rust
    // Captured from GET /api/oauth/usage (Task 1). Lightly trimmed.
    const SAMPLE_USAGE: &str = r#"{
        "five_hour":      {"utilization": 41, "resets_at": "2026-06-20T18:00:00Z"},
        "seven_day":      {"utilization": 33, "resets_at": "2026-06-24T06:00:00Z"},
        "seven_day_opus": {"utilization": 18, "resets_at": "2026-06-24T06:00:00Z"},
        "seven_day_sonnet": {"utilization": 7, "resets_at": "2026-06-24T06:00:00Z"},
        "overage":        {"utilization": 0, "resets_at": "2026-07-01T00:00:00Z"}
    }"#;

    #[test]
    fn parse_usage_reads_all_windows_in_order() {
        let snap = parse_usage(SAMPLE_USAGE.as_bytes()).unwrap();
        let kinds: Vec<WindowKind> = snap.windows.iter().map(|w| w.kind).collect();
        assert_eq!(
            kinds,
            vec![
                WindowKind::FiveHour,
                WindowKind::SevenDay,
                WindowKind::SevenDayOpus,
                WindowKind::SevenDaySonnet,
                WindowKind::Overage,
            ]
        );
        assert_eq!(snap.windows[0].utilization, 41.0);
        // 2026-06-20T18:00:00Z == 1781978400 epoch seconds.
        assert_eq!(snap.windows[0].resets_at, Some(1781978400));
    }

    #[test]
    fn parse_usage_omits_absent_windows() {
        let json = r#"{"five_hour":{"utilization":10,"resets_at":"2026-06-20T18:00:00Z"},"seven_day":{"utilization":20,"resets_at":"2026-06-24T06:00:00Z"}}"#;
        let snap = parse_usage(json.as_bytes()).unwrap();
        assert_eq!(snap.windows.len(), 2);
    }

    #[test]
    fn parse_usage_skips_unknown_window_kind() {
        let json = r#"{"five_hour":{"utilization":10,"resets_at":"2026-06-20T18:00:00Z"},"some_future_window":{"utilization":99}}"#;
        let snap = parse_usage(json.as_bytes()).unwrap();
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].kind, WindowKind::FiveHour);
    }

    #[test]
    fn parse_usage_accepts_epoch_number_resets() {
        let json = r#"{"five_hour":{"utilization":5,"resets_at":1782021600}}"#;
        let snap = parse_usage(json.as_bytes()).unwrap();
        assert_eq!(snap.windows[0].resets_at, Some(1782021600));
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
        assert_eq!(parse_rfc3339_utc("nope"), None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib usage::tests 2>&1 | tail -20`
Expected: compile error (`parse_usage`, `WindowKind`, etc. do not exist).

- [ ] **Step 3: Write the implementation**

Add to `src/usage.rs` (above the test module, below the Task 2 code):
```rust
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
/// omits are dropped; unknown window kinds are ignored (forward-compatible).
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib usage::tests 2>&1 | tail -20`
Expected: all usage tests pass (the 4 from Task 2 plus the 7 new ones).

- [ ] **Step 5: Commit**

```bash
git add src/usage.rs
git commit -m "feat(usage): parse the usage report into ordered window gauges"
```

---

### Task 4: Render helpers (bar, color, reset countdown, header label)

**Files:**
- Modify: `src/usage.rs`
- Test: inline `#[cfg(test)]` module in `src/usage.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `bar_string(util: f64, width: usize) -> String`; `util_color(util: f64) -> ratatui::style::Color`; `fmt_reset(remaining_secs: i64) -> String`; `header_label(subscription: &str, tier: &str) -> String`.

- [ ] **Step 1: Write the failing tests**

Add inside `#[cfg(test)] mod tests` in `src/usage.rs`:
```rust
    #[test]
    fn bar_string_fills_proportionally() {
        assert_eq!(bar_string(0.0, 20), "░".repeat(20));
        assert_eq!(bar_string(100.0, 20), "█".repeat(20));
        assert_eq!(bar_string(50.0, 20), format!("{}{}", "█".repeat(10), "░".repeat(10)));
        assert_eq!(bar_string(150.0, 10), "█".repeat(10)); // clamped
        assert_eq!(bar_string(50.0, 0), "");
    }

    #[test]
    fn util_color_thresholds() {
        use ratatui::style::Color;
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib usage::tests 2>&1 | tail -20`
Expected: compile error (the helper functions do not exist).

- [ ] **Step 3: Write the implementation**

Add to `src/usage.rs` (above the test module). Note the new `use` line for `Color` goes at the top of the file with the other imports:
```rust
// add near the top of the file, with the other `use` lines:
use crate::theme;
use ratatui::style::Color;
```
```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib usage::tests 2>&1 | tail -20`
Expected: all usage tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage.rs
git commit -m "feat(usage): add gauge bar, color, countdown, and header helpers"
```

---

### Task 5: Network fetch (the untested edge)

**Files:**
- Modify: `src/usage.rs`
- Test: one `#[ignore]` live smoke test in the existing test module.

**Interfaces:**
- Consumes: `parse_usage`, `read_credentials`, `now_ms` (earlier in this file).
- Produces: `fetch(token: &str) -> Result<UsageSnapshot, String>`.

- [ ] **Step 1: Write the implementation**

Add to `src/usage.rs` (above the test module). Use the EXACT header set recorded in Task 1; the headers below are the expected set and must be reconciled with Task 1's finding:
```rust
// add with the other `use` lines at the top of the file:
use std::process::Command;
```
```rust
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

/// Fetch the live usage report. Network is isolated here (curl subprocess,
/// same approach as the weather panel). `--fail` makes curl exit non-zero on
/// any HTTP 4xx/5xx (expired token, rate limit), surfacing as an Err.
pub fn fetch(token: &str) -> Result<UsageSnapshot, String> {
    let auth = format!("Authorization: Bearer {token}");
    let out = Command::new("curl")
        .args([
            "-sf",
            "--max-time",
            "10",
            "-H",
            &auth,
            // Header set confirmed in Task 1. Adjust to match if Task 1 found
            // a different working set.
            "-H",
            "anthropic-beta: oauth-2025-04-20",
            USAGE_URL,
        ])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl exited {}", out.status));
    }
    parse_usage(&out.stdout)
}
```

- [ ] **Step 2: Add an ignored live smoke test**

Add inside `#[cfg(test)] mod tests` in `src/usage.rs`:
```rust
    #[test]
    #[ignore = "hits the network; run with --ignored while logged into Claude Code"]
    fn live_fetch_smoke() {
        let creds = read_credentials(now_ms()).expect("logged in");
        let snap = fetch(&creds.access_token).expect("fetch ok");
        assert!(!snap.windows.is_empty());
    }
```

- [ ] **Step 3: Verify the crate builds and the ignored test compiles**

Run: `cargo test --lib usage 2>&1 | tail -20`
Expected: pure tests pass; `live_fetch_smoke` is listed as ignored.

- [ ] **Step 4: Run the live smoke test once (real network)**

Run: `cargo test --lib usage::tests::live_fetch_smoke -- --ignored --nocapture 2>&1 | tail -20`
Expected: PASS (1 passed). If it fails with an HTTP error, reconcile the `fetch` headers with Task 1's confirmed set and re-run. This is the real end-to-end confirmation of the endpoint.

- [ ] **Step 5: Commit**

```bash
git add src/usage.rs
git commit -m "feat(usage): fetch the live usage report over curl"
```

---

### Task 6: `UsagePanel` and panel registration

**Files:**
- Create: `src/panels/usage.rs`
- Modify: `src/panels/mod.rs` (add `pub mod usage;`, a `build_panel` arm, and `"usage"` in `DEFAULT_ORDER` and `ALL_PANELS`)

**Interfaces:**
- Consumes: `crate::usage::{UsageSnapshot, Window, WindowKind, fetch, read_credentials, now_ms, header_label, bar_string, util_color, fmt_reset, CredsError}`.
- Produces: `UsagePanel` implementing `crate::panels::Panel`, constructed by `UsagePanel::new()`.

- [ ] **Step 1: Create the panel**

Create `src/panels/usage.rs`:
```rust
//! Claude usage panel: live limit gauges (session / weekly / per-model) from
//! the local OAuth token. Background curl fetch on a 60s cadence, mirroring
//! the weather panel. Read-only: never logs or writes the token.
use crate::panels::Panel;
use crate::theme;
use crate::usage::{
    bar_string, fetch, fmt_reset, header_label, now_ms, read_credentials, util_color, CredsError,
    UsageSnapshot, WindowKind,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

enum Msg {
    Ok { header: String, snapshot: UsageSnapshot },
    NoCreds,
    Err { header: Option<String>, reason: String },
}

enum Status {
    Loading,
    Ok,
    Stale(String),
    NoCreds,
}

pub struct UsagePanel {
    snapshot: Option<UsageSnapshot>,
    status: Status,
    header: String,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<Msg>,
    tx: mpsc::Sender<Msg>,
    inflight: Arc<Mutex<bool>>,
}

impl UsagePanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            snapshot: None,
            status: Status::Loading,
            header: "claude".to_string(),
            last_kick: None,
            rx,
            tx,
            inflight: Arc::new(Mutex::new(false)),
        }
    }

    fn kick(&mut self) {
        let mut g = match self.inflight.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if *g {
            return;
        }
        *g = true;
        drop(g);
        let tx = self.tx.clone();
        let inflight = Arc::clone(&self.inflight);
        thread::spawn(move || {
            let msg = match read_credentials(now_ms()) {
                Ok(c) => {
                    let header = header_label(&c.subscription, &c.tier);
                    match fetch(&c.access_token) {
                        Ok(snapshot) => Msg::Ok { header, snapshot },
                        Err(reason) => Msg::Err {
                            header: Some(header),
                            reason,
                        },
                    }
                }
                Err(CredsError::Missing) => Msg::NoCreds,
                Err(CredsError::Expired) => Msg::Err {
                    header: None,
                    reason: "token expired, open claude to refresh".to_string(),
                },
                Err(CredsError::Malformed(e)) => Msg::Err {
                    header: None,
                    reason: format!("creds malformed: {e}"),
                },
            };
            let _ = tx.send(msg);
            if let Ok(mut g) = inflight.lock() {
                *g = false;
            }
        });
        self.last_kick = Some(Instant::now());
    }
}

impl Panel for UsagePanel {
    fn name(&self) -> &str {
        "usage"
    }

    fn refresh_ms(&self) -> u64 {
        // Drain the inbox often; the network fetch itself is gated to 60s below.
        500
    }

    fn tick(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Ok { header, snapshot } => {
                    self.header = header;
                    self.snapshot = Some(snapshot);
                    self.status = Status::Ok;
                }
                Msg::NoCreds => self.status = Status::NoCreds,
                Msg::Err { header, reason } => {
                    if let Some(h) = header {
                        self.header = h;
                    }
                    self.status = Status::Stale(reason);
                }
            }
        }
        let due = match self.last_kick {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_secs(60),
        };
        if due {
            self.kick();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(1), // gap
                Constraint::Min(0),    // gauges / message
            ])
            .split(area);

        let title = Line::from(vec![
            Span::styled(" usage ", theme::pane_header()),
            Span::styled(self.header.clone(), theme::pane_header_focused()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        let snap = match &self.snapshot {
            Some(s) => s,
            None => {
                let msg = match &self.status {
                    Status::Loading => "loading…".to_string(),
                    Status::NoCreds => "no claude credentials found".to_string(),
                    Status::Stale(r) => r.clone(),
                    Status::Ok => "no usage data".to_string(),
                };
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(msg, theme::dim()))),
                    chunks[2],
                );
                return;
            }
        };

        let stale = matches!(self.status, Status::Stale(_));
        let bar_w = (area.width as usize).saturating_sub(34).clamp(6, 40);
        let now_s = now_ms() / 1000;

        let mut lines: Vec<Line> = Vec::new();
        if let Status::Stale(r) = &self.status {
            lines.push(Line::from(Span::styled(format!("stale · {r}"), theme::dim())));
        }
        for w in &snap.windows {
            let label = match w.kind {
                WindowKind::SevenDayOpus | WindowKind::SevenDaySonnet => {
                    format!("  {}", w.kind.label())
                }
                _ => w.kind.label().to_string(),
            };
            let bar = bar_string(w.utilization, bar_w);
            let bar_style = if stale {
                theme::dim()
            } else {
                Style::default().fg(util_color(w.utilization))
            };
            let pct_style = if stale { theme::dim() } else { theme::historical() };
            let resets = w
                .resets_at
                .map(|r| format!("   resets {}", fmt_reset(r - now_s)))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(format!("{label:<8}"), theme::dim()),
                Span::styled(bar, bar_style),
                Span::styled(format!(" {:>3.0}%", w.utilization), pct_style),
                Span::styled(resets, theme::dim()),
            ]));
        }
        f.render_widget(Paragraph::new(lines), chunks[2]);
    }
}
```

- [ ] **Step 2: Register the panel**

In `src/panels/mod.rs`:
1. Add `pub mod usage;` to the `pub mod` list at the top.
2. Add this arm to `build_panel` (next to the other arms):
```rust
        "usage" => Box::new(usage::UsagePanel::new()),
```
3. Add `"usage"` to the end of the `DEFAULT_ORDER` array (after `"standup"`).
4. Add `"usage"` to the end of the `ALL_PANELS` array (after `"standup"`).

- [ ] **Step 3: Build and confirm the panel is wired**

Run: `cargo build 2>&1 | tail -20`
Expected: builds cleanly (warnings tolerated, no errors).

- [ ] **Step 4: Smoke-test the panel live in glance via tmux**

Panel selection is via `~/.config/glance/panels.toml` (key `panels = [...]`), and `dirs` honors `XDG_CONFIG_HOME`, so pin a single-panel config in a throwaway dir without touching the real config:
```bash
cargo build --release 2>&1 | tail -3
CFG="$CLAUDE_JOB_DIR/tmp/xdg"
mkdir -p "$CFG/glance"
printf 'panels = ["usage"]\n' > "$CFG/glance/panels.toml"
tmux kill-session -t usagetest 2>/dev/null
tmux new-session -d -s usagetest -x 120 -y 40 "env XDG_CONFIG_HOME=$CFG ./target/release/glance"
sleep 4
tmux capture-pane -t usagetest -p | sed -n '1,20p'
tmux kill-session -t usagetest 2>/dev/null
```
Expected: the pane shows the `usage` title with `max · 20x`, then gauge rows (session/weekly/...) with bars and percentages, OR a `loading…` line on the very first capture (fetch in flight).

- [ ] **Step 5: Run the full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass (no regressions).

- [ ] **Step 6: Commit**

```bash
git add src/panels/usage.rs src/panels/mod.rs
git commit -m "feat(usage): add the usage gauge panel to glance"
```

---

### Task 7: Standalone `usage` binary

**Files:**
- Create: `src/bin/usage.rs` (cargo auto-discovers `src/bin/*.rs`; no `Cargo.toml` change)

**Interfaces:**
- Consumes: `glance::panels::usage::UsagePanel`, `glance::panels::Panel`, `glance::{brightness, theme}`.

- [ ] **Step 1: Create the binary**

Create `src/bin/usage.rs` (mirrors `src/bin/music.rs`):
```rust
//! Standalone `usage` cockpit: the glance usage panel running full-screen as
//! its own binary. Shows live Claude limit gauges (session / weekly / per
//! model). q quits.
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use glance::panels::usage::UsagePanel;
use glance::panels::Panel;
use glance::{brightness, theme};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SEP: &str = "  ·  ";
const HELP: &str = "\
usage :: live Claude limit gauges (standalone)

USAGE:
  usage              Launch the gauges (interactive TTY required).
  usage --help       Print this message.
  usage --version    Print version.

Reads the local Claude OAuth token (~/.claude/.credentials.json, read-only)
and shows the same limit data as the Claude Code /usage view: session (5h),
weekly, and per-model windows.

KEYS: [ ] brightness · q quit.
";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("usage {VERSION}");
        return Ok(());
    }
    if let Some(other) = args.first() {
        eprintln!("usage: unknown arg: {other}\n\nTry: usage --help");
        std::process::exit(2);
    }

    let mut panel = UsagePanel::new();

    suite_term::panic::install_panic_hook();
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle("usage"),
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let res = run(&mut terminal, &mut panel);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;
    res
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    panel: &mut UsagePanel,
) -> Result<()> {
    let mut last = Instant::now();
    panel.tick(); // prime so the first frame kicks a fetch
    loop {
        if last.elapsed() >= Duration::from_millis(500) {
            panel.tick();
            last = Instant::now();
        }

        terminal.draw(|f| {
            let chunks =
                Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(f.area());
            panel.render(f, chunks[0]);
            f.render_widget(Paragraph::new(footer_line()), chunks[1]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(());
                }
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('[') => {
                        brightness::dim();
                    }
                    KeyCode::Char(']') => {
                        brightness::brighten();
                    }
                    _ => {
                        panel.handle_key(key);
                    }
                }
            }
        }
    }
}

fn footer_line() -> Line<'static> {
    Line::from(vec![
        Span::styled(" [ ]", theme::pane_header_focused()),
        Span::raw(" bright"),
        Span::styled(SEP, theme::dim()),
        Span::styled("q", theme::pane_header_focused()),
        Span::raw(" quit"),
    ])
}
```

- [ ] **Step 2: Build the binary**

Run: `cargo build --bin usage 2>&1 | tail -20`
Expected: builds cleanly.

- [ ] **Step 3: Verify --help and --version**

Run:
```bash
cargo run --quiet --bin usage -- --version
cargo run --quiet --bin usage -- --help | head -5
```
Expected: `usage <version>` printed; help text begins with `usage :: live Claude limit gauges`.

- [ ] **Step 4: Smoke-test the standalone live in a tmux pty**

Run:
```bash
cargo build --release --bin usage 2>&1 | tail -3
tmux kill-session -t usagebin 2>/dev/null
tmux new-session -d -s usagebin -x 120 -y 40 "./target/release/usage"
sleep 4
tmux capture-pane -t usagebin -p | sed -n '1,20p'
tmux send-keys -t usagebin q
sleep 1
tmux kill-session -t usagebin 2>/dev/null
```
Expected: the pane shows the usage title and gauge rows (or `loading…` on a very early capture), and the footer `[ ] bright · q quit`. `q` exits cleanly.

- [ ] **Step 5: Commit**

```bash
git add src/bin/usage.rs
git commit -m "feat(usage): add standalone usage binary"
```

---

## Final verification (after all tasks)

- [ ] Run the full suite: `cargo test 2>&1 | tail -20` (all pass).
- [ ] Run the live endpoint test once: `cargo test --lib usage::tests::live_fetch_smoke -- --ignored --nocapture` (passes).
- [ ] Build both binaries: `cargo build --release 2>&1 | tail -3`.
- [ ] Optional install for daily use: `install -m 0755 target/release/usage ~/.local/bin/usage`.
- [ ] Dispatch a final code review over the whole branch, then use superpowers:finishing-a-development-branch.

## Notes for the executor

- The one genuine unknown is the exact `fetch` header set and the `resets_at` encoding. Task 1 pins both before any parsing code is written; Task 3's `normalize_reset` already tolerates string-or-number resets, and Task 5's headers must be reconciled with Task 1's finding.
- Multi-account view, historical token/cost accounting, and OAuth refresh are explicitly out of scope (see the spec's "Out of scope" section). The single-source `read_credentials` is the seam where a future multi-account version would return a list.
