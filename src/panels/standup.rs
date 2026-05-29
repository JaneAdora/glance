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
}
