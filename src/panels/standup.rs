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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
// project_roots + find_repos are reused from commits.rs (made pub(crate)).
use crate::panels::commits::{find_repos, project_roots};
use crate::cal::CalCore;

enum Msg {
    Commits { today: CommitsSnapshot, yesterday: CommitsSnapshot },
    Sessions { today: SessionsSnapshot, yesterday: SessionsSnapshot },
}

#[derive(Default, Clone)]
struct Snapshot {
    commits: CommitsSnapshot,
    sessions: SessionsSnapshot,
    meetings: MeetingsSnapshot,
}

pub struct StandupPanel {
    today: Snapshot,
    yesterday: Snapshot,
    last_git_scan: Option<Instant>,
    last_session_scan: Option<Instant>,
    rx: mpsc::Receiver<Msg>,
    tx: mpsc::Sender<Msg>,
    loading_git: bool,
    loading_sessions: bool,
    last_date_seen: Option<jiff::civil::Date>,
    cal: CalCore,
    meetings_unavailable: bool,
}

impl StandupPanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            today: Snapshot::default(),
            yesterday: Snapshot::default(),
            last_git_scan: None,
            last_session_scan: None,
            rx,
            tx,
            loading_git: false,
            loading_sessions: false,
            last_date_seen: None,
            cal: CalCore::new(),
            meetings_unavailable: false,
        }
    }

    fn kick_git_scan(&mut self) {
        let tx = self.tx.clone();
        let now = jiff::Zoned::now();
        let (today_start, yesterday_start) = day_boundaries(now);
        self.loading_git = true;
        self.last_git_scan = Some(Instant::now());
        thread::spawn(move || {
            let (today, yesterday) = scan_commits(today_start, yesterday_start);
            let _ = tx.send(Msg::Commits { today, yesterday });
        });
    }

    fn kick_session_scan(&mut self) {
        let tx = self.tx.clone();
        let now = jiff::Zoned::now();
        let (today_start, yesterday_start) = day_boundaries(now);
        self.loading_sessions = true;
        self.last_session_scan = Some(Instant::now());
        thread::spawn(move || {
            let (today, yesterday) = scan_sessions(today_start, yesterday_start);
            let _ = tx.send(Msg::Sessions { today, yesterday });
        });
    }

    fn refresh_meetings(&mut self, now_local: &jiff::Zoned) {
        self.cal.tick();
        let now_ts = now_local.timestamp();
        let today_date = now_local.date();
        let yesterday_date = today_date
            .checked_sub(jiff::Span::new().days(1))
            .unwrap_or(today_date);

        let today_events: Vec<_> = self.cal.days.iter()
            .find(|g| g.date == today_date)
            .map(|g| g.events.clone())
            .unwrap_or_default();
        let yesterday_events: Vec<_> = self.cal.days.iter()
            .find(|g| g.date == yesterday_date)
            .map(|g| g.events.clone())
            .unwrap_or_default();

        self.today.meetings = summarize_meetings(&today_events, now_ts);
        self.yesterday.meetings = summarize_meetings(&yesterday_events, now_ts);

        // "unavailable" only when a fetch errored AND we have no calendar data at
        // all. A successful fetch with no events today is "0 meetings", not an error.
        self.meetings_unavailable =
            self.cal.last_fetch_error.is_some() && self.cal.days.is_empty();
    }
}

fn fmt_hh_mm(secs: i64) -> String {
    let ts = match jiff::Timestamp::from_second(secs) {
        Ok(t) => t,
        Err(_) => return "--:--".to_string(),
    };
    let z = ts.to_zoned(jiff::tz::TimeZone::system());
    let hour12 = match z.hour() % 12 {
        0 => 12,
        n => n,
    };
    let suffix = if z.hour() >= 12 { "PM" } else { "AM" };
    format!("{}:{:02} {}", hour12, z.minute(), suffix)
}

fn fmt_today_header(z: &jiff::Zoned) -> String {
    let weekday = match z.date().weekday() {
        jiff::civil::Weekday::Monday => "Mon",
        jiff::civil::Weekday::Tuesday => "Tue",
        jiff::civil::Weekday::Wednesday => "Wed",
        jiff::civil::Weekday::Thursday => "Thu",
        jiff::civil::Weekday::Friday => "Fri",
        jiff::civil::Weekday::Saturday => "Sat",
        jiff::civil::Weekday::Sunday => "Sun",
    };
    let month = match z.month() {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
        _ => "-",
    };
    format!("TODAY · {} {} {}", weekday, month, z.day())
}

fn minutes_until_secs(start_secs: i64, now_secs: i64) -> i64 {
    (start_secs - now_secs) / 60
}

impl Panel for StandupPanel {
    fn name(&self) -> &str { "standup" }
    // Tick fast so the channel drains within ~2s of the scan threads finishing
    // and the next-meeting countdown stays fresh. The git/session scans themselves
    // stay gated to every 5 min via the last_*_scan staleness checks in tick().
    fn refresh_ms(&self) -> u64 { 2_000 }

    fn tick(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Commits { today, yesterday } => {
                    self.today.commits = today;
                    self.yesterday.commits = yesterday;
                    self.loading_git = false;
                }
                Msg::Sessions { today, yesterday } => {
                    self.today.sessions = today;
                    self.yesterday.sessions = yesterday;
                    self.loading_sessions = false;
                }
            }
        }

        let now_local = jiff::Zoned::now();
        let today_date = now_local.date();
        let rolled = match self.last_date_seen {
            Some(d) => d != today_date,
            None => false,
        };
        self.last_date_seen = Some(today_date);

        let stale_git = rolled || self.last_git_scan
            .map(|t| t.elapsed() > Duration::from_secs(300))
            .unwrap_or(true);
        if stale_git && !self.loading_git {
            self.kick_git_scan();
        }

        let stale_sessions = rolled || self.last_session_scan
            .map(|t| t.elapsed() > Duration::from_secs(300))
            .unwrap_or(true);
        if stale_sessions && !self.loading_sessions {
            self.kick_session_scan();
        }

        self.refresh_meetings(&now_local);
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let now_local = jiff::Zoned::now();
        let now_secs = now_local.timestamp().as_second();

        let header = Line::from(Span::styled(
            format!(" {} ", fmt_today_header(&now_local)),
            theme::pane_header(),
        ));
        let rule = Line::from(Span::styled(
            "─".repeat(area.width.saturating_sub(2) as usize),
            theme::dim(),
        ));

        let pad = "  ";
        let label_w = 12;

        let count_or_dots = |loading: bool, n: u32| -> String {
            if loading && n == 0 { "  …".to_string() } else { format!("{:>3}", n) }
        };

        // Commits row.
        let commits_count = count_or_dots(self.loading_git, self.today.commits.total);
        let commits_suffix = if self.today.commits.total == 0 && !self.loading_git {
            String::new()
        } else {
            let mut s = format!("across {} repos", self.today.commits.repos_touched);
            if let Some(last) = self.today.commits.last_at {
                s.push_str(&format!(" · last {}", fmt_hh_mm(last)));
            }
            s
        };
        let commits_line = Line::from(vec![
            Span::raw(pad),
            Span::styled(format!("{:label_w$}", "commits", label_w = label_w), theme::historical()),
            Span::styled(commits_count, theme::now()),
            Span::raw("  "),
            Span::styled(commits_suffix, theme::historical()),
        ]);

        // Sessions row.
        let sessions_count = count_or_dots(self.loading_sessions, self.today.sessions.count);
        let sessions_suffix = if self.today.sessions.count == 0 && !self.loading_sessions {
            String::new()
        } else {
            let mut s = "claude code".to_string();
            if let Some(last) = self.today.sessions.last_at {
                s.push_str(&format!(" · last {}", fmt_hh_mm(last)));
            }
            s
        };
        let sessions_line = Line::from(vec![
            Span::raw(pad),
            Span::styled(format!("{:label_w$}", "sessions", label_w = label_w), theme::historical()),
            Span::styled(sessions_count, theme::now()),
            Span::raw("  "),
            Span::styled(sessions_suffix, theme::historical()),
        ]);

        // Meetings row.
        let m = &self.today.meetings;
        let total_meetings = m.done + m.upcoming;
        let (meetings_count, next_summary, suffix_style) = if self.meetings_unavailable {
            ("  -".to_string(), "meetings unavailable".to_string(), theme::dim())
        } else {
            let count = format!("{:>3}", total_meetings);
            let urgent = m
                .next
                .as_ref()
                .is_some_and(|n| (0..=15).contains(&minutes_until_secs(n.start_secs, now_secs)));
            let summary = match &m.next {
                Some(n) => format!("{} done · {} left · next {}", m.done, m.upcoming, fmt_hh_mm(n.start_secs)),
                None if total_meetings > 0 => format!("{} done · 0 left", m.done),
                None => String::new(),
            };
            let style = if urgent { theme::alert() } else { theme::historical() };
            (count, summary, style)
        };
        let meetings_line = Line::from(vec![
            Span::raw(pad),
            Span::styled(format!("{:label_w$}", "meetings", label_w = label_w), theme::historical()),
            Span::styled(meetings_count, theme::now()),
            Span::raw("  "),
            Span::styled(next_summary, suffix_style),
        ]);
        let next_title_line = match (self.meetings_unavailable, &m.next) {
            (false, Some(n)) => Line::from(vec![
                Span::raw(" ".repeat(pad.len() + label_w + 5)),
                Span::styled(format!("\"{}\"", n.title), suffix_style),
            ]),
            _ => Line::from(""),
        };

        // Yesterday line.
        let y = &self.yesterday;
        let yesterday_line = Line::from(vec![
            Span::raw(pad),
            Span::styled(format!("{:label_w$}", "yesterday", label_w = label_w), theme::historical()),
            Span::styled(
                format!(
                    "{}c · {}s · {}m",
                    y.commits.total, y.sessions.count, y.meetings.done + y.meetings.upcoming,
                ),
                theme::dim(),
            ),
        ]);

        let lines = vec![
            header, rule,
            commits_line, sessions_line, meetings_line, next_title_line,
            Line::from(""),
            yesterday_line,
        ];

        f.render_widget(Paragraph::new(lines), area);
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

fn git_log_rows(
    repo: &Path,
    since: &str,
    until: &str,
) -> Vec<(String, String, String, String)> {
    let res = Command::new("git")
        .arg("-C").arg(repo)
        .args([
            "log",
            &format!("--since={since}"),
            &format!("--until={until}"),
            "--format=%cI|%H|%s",
            "--all",
            "--no-merges",
        ])
        .output();
    let stdout = match res {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    let repo_label = repo.to_string_lossy().to_string();
    let mut out = Vec::new();
    for line in String::from_utf8_lossy(&stdout).lines() {
        let mut parts = line.splitn(3, '|');
        let (Some(ci), Some(sha), Some(subj)) = (parts.next(), parts.next(), parts.next())
        else { continue };
        out.push((repo_label.clone(), ci.to_string(), sha.to_string(), subj.to_string()));
    }
    out
}

fn format_iso(t: SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64).unwrap_or(0);
    jiff::Timestamp::from_second(secs)
        .map(|ts| ts.to_string())
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn scan_commits(
    today_start: SystemTime,
    yesterday_start: SystemTime,
) -> (CommitsSnapshot, CommitsSnapshot) {
    let today_iso = format_iso(today_start);
    let yesterday_iso = format_iso(yesterday_start);
    let now_iso = format_iso(SystemTime::now());

    let mut today_rows = Vec::new();
    let mut yesterday_rows = Vec::new();
    for root in project_roots() {
        for repo in find_repos(&root) {
            today_rows.extend(git_log_rows(&repo, &today_iso, &now_iso));
            yesterday_rows.extend(git_log_rows(&repo, &yesterday_iso, &today_iso));
        }
    }
    (summarize_commits(&today_rows), summarize_commits(&yesterday_rows))
}

fn claude_projects_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

fn collect_session_mtimes() -> Vec<SystemTime> {
    let Some(root) = claude_projects_dir() else { return Vec::new() };
    let outer = match std::fs::read_dir(&root) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for project in outer.flatten() {
        let pp = project.path();
        if !pp.is_dir() { continue; }
        let inner = match std::fs::read_dir(&pp) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for f in inner.flatten() {
            let fp = f.path();
            if fp.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(meta) = f.metadata() {
                if let Ok(mtime) = meta.modified() {
                    out.push(mtime);
                }
            }
        }
    }
    out
}

fn scan_sessions(
    today_start: SystemTime,
    yesterday_start: SystemTime,
) -> (SessionsSnapshot, SessionsSnapshot) {
    let mtimes = collect_session_mtimes();
    let now = SystemTime::now();
    let today = count_sessions(&mtimes, today_start, now);
    let yesterday = count_sessions(&mtimes, yesterday_start, today_start);
    (today, yesterday)
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
