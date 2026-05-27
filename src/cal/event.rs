//! Event data model + helpers. Matches the JSON shape emitted by
//! `~/Projects/skai-work/scripts/zele/cal_json.py`.

use jiff::{Timestamp, Zoned};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Accepted,
    Declined,
    Tentative,
    #[serde(alias = "needsAction")]
    NeedsAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attendee {
    pub email: String,
    #[serde(default)]
    pub name: String,
    pub response_status: ResponseStatus,
    #[serde(default)]
    pub is_self: bool,
    #[serde(default)]
    pub organizer: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub location: String,
    /// RFC3339 ("2026-05-26T09:00:00-05:00") for timed events; "YYYY-MM-DD" for all-day.
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub html_link: String,
    #[serde(default)]
    pub hangout_link: String,
    #[serde(default)]
    pub meet_url: String,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    #[serde(default)]
    pub is_recurring: bool,
    #[serde(default)]
    pub recurring_event_id: String,
    #[serde(default)]
    pub calendar_id: String,
}

impl Event {
    /// RFC3339 with offset parses as a `Timestamp` (absolute instant).
    /// All-day "YYYY-MM-DD" parses as midnight local-tz.
    pub fn start_ts(&self) -> Option<Timestamp> {
        parse_ts(&self.start)
    }
    pub fn end_ts(&self) -> Option<Timestamp> {
        parse_ts(&self.end)
    }
    /// `Zoned` view in the local system tz (for HH:MM formatting, weekday, etc).
    pub fn start_zoned(&self) -> Option<Zoned> {
        Some(self.start_ts()?.to_zoned(jiff::tz::TimeZone::system()))
    }
    pub fn end_zoned(&self) -> Option<Zoned> {
        Some(self.end_ts()?.to_zoned(jiff::tz::TimeZone::system()))
    }
    pub fn is_past(&self, now: &Timestamp) -> bool {
        self.end_ts().map(|e| e < *now).unwrap_or(false)
    }
    pub fn is_declined(&self) -> bool {
        self.attendees.iter().any(|a| a.is_self && matches!(a.response_status, ResponseStatus::Declined))
    }
    /// Duration in whole minutes.
    pub fn duration_minutes(&self) -> Option<i64> {
        let s = self.start_ts()?;
        let e = self.end_ts()?;
        Some(e.as_second() - s.as_second()).map(|secs| secs / 60)
    }
    /// Minutes until start; None if past or unparseable.
    pub fn minutes_until(&self, now: &Timestamp) -> Option<i64> {
        let s = self.start_ts()?;
        if s < *now { return None; }
        Some((s.as_second() - now.as_second()) / 60)
    }
}

fn parse_ts(s: &str) -> Option<Timestamp> {
    if let Ok(t) = s.parse::<Timestamp>() {
        return Some(t);
    }
    if let Ok(d) = s.parse::<jiff::civil::Date>() {
        let dt = d.at(0, 0, 0, 0);
        let tz = jiff::tz::TimeZone::system();
        return dt.to_zoned(tz).ok().map(|z| z.timestamp());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_event() -> Event {
        Event {
            id: "abc123".into(),
            summary: "Daily Huddle".into(),
            description: "".into(),
            location: "".into(),
            start: "2026-05-26T09:00:00-05:00".into(),
            end: "2026-05-26T09:30:00-05:00".into(),
            all_day: false,
            status: "confirmed".into(),
            html_link: "".into(),
            hangout_link: "https://meet.google.com/abc".into(),
            meet_url: "https://meet.google.com/abc".into(),
            attendees: vec![Attendee {
                email: "jane@repcap.com".into(),
                name: "Jane".into(),
                response_status: ResponseStatus::Accepted,
                is_self: true,
                organizer: false,
            }],
            is_recurring: true,
            recurring_event_id: "abc".into(),
            calendar_id: "primary".into(),
        }
    }

    #[test]
    fn parses_shim_json_shape() {
        let raw = r#"{
            "id": "x", "summary": "Test", "description": "d",
            "start": "2026-05-26T09:00:00-05:00",
            "end": "2026-05-26T09:30:00-05:00",
            "all_day": false, "status": "confirmed",
            "html_link": "", "hangout_link": "", "meet_url": "",
            "attendees": [{"email":"a@b","name":"A","response_status":"needs_action","is_self":false,"organizer":true}],
            "is_recurring": false, "recurring_event_id": "", "calendar_id": "primary",
            "location": ""
        }"#;
        let e: Event = serde_json::from_str(raw).unwrap();
        assert_eq!(e.id, "x");
        assert_eq!(e.attendees[0].response_status, ResponseStatus::NeedsAction);
        assert!(e.attendees[0].organizer);
    }

    #[test]
    fn alias_needs_action_camel() {
        let raw = r#"{"email":"a","response_status":"needsAction"}"#;
        let a: Attendee = serde_json::from_str(raw).unwrap();
        assert_eq!(a.response_status, ResponseStatus::NeedsAction);
    }

    #[test]
    fn detects_declined_self() {
        let mut e = fixture_event();
        e.attendees[0].response_status = ResponseStatus::Declined;
        assert!(e.is_declined());
        // not declined if attendee is someone else
        e.attendees[0].is_self = false;
        assert!(!e.is_declined());
    }

    #[test]
    fn is_past_uses_end_time() {
        let e = fixture_event();
        let future: Timestamp = "2026-05-27T00:00:00-05:00".parse().unwrap();
        let past: Timestamp = "2026-05-26T08:00:00-05:00".parse().unwrap();
        assert!(e.is_past(&future));
        assert!(!e.is_past(&past));
    }

    #[test]
    fn duration_30min() {
        let e = fixture_event();
        assert_eq!(e.duration_minutes(), Some(30));
    }

    #[test]
    fn parses_all_day_date() {
        let t = parse_ts("2026-05-26").unwrap();
        let z = t.to_zoned(jiff::tz::TimeZone::system());
        assert_eq!(z.year(), 2026);
        assert_eq!(z.month(), 5);
        assert_eq!(z.day(), 26);
    }
}
