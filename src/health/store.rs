//! Append-only event store: ~/.local/share/glance/health.jsonl
use crate::health::config::Activity;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub ts: i64,
    pub date: String, // YYYY-MM-DD (local)
    pub activity: String,
    pub count: f64,
}

pub struct HealthStore {
    pub path: PathBuf,
    pub events: Vec<Event>,
    mtime: Option<SystemTime>,
}

pub fn data_path() -> PathBuf {
    dirs::data_local_dir()
        .map(|d| d.join("glance").join("health.jsonl"))
        .unwrap_or_else(|| PathBuf::from("/tmp/glance-health.jsonl"))
}

impl HealthStore {
    pub fn load(path: PathBuf) -> Self {
        let events = parse_file(&path);
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        Self { path, events, mtime }
    }

    /// Re-read the file if its mtime changed (keeps panel + standalone in sync).
    pub fn reload_if_changed(&mut self) {
        let m = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        if m != self.mtime {
            self.events = parse_file(&self.path);
            self.mtime = m;
        }
    }

    pub fn append(&mut self, activity: &str, count: f64, today: &str) {
        let ev = Event {
            ts: now_unix(),
            date: today.to_string(),
            activity: activity.to_string(),
            count,
        };
        append_line(&self.path, &ev);
        self.events.push(ev);
        self.mtime = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
    }
}

/// Append a single event directly to the default data file (used by migration).
pub fn log_event(activity: &str, count: f64, today: &str) {
    let ev = Event {
        ts: now_unix(),
        date: today.to_string(),
        activity: activity.to_string(),
        count,
    };
    append_line(&data_path(), &ev);
}

fn parse_file(path: &Path) -> Vec<Event> {
    let Ok(s) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    s.lines()
        .filter_map(|l| serde_json::from_str::<Event>(l).ok())
        .collect()
}

fn append_line(path: &Path, ev: &Event) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(line) = serde_json::to_string(ev) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---- aggregation (pure over &[Event]) ----

pub fn today_total(events: &[Event], activity: &str, today: &str) -> f64 {
    events
        .iter()
        .filter(|e| e.activity == activity && e.date == today)
        .map(|e| e.count)
        .sum::<f64>()
        .max(0.0)
}

/// Per-day summed values for `activity` over the given (oldest..newest) dates.
pub fn series(events: &[Event], activity: &str, dates: &[String]) -> Vec<f64> {
    let mut map: BTreeMap<&str, f64> = BTreeMap::new();
    for e in events.iter().filter(|e| e.activity == activity) {
        *map.entry(e.date.as_str()).or_insert(0.0) += e.count;
    }
    dates
        .iter()
        .map(|d| map.get(d.as_str()).copied().unwrap_or(0.0).max(0.0))
        .collect()
}

pub struct AllTime {
    pub total: f64,
    pub best_day: f64,
    pub active_days: usize,
    pub avg: f64,
}

pub fn all_time(events: &[Event], activity: &str) -> AllTime {
    let mut per_day: BTreeMap<&str, f64> = BTreeMap::new();
    for e in events.iter().filter(|e| e.activity == activity) {
        *per_day.entry(e.date.as_str()).or_insert(0.0) += e.count;
    }
    let total: f64 = per_day.values().sum();
    let best_day = per_day.values().cloned().fold(0.0_f64, f64::max);
    let active_days = per_day.values().filter(|v| **v > 0.0).count();
    let avg = if active_days > 0 {
        total / active_days as f64
    } else {
        0.0
    };
    AllTime { total, best_day, active_days, avg }
}

/// Consecutive days (ending today, or yesterday if today is incomplete) where
/// EVERY activity reached its goal.
pub fn streak(events: &[Event], activities: &[Activity], today: &str) -> u32 {
    if activities.is_empty() {
        return 0;
    }
    let day_complete = |d: &str| {
        activities
            .iter()
            .all(|a| today_total(events, &a.name, d) >= a.goal)
    };
    let mut cur = today.to_string();
    if !day_complete(&cur) {
        cur = prev_date(&cur);
    }
    let mut n = 0u32;
    while day_complete(&cur) {
        n += 1;
        cur = prev_date(&cur);
        if n > 3650 {
            break;
        }
    }
    n
}

// ---- date helpers (jiff) ----

pub fn today_iso() -> String {
    let d = jiff::Zoned::now().date();
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}

pub fn prev_date(date: &str) -> String {
    offset_date(date, -1)
}

pub fn offset_date(date: &str, days: i64) -> String {
    if let Ok(d) = date.parse::<jiff::civil::Date>() {
        if let Ok(nd) = d.checked_add(jiff::Span::new().days(days)) {
            return format!("{:04}-{:02}-{:02}", nd.year(), nd.month(), nd.day());
        }
    }
    date.to_string()
}

/// The last `n` date strings, oldest..today (inclusive of today).
pub fn last_n_dates(today: &str, n: usize) -> Vec<String> {
    (0..n).rev().map(|k| offset_date(today, -(k as i64))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::config::Activity;

    fn ev(date: &str, activity: &str, count: f64) -> Event {
        Event { ts: 0, date: date.into(), activity: activity.into(), count }
    }
    fn act(name: &str, goal: f64) -> Activity {
        Activity { name: name.into(), goal, unit: "x".into(), weekly_target: None }
    }

    #[test]
    fn today_total_sums_and_floors_at_zero() {
        let e = vec![
            ev("2026-05-23", "pushups", 6.0),
            ev("2026-05-23", "pushups", 4.0),
            ev("2026-05-22", "pushups", 99.0),
        ];
        assert_eq!(today_total(&e, "pushups", "2026-05-23"), 10.0);
        let e2 = vec![ev("2026-05-23", "water", 1.0), ev("2026-05-23", "water", -3.0)];
        assert_eq!(today_total(&e2, "water", "2026-05-23"), 0.0);
    }

    #[test]
    fn series_maps_dates_in_order() {
        let e = vec![ev("2026-05-21", "bike", 30.0), ev("2026-05-23", "bike", 15.0)];
        let dates = vec!["2026-05-21".to_string(), "2026-05-22".to_string(), "2026-05-23".to_string()];
        assert_eq!(series(&e, "bike", &dates), vec![30.0, 0.0, 15.0]);
    }

    #[test]
    fn all_time_totals() {
        let e = vec![
            ev("2026-05-20", "squats", 10.0),
            ev("2026-05-21", "squats", 20.0),
            ev("2026-05-21", "squats", 5.0),
        ];
        let at = all_time(&e, "squats");
        assert_eq!(at.total, 35.0);
        assert_eq!(at.best_day, 25.0);
        assert_eq!(at.active_days, 2);
        assert!((at.avg - 17.5).abs() < 1e-9);
    }

    #[test]
    fn streak_counts_all_met_days_and_breaks_on_gap() {
        let acts = vec![act("a", 10.0), act("b", 5.0)];
        let e = vec![
            ev("2026-05-23", "a", 10.0), ev("2026-05-23", "b", 5.0),
            ev("2026-05-22", "a", 12.0), ev("2026-05-22", "b", 5.0),
            ev("2026-05-21", "a", 1.0), ev("2026-05-21", "b", 5.0),
            ev("2026-05-20", "a", 10.0), ev("2026-05-20", "b", 5.0),
        ];
        assert_eq!(streak(&e, &acts, "2026-05-23"), 2);
    }

    #[test]
    fn streak_today_incomplete_counts_through_yesterday() {
        let acts = vec![act("a", 10.0)];
        let e = vec![
            ev("2026-05-23", "a", 1.0),
            ev("2026-05-22", "a", 10.0),
            ev("2026-05-21", "a", 10.0),
        ];
        assert_eq!(streak(&e, &acts, "2026-05-23"), 2);
    }

    #[test]
    fn date_helpers() {
        assert_eq!(prev_date("2026-05-01"), "2026-04-30");
        assert_eq!(offset_date("2026-05-23", -2), "2026-05-21");
        let last3 = last_n_dates("2026-05-23", 3);
        assert_eq!(last3, vec!["2026-05-21", "2026-05-22", "2026-05-23"]);
    }

    #[test]
    fn append_then_reload_roundtrips() {
        let dir = std::env::temp_dir().join(format!("glance-health-test-{}", now_unix()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("health.jsonl");
        let mut s = HealthStore::load(path.clone());
        assert!(s.events.is_empty());
        s.append("pushups", 10.0, "2026-05-23");
        let s2 = HealthStore::load(path.clone());
        assert_eq!(s2.events.len(), 1);
        assert_eq!(s2.events[0].activity, "pushups");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
