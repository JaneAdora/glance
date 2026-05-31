//! DayGroup + day-bucketing + NOW marker placement + WidthClass.

use crate::cal::event::Event;
use jiff::civil::Date;
use jiff::Timestamp;

#[derive(Debug, Clone)]
pub struct DayGroup {
    pub date: Date,
    pub label: String,
    pub is_today: bool,
    pub events: Vec<Event>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WidthClass { Tiny, Narrow, Mid, Wide }

impl WidthClass {
    pub fn from(cols: u16) -> Self {
        match cols {
            0..=39 => Self::Tiny,
            40..=59 => Self::Narrow,
            60..=79 => Self::Mid,
            _ => Self::Wide,
        }
    }
    pub fn show_meet_glyph(&self) -> bool { matches!(self, Self::Mid | Self::Wide) }
    pub fn show_header_count(&self) -> bool { !matches!(self, Self::Tiny | Self::Narrow) }
    pub fn show_time_range(&self) -> bool { matches!(self, Self::Wide) }
}

/// Group events by their local-tz start date. Today first if present, then chronological.
/// Within a day: all-day events first, then by start timestamp ascending.
pub fn bucket_by_day(events: Vec<Event>, today: Date) -> Vec<DayGroup> {
    use std::collections::BTreeMap;
    let mut buckets: BTreeMap<Date, Vec<Event>> = BTreeMap::new();
    for e in events {
        let Some(z) = e.start_zoned() else { continue; };
        let d = z.date();
        buckets.entry(d).or_default().push(e);
    }
    buckets
        .into_iter()
        .map(|(date, mut events)| {
            events.sort_by(|a, b| {
                match (a.all_day, b.all_day) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.start_ts().cmp(&b.start_ts()),
                }
            });
            let is_today = date == today;
            let label = label_for(date, is_today);
            DayGroup { date, label, is_today, events }
        })
        .collect()
}

fn label_for(date: Date, is_today: bool) -> String {
    let dow = match date.weekday() {
        jiff::civil::Weekday::Monday => "Mon",
        jiff::civil::Weekday::Tuesday => "Tue",
        jiff::civil::Weekday::Wednesday => "Wed",
        jiff::civil::Weekday::Thursday => "Thu",
        jiff::civil::Weekday::Friday => "Fri",
        jiff::civil::Weekday::Saturday => "Sat",
        jiff::civil::Weekday::Sunday => "Sun",
    };
    let month = match date.month() {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
        _ => "?",
    };
    if is_today {
        format!("TODAY · {} {} {}", dow, month, date.day())
    } else {
        format!("{} {} {}", dow.to_uppercase(), month, date.day())
    }
}

/// Where the NOW marker goes in today's events. `Some(i)` means insert the
/// marker before `events[i]` (the first non-past event). Returns `None` if
/// all events are past or all are future (marker would be at top/bottom).
pub fn now_marker_index(events: &[Event], now: &Timestamp) -> Option<usize> {
    let first_upcoming = events.iter().position(|e| !e.is_past(now))?;
    if first_upcoming == 0 { return None; }
    Some(first_upcoming)
}

#[cfg(test)]
mod tests {
    use super::*;
    

    fn ev(id: &str, start: &str, end: &str, all_day: bool) -> Event {
        Event {
            id: id.into(), summary: id.into(), description: "".into(),
            location: "".into(), start: start.into(), end: end.into(),
            all_day, status: "confirmed".into(),
            html_link: "".into(), hangout_link: "".into(), meet_url: "".into(),
            attendees: vec![], is_recurring: false, recurring_event_id: "".into(),
            calendar_id: "primary".into(),
        }
    }

    #[test]
    fn buckets_split_by_date() {
        let events = vec![
            ev("a", "2026-05-26T09:00:00-05:00", "2026-05-26T10:00:00-05:00", false),
            ev("b", "2026-05-27T09:00:00-05:00", "2026-05-27T10:00:00-05:00", false),
        ];
        let today: Date = "2026-05-26".parse().unwrap();
        let groups = bucket_by_day(events, today);
        assert_eq!(groups.len(), 2);
        assert!(groups[0].is_today);
        assert_eq!(groups[0].events.len(), 1);
        assert!(groups[0].label.starts_with("TODAY"));
        assert!(groups[1].label.starts_with("WED"));
    }

    #[test]
    fn all_day_sorts_first() {
        let events = vec![
            ev("late", "2026-05-26T15:00:00-05:00", "2026-05-26T16:00:00-05:00", false),
            ev("allday", "2026-05-26", "2026-05-27", true),
            ev("early", "2026-05-26T09:00:00-05:00", "2026-05-26T10:00:00-05:00", false),
        ];
        let today: Date = "2026-05-26".parse().unwrap();
        let groups = bucket_by_day(events, today);
        let day = &groups[0];
        assert_eq!(day.events[0].id, "allday");
        assert_eq!(day.events[1].id, "early");
        assert_eq!(day.events[2].id, "late");
    }

    #[test]
    fn now_marker_only_between_past_and_future() {
        let events = vec![
            ev("a", "2026-05-26T09:00:00-05:00", "2026-05-26T10:00:00-05:00", false),
            ev("b", "2026-05-26T15:00:00-05:00", "2026-05-26T16:00:00-05:00", false),
        ];
        let noon: Timestamp = "2026-05-26T12:00:00-05:00".parse().unwrap();
        assert_eq!(now_marker_index(&events, &noon), Some(1));
        let dawn: Timestamp = "2026-05-26T06:00:00-05:00".parse().unwrap();
        assert_eq!(now_marker_index(&events, &dawn), None);
        let dusk: Timestamp = "2026-05-26T22:00:00-05:00".parse().unwrap();
        assert_eq!(now_marker_index(&events, &dusk), None);
    }

    #[test]
    fn width_class_breakpoints() {
        assert_eq!(WidthClass::from(30), WidthClass::Tiny);
        assert_eq!(WidthClass::from(50), WidthClass::Narrow);
        assert_eq!(WidthClass::from(70), WidthClass::Mid);
        assert_eq!(WidthClass::from(100), WidthClass::Wide);
        assert!(WidthClass::Mid.show_meet_glyph());
        assert!(!WidthClass::Narrow.show_meet_glyph());
        assert!(WidthClass::Wide.show_time_range());
        assert!(WidthClass::Mid.show_header_count());
    }
}
