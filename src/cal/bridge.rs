//! Bridge to ~/Projects/skai-work/scripts/zele/cal_json.py + on-disk cache.

use crate::cal::event::Event;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub enum BridgeError {
    ShimMissing(PathBuf),
    ShimFailed(String),
    JsonParse(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShimMissing(p) => write!(f, "shim missing at {}", p.display()),
            Self::ShimFailed(s) => write!(f, "bridge error: {}", s),
            Self::JsonParse(s) => write!(f, "bridge JSON error: {}", s),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FetchResult {
    pub events: Vec<Event>,
    pub fetched_at: SystemTime,
    pub stale_cache: bool,
}

const CACHE_TTL: Duration = Duration::from_secs(300);

pub fn shim_path() -> PathBuf {
    if let Ok(p) = std::env::var("GLANCE_CAL_SHIM") {
        return PathBuf::from(p);
    }
    dirs::home_dir().unwrap_or_default()
        .join("Projects/skai-work/scripts/zele/cal_json.py")
}

pub fn cache_path() -> PathBuf {
    if let Ok(p) = std::env::var("GLANCE_CAL_CACHE") {
        return PathBuf::from(p);
    }
    dirs::cache_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".cache"))
        .join("glance")
        .join("cal.json")
}

pub fn account() -> String {
    std::env::var("GLANCE_CAL_ACCOUNT").unwrap_or_else(|_| "jane@repcap.com".into())
}

pub fn fetch_sync() -> Result<FetchResult, BridgeError> {
    let shim = shim_path();
    if !shim.exists() {
        return Err(BridgeError::ShimMissing(shim));
    }
    let out = Command::new("python3")
        .arg(&shim)
        .arg("--week")
        .arg("--account").arg(account())
        .output()
        .map_err(|e| BridgeError::ShimFailed(e.to_string()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let first = stderr.lines().next().unwrap_or("(no stderr)").to_string();
        return Err(BridgeError::ShimFailed(first));
    }
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| BridgeError::JsonParse(e.to_string()))?;
    let items = parsed.get("items").cloned().unwrap_or(serde_json::Value::Array(vec![]));
    let events: Vec<Event> = serde_json::from_value(items)
        .map_err(|e| BridgeError::JsonParse(e.to_string()))?;
    let _ = write_cache(&events);
    Ok(FetchResult { events, fetched_at: SystemTime::now(), stale_cache: false })
}

pub fn fetch_async() -> mpsc::Receiver<Result<FetchResult, BridgeError>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(fetch_sync());
    });
    rx
}

pub fn write_cache(events: &[Event]) -> Result<(), std::io::Error> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let envelope = serde_json::json!({
        "_fetched_at_ms": now_ms,
        "events": events,
    });
    std::fs::write(&path, serde_json::to_string(&envelope)?)
}

/// `(result, is_fresh)`. `is_fresh = true` when cache is younger than CACHE_TTL.
/// Returns None if no cache file or it cannot be parsed.
pub fn load_cache_any() -> Option<(FetchResult, bool)> {
    let path = cache_path();
    let s = std::fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    let fetched_ms = v.get("_fetched_at_ms")?.as_u64()?;
    let events: Vec<Event> = serde_json::from_value(v.get("events")?.clone()).ok()?;
    let fetched_at = UNIX_EPOCH + Duration::from_millis(fetched_ms);
    let is_fresh = SystemTime::now().duration_since(fetched_at)
        .map(|d| d < CACHE_TTL).unwrap_or(false);
    Some((FetchResult { events, fetched_at, stale_cache: !is_fresh }, is_fresh))
}

/// Convenience: returns Some(result) only if cache is fresh.
pub fn load_cache() -> Option<FetchResult> {
    let (r, fresh) = load_cache_any()?;
    if fresh { Some(r) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::sync::Mutex;
    // Serialize tests that mutate process-global env vars (GLANCE_CAL_SHIM /
    // GLANCE_CAL_CACHE) so parallel `cargo test` runs can't stomp each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn shim_path_respects_env_override() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("GLANCE_CAL_SHIM", "/tmp/foo.py");
        assert_eq!(shim_path(), PathBuf::from("/tmp/foo.py"));
        std::env::remove_var("GLANCE_CAL_SHIM");
    }

    #[test]
    fn shim_missing_yields_typed_error() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("GLANCE_CAL_SHIM", "/nonexistent/path.py");
        let err = fetch_sync().unwrap_err();
        assert!(matches!(err, BridgeError::ShimMissing(_)));
        std::env::remove_var("GLANCE_CAL_SHIM");
    }

    #[test]
    fn fetch_via_fake_python_shim() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = TempDir::new().unwrap();
        let pyshim = tmp.path().join("fakeshim.py");
        let json_line = r#"{"summary":"1 events","items":[{"id":"a","summary":"S","description":"","location":"","start":"2026-05-26T09:00:00-05:00","end":"2026-05-26T09:30:00-05:00","all_day":false,"status":"confirmed","html_link":"","hangout_link":"","meet_url":"","attendees":[],"is_recurring":false,"recurring_event_id":"","calendar_id":"primary"}]}"#;
        std::fs::write(&pyshim, format!("import json\nprint('{}')\n", json_line.replace("'", "\\'"))).unwrap();
        // simpler: write JSON via heredoc-style
        std::fs::write(&pyshim, format!("print({:?})", json_line)).unwrap();
        std::env::set_var("GLANCE_CAL_SHIM", &pyshim);
        let cache = tmp.path().join("cache.json");
        std::env::set_var("GLANCE_CAL_CACHE", &cache);
        let r = fetch_sync().unwrap();
        assert_eq!(r.events.len(), 1);
        assert_eq!(r.events[0].id, "a");
        std::env::remove_var("GLANCE_CAL_SHIM");
        std::env::remove_var("GLANCE_CAL_CACHE");
    }

    #[test]
    fn cache_roundtrip_via_env_override() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache.json");
        std::env::set_var("GLANCE_CAL_CACHE", &cache);
        let e = Event {
            id: "roundtrip".into(), summary: "T".into(), description: "".into(),
            location: "".into(),
            start: "2026-05-26T09:00:00-05:00".into(),
            end: "2026-05-26T09:30:00-05:00".into(),
            all_day: false, status: "confirmed".into(),
            html_link: "".into(), hangout_link: "".into(), meet_url: "".into(),
            attendees: vec![], is_recurring: false, recurring_event_id: "".into(),
            calendar_id: "primary".into(),
        };
        write_cache(&[e]).unwrap();
        let (loaded, fresh) = load_cache_any().expect("cache exists");
        assert!(fresh);
        assert_eq!(loaded.events.len(), 1);
        assert_eq!(loaded.events[0].id, "roundtrip");
        std::env::remove_var("GLANCE_CAL_CACHE");
    }
}
