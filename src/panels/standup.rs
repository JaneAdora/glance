//! `standup` -- today-scoreboard glance panel.
//!
//! Synthesizes today's git commits, Claude Code sessions, and calendar meetings
//! into a single compact tile. Spec: docs/superpowers/specs/2026-05-28-standup-design.md
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub struct StandupPanel {}

impl StandupPanel {
    pub fn new() -> Self { Self {} }
}

impl Panel for StandupPanel {
    fn name(&self) -> &str { "standup" }
    fn refresh_ms(&self) -> u64 { 60_000 }
    fn tick(&mut self) {}
    fn render(&self, f: &mut Frame, area: Rect) {
        let title = Line::from(Span::styled(" standup ", theme::pane_header()));
        let body = Line::from(Span::styled("loading...", theme::dim()));
        f.render_widget(Paragraph::new(vec![title, body]), area);
    }
}

use std::time::SystemTime;

/// Local-tz civil midnight for `date`, as a `SystemTime`.
fn civil_midnight_systemtime(date: jiff::civil::Date, tz: &jiff::tz::TimeZone) -> SystemTime {
    let secs = date
        .at(0, 0, 0, 0)
        .to_zoned(tz.clone())
        .expect("local midnight is always representable")
        .timestamp()
        .as_second();
    std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64)
}

/// Returns `(today_midnight, yesterday_midnight)` as `SystemTime`s in the
/// local timezone. Seconds-precision. Each boundary is derived from its civil
/// date so DST-transition days stay correct (a flat -86_400 s would drift an
/// hour twice a year).
fn day_boundaries(now: jiff::Zoned) -> (SystemTime, SystemTime) {
    let tz = now.time_zone().clone();
    let today_date = now.date();
    let yesterday_date = today_date
        .checked_sub(jiff::Span::new().days(1))
        .unwrap_or(today_date);
    (
        civil_midnight_systemtime(today_date, &tz),
        civil_midnight_systemtime(yesterday_date, &tz),
    )
}

#[derive(Default, Clone, Debug)]
pub struct CommitsSnapshot {
    pub total: u32,
    pub repos_touched: u32,
    /// Most recent committer time, unix seconds.
    pub last_at: Option<i64>,
}

/// Aggregates `(repo_label, %cI, %H, %s)` rows into a snapshot.
fn summarize_commits(lines: &[(String, String, String, String)]) -> CommitsSnapshot {
    let mut repos: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut last_at: Option<i64> = None;
    for (repo, ci, _sha, _subj) in lines {
        repos.insert(repo.as_str());
        if let Ok(ts) = ci.parse::<jiff::Timestamp>() {
            let secs = ts.as_second();
            last_at = Some(last_at.map_or(secs, |prev| prev.max(secs)));
        }
    }
    CommitsSnapshot {
        total: lines.len() as u32,
        repos_touched: repos.len() as u32,
        last_at,
    }
}

#[derive(Default, Clone, Debug)]
pub struct SessionsSnapshot {
    pub count: u32,
    /// Most recent mtime within the range, unix seconds.
    pub last_at: Option<i64>,
}

fn count_sessions(
    mtimes: &[SystemTime],
    start: SystemTime,
    end: SystemTime,
) -> SessionsSnapshot {
    let mut count: u32 = 0;
    let mut last_at: Option<i64> = None;
    for m in mtimes {
        if *m >= start && *m < end {
            count += 1;
            let secs = m
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            last_at = Some(last_at.map_or(secs, |prev| prev.max(secs)));
        }
    }
    SessionsSnapshot { count, last_at }
}

use crate::cal::event::Event;

#[derive(Clone, Debug)]
pub struct EventLite {
    pub start_secs: i64,
    pub title: String,
}

#[derive(Default, Clone, Debug)]
pub struct MeetingsSnapshot {
    pub done: u32,
    /// Not-yet-finished meetings: in-progress + strictly future. `done + upcoming`
    /// equals the total counted meetings for the day.
    pub upcoming: u32,
    pub next: Option<EventLite>,
}

/// Splits a day's events into done/upcoming relative to `now`, and selects
/// the next strictly-future event by start time. Skips all-day, declined, and
/// events whose timestamps fail to parse.
fn summarize_meetings(events: &[Event], now: jiff::Timestamp) -> MeetingsSnapshot {
    let mut done: u32 = 0;
    let mut upcoming: u32 = 0;
    let mut next_pair: Option<(i64, String)> = None;

    for ev in events {
        if ev.all_day || ev.is_declined() {
            continue;
        }
        let Some(start) = ev.start_ts() else { continue };
        if ev.is_past(&now) {
            done += 1;
            continue;
        }
        upcoming += 1; // counts a meeting in progress right now, too
        let start_secs = start.as_second();
        if start_secs > now.as_second() {
            // Only a strictly-future event is eligible as "next", so the tile
            // never prints a clock time that has already passed.
            let take = match &next_pair {
                None => true,
                Some((prev_secs, _)) => start_secs < *prev_secs,
            };
            if take {
                next_pair = Some((start_secs, ev.summary.clone()));
            }
        }
    }

    MeetingsSnapshot {
        done,
        upcoming,
        next: next_pair.map(|(start_secs, title)| EventLite { start_secs, title }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_boundaries_today_is_local_midnight() {
        let now: jiff::Zoned = "2026-05-28T14:32:00-05:00[America/Chicago]"
            .parse()
            .unwrap();
        let (today, yesterday) = day_boundaries(now);
        let expected_today = std::time::UNIX_EPOCH
            + std::time::Duration::from_secs(1779944400);
        assert_eq!(today, expected_today);
        assert_eq!(today.duration_since(yesterday).unwrap().as_secs(), 86_400);
    }

    #[test]
    fn day_boundaries_at_local_midnight_returns_that_midnight() {
        let now: jiff::Zoned = "2026-05-28T00:00:00-05:00[America/Chicago]"
            .parse()
            .unwrap();
        let (today, _) = day_boundaries(now);
        let expected_today = std::time::UNIX_EPOCH
            + std::time::Duration::from_secs(1779944400);
        assert_eq!(today, expected_today);
    }

    fn cm(repo: &str, ci: &str, sha: &str, subj: &str) -> (String, String, String, String) {
        (repo.into(), ci.into(), sha.into(), subj.into())
    }

    #[test]
    fn summarize_commits_empty_is_zero() {
        let s = summarize_commits(&[]);
        assert_eq!(s.total, 0);
        assert_eq!(s.repos_touched, 0);
        assert_eq!(s.last_at, None);
    }

    #[test]
    fn summarize_commits_counts_across_repos() {
        let rows = vec![
            cm("/p/a", "2026-05-28T09:00:00-05:00", "aaa", "first"),
            cm("/p/a", "2026-05-28T11:30:00-05:00", "bbb", "second"),
            cm("/p/b", "2026-05-28T14:14:00-05:00", "ccc", "third"),
        ];
        let s = summarize_commits(&rows);
        assert_eq!(s.total, 3);
        assert_eq!(s.repos_touched, 2);
        let expected_last: i64 = "2026-05-28T14:14:00-05:00"
            .parse::<jiff::Timestamp>().unwrap().as_second();
        assert_eq!(s.last_at, Some(expected_last));
    }

    #[test]
    fn summarize_commits_skips_malformed_time() {
        let rows = vec![
            cm("/p/a", "not-a-date", "aaa", "first"),
            cm("/p/a", "2026-05-28T11:30:00-05:00", "bbb", "second"),
        ];
        let s = summarize_commits(&rows);
        assert_eq!(s.total, 2);
        assert_eq!(s.repos_touched, 1);
        let expected_last: i64 = "2026-05-28T11:30:00-05:00"
            .parse::<jiff::Timestamp>().unwrap().as_second();
        assert_eq!(s.last_at, Some(expected_last));
    }

    fn t(secs: u64) -> SystemTime {
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }

    #[test]
    fn count_sessions_empty_is_zero() {
        let s = count_sessions(&[], t(0), t(1));
        assert_eq!(s.count, 0);
        assert_eq!(s.last_at, None);
    }

    #[test]
    fn count_sessions_filters_to_range() {
        let mtimes = vec![t(100), t(200), t(300), t(400)];
        let s = count_sessions(&mtimes, t(150), t(350));
        assert_eq!(s.count, 2);
        assert_eq!(s.last_at, Some(300));
    }

    #[test]
    fn count_sessions_end_is_exclusive() {
        let mtimes = vec![t(200)];
        let s = count_sessions(&mtimes, t(100), t(200));
        assert_eq!(s.count, 0);
    }

    #[test]
    fn count_sessions_start_is_inclusive() {
        let mtimes = vec![t(200)];
        let s = count_sessions(&mtimes, t(200), t(300));
        assert_eq!(s.count, 1);
        assert_eq!(s.last_at, Some(200));
    }

    use crate::cal::event::{Attendee, ResponseStatus};

    fn make_event(start: &str, end: &str, title: &str) -> Event {
        Event {
            id: title.into(),
            summary: title.into(),
            description: String::new(),
            location: String::new(),
            start: start.into(),
            end: end.into(),
            all_day: false,
            status: "confirmed".into(),
            html_link: String::new(),
            hangout_link: String::new(),
            meet_url: String::new(),
            attendees: vec![Attendee {
                email: "jane@repcap.com".into(),
                name: "Jane".into(),
                response_status: ResponseStatus::Accepted,
                is_self: true,
                organizer: false,
            }],
            is_recurring: false,
            recurring_event_id: String::new(),
            calendar_id: "primary".into(),
        }
    }

    #[test]
    fn summarize_meetings_empty_is_zero() {
        let now: jiff::Timestamp = "2026-05-28T14:00:00-05:00".parse().unwrap();
        let s = summarize_meetings(&[], now);
        assert_eq!(s.done, 0);
        assert_eq!(s.upcoming, 0);
        assert!(s.next.is_none());
    }

    #[test]
    fn summarize_meetings_splits_by_now() {
        let now: jiff::Timestamp = "2026-05-28T14:00:00-05:00".parse().unwrap();
        let events = vec![
            make_event("2026-05-28T09:00:00-05:00", "2026-05-28T09:30:00-05:00", "morning"),
            make_event("2026-05-28T13:00:00-05:00", "2026-05-28T13:30:00-05:00", "lunch"),
            make_event("2026-05-28T16:00:00-05:00", "2026-05-28T16:30:00-05:00", "thelma sync"),
            make_event("2026-05-28T17:00:00-05:00", "2026-05-28T17:30:00-05:00", "wrap"),
        ];
        let s = summarize_meetings(&events, now);
        assert_eq!(s.done, 2);
        assert_eq!(s.upcoming, 2);
        let next = s.next.expect("next event present");
        assert_eq!(next.title, "thelma sync");
        let expected_start: i64 = "2026-05-28T16:00:00-05:00"
            .parse::<jiff::Timestamp>().unwrap().as_second();
        assert_eq!(next.start_secs, expected_start);
    }

    #[test]
    fn summarize_meetings_skips_declined() {
        let now: jiff::Timestamp = "2026-05-28T14:00:00-05:00".parse().unwrap();
        let mut declined = make_event(
            "2026-05-28T16:00:00-05:00",
            "2026-05-28T16:30:00-05:00",
            "declined",
        );
        declined.attendees[0].response_status = ResponseStatus::Declined;
        let kept = make_event(
            "2026-05-28T17:00:00-05:00",
            "2026-05-28T17:30:00-05:00",
            "kept",
        );
        let s = summarize_meetings(&[declined, kept], now);
        assert_eq!(s.done, 0);
        assert_eq!(s.upcoming, 1);
        assert_eq!(s.next.unwrap().title, "kept");
    }

    #[test]
    fn summarize_meetings_skips_all_day() {
        let now: jiff::Timestamp = "2026-05-28T14:00:00-05:00".parse().unwrap();
        let mut all_day = make_event("2026-05-28", "2026-05-29", "OOO");
        all_day.all_day = true;
        let timed = make_event(
            "2026-05-28T16:00:00-05:00",
            "2026-05-28T16:30:00-05:00",
            "real",
        );
        let s = summarize_meetings(&[all_day, timed], now);
        assert_eq!(s.upcoming, 1);
        assert_eq!(s.next.unwrap().title, "real");
    }

    #[test]
    fn summarize_meetings_in_progress_counts_but_is_not_next() {
        let now: jiff::Timestamp = "2026-05-28T14:00:00-05:00".parse().unwrap();
        let in_progress = make_event(
            "2026-05-28T13:45:00-05:00",
            "2026-05-28T14:30:00-05:00",
            "in progress",
        );
        let later = make_event(
            "2026-05-28T16:00:00-05:00",
            "2026-05-28T16:30:00-05:00",
            "later",
        );
        let s = summarize_meetings(&[in_progress, later], now);
        assert_eq!(s.done, 0);
        assert_eq!(s.upcoming, 2); // both not-yet-finished
        assert_eq!(s.next.unwrap().title, "later"); // in-progress skipped as "next"
    }
}
