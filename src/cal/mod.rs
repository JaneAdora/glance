//! Cal — Google Calendar agenda tile (today + week).
//! See docs/superpowers/specs/2026-05-26-cal-design.md.

pub mod bridge;
pub mod desc;
pub mod event;
pub mod view;

use crate::cal::bridge::{BridgeError, FetchResult};
use crate::cal::event::{Event, ResponseStatus};
use crate::cal::view::DayGroup;
use jiff::Timestamp;
use std::collections::HashSet;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Default)]
pub struct Focus {
    pub day: usize,
    pub event: Option<usize>,
}

#[derive(Debug)]
pub enum CalAction {
    None,
    CopiedUrl,
    CopiedDetail,
    Toast(String),
    Quit,
}

pub struct CalCore {
    pub days: Vec<DayGroup>,
    pub focus: Focus,
    pub expanded: HashSet<jiff::civil::Date>,
    pub show_detail: bool,
    pub show_past: bool,
    pub last_fetched: Option<Instant>,
    pub last_toast: Option<(String, Instant)>,
    pub stale_cache: bool,
    pub last_fetch_error: Option<String>,
    rx: Option<mpsc::Receiver<Result<FetchResult, BridgeError>>>,
}

impl CalCore {
    pub fn new() -> Self {
        let mut core = Self {
            days: Vec::new(),
            focus: Focus::default(),
            expanded: HashSet::new(),
            show_detail: false,
            show_past: true,
            last_fetched: None,
            last_toast: None,
            stale_cache: false,
            last_fetch_error: None,
            rx: None,
        };
        // Seed from cache: any age. If fresh, no need to fetch immediately; if stale, fetch later in tick.
        if let Some((cached, fresh)) = bridge::load_cache_any() {
            core.apply_fetch(FetchResult { stale_cache: !fresh, ..cached });
            if fresh {
                // last_fetched set; tick will refresh after 5 min.
            } else {
                // start_fetch now (background)
                core.start_fetch();
            }
        } else {
            // Cold launch: render "loading..." until first fetch arrives.
            core.start_fetch();
        }
        core
    }

    fn start_fetch(&mut self) {
        if self.rx.is_some() { return; }
        self.rx = Some(bridge::fetch_async());
    }

    fn apply_fetch(&mut self, result: FetchResult) {
        let today = today_local();
        let prev_focus = self.current_focus_key();
        self.days = view::bucket_by_day(result.events, today);
        self.last_fetched = Some(Instant::now());
        self.stale_cache = result.stale_cache;
        if let Some(today_grp) = self.days.iter().find(|g| g.is_today) {
            self.expanded.insert(today_grp.date);
        }
        // Drop expanded entries that no longer exist.
        let live: HashSet<jiff::civil::Date> = self.days.iter().map(|g| g.date).collect();
        self.expanded.retain(|d| live.contains(d));
        // Restore focus by (date, event_id) if possible; otherwise default to first upcoming today.
        match prev_focus {
            Some((date, event_id)) => self.set_focus_by_key(date, event_id.as_deref()),
            None => self.focus_default_event(),
        }
    }

    fn focus_default_event(&mut self) {
        let now = Timestamp::now();
        let Some((di, day)) = self.days.iter().enumerate().find(|(_, g)| g.is_today) else {
            self.focus = Focus::default();
            return;
        };
        let ei = day.events.iter().position(|e| !e.is_past(&now))
            .or_else(|| if day.events.is_empty() { None } else { Some(0) });
        self.focus = Focus { day: di, event: ei };
    }

    fn current_focus_key(&self) -> Option<(jiff::civil::Date, Option<String>)> {
        let day = self.days.get(self.focus.day)?;
        let ev_id = self.focus.event
            .and_then(|i| day.events.get(i).map(|e| e.id.clone()));
        Some((day.date, ev_id))
    }

    fn set_focus_by_key(&mut self, date: jiff::civil::Date, event_id: Option<&str>) {
        let Some(di) = self.days.iter().position(|g| g.date == date) else {
            self.focus_default_event();
            return;
        };
        self.focus.day = di;
        self.focus.event = event_id.and_then(|id| {
            self.days[di].events.iter().position(|e| e.id == id)
        });
    }

    pub fn tick(&mut self) {
        // Poll receiver.
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Ok(result)) => {
                    self.apply_fetch(result);
                    self.last_fetch_error = None;
                    self.rx = None;
                }
                Ok(Err(e)) => {
                    self.last_fetch_error = Some(format!("{}", e));
                    self.toast(format!("{}", e));
                    self.rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.rx = None;
                }
            }
        }
        // 5-min refresh cadence.
        let stale = self.last_fetched.map(|t| t.elapsed() > Duration::from_secs(300)).unwrap_or(true);
        if stale && self.rx.is_none() {
            self.start_fetch();
        }
        // Toast expiry.
        if let Some((_, t)) = self.last_toast {
            if t.elapsed() > Duration::from_secs(3) {
                self.last_toast = None;
            }
        }
    }

    pub fn refresh(&mut self) {
        if self.rx.is_none() {
            self.start_fetch();
            self.toast("refreshing...".into());
        }
    }

    pub fn toggle_past(&mut self) {
        self.show_past = !self.show_past;
        self.toast(if self.show_past { "showing past events".into() } else { "hiding past events".into() });
    }

    pub fn toggle_expand(&mut self) {
        let Some(day) = self.days.get(self.focus.day) else { return; };
        let d = day.date;
        if !self.expanded.remove(&d) {
            self.expanded.insert(d);
        }
    }

    pub fn drill_in(&mut self) {
        if self.show_detail { return; }
        if self.focus.event.is_some() {
            self.show_detail = true;
            return;
        }
        let Some(day) = self.days.get(self.focus.day) else { return; };
        let d = day.date;
        if !self.expanded.contains(&d) {
            self.expanded.insert(d);
        }
        self.move_down();
    }

    pub fn drill_out(&mut self) {
        if self.show_detail {
            self.show_detail = false;
            return;
        }
        let Some(day) = self.days.get(self.focus.day) else { return; };
        let d = day.date;
        self.expanded.remove(&d);
        if self.focus.event.is_some() {
            self.focus.event = None;
        }
    }

    pub fn visible_rows(&self) -> Vec<(usize, Option<usize>)> {
        let mut rows = Vec::new();
        let now = Timestamp::now();
        for (di, day) in self.days.iter().enumerate() {
            rows.push((di, None));
            if self.expanded.contains(&day.date) {
                for (ei, ev) in day.events.iter().enumerate() {
                    if !self.show_past && ev.is_past(&now) { continue; }
                    rows.push((di, Some(ei)));
                }
            }
        }
        rows
    }

    pub fn move_down(&mut self) {
        let visible = self.visible_rows();
        if visible.is_empty() { return; }
        let cur = visible.iter().position(|(d, e)| *d == self.focus.day && *e == self.focus.event).unwrap_or(0);
        let next = (cur + 1).min(visible.len() - 1);
        self.focus.day = visible[next].0;
        self.focus.event = visible[next].1;
    }

    pub fn move_up(&mut self) {
        let visible = self.visible_rows();
        if visible.is_empty() { return; }
        let cur = visible.iter().position(|(d, e)| *d == self.focus.day && *e == self.focus.event).unwrap_or(0);
        let next = cur.saturating_sub(1);
        self.focus.day = visible[next].0;
        self.focus.event = visible[next].1;
    }

    pub fn focused_event(&self) -> Option<&Event> {
        self.days.get(self.focus.day)?.events.get(self.focus.event?)
    }

    pub fn copy_url(&mut self) -> CalAction {
        let Some(ev) = self.focused_event() else {
            self.toast("no event focused".into());
            return CalAction::None;
        };
        if ev.meet_url.is_empty() {
            let s = ev.summary.clone();
            self.toast(format!("{}: no meet url", s));
            return CalAction::None;
        }
        let url = ev.meet_url.clone();
        let summary = ev.summary.clone();
        crate::clip::copy(&url);
        self.toast(format!("copied url: {}", summary));
        CalAction::CopiedUrl
    }

    pub fn detail_text(&self) -> Option<String> {
        let day = self.days.get(self.focus.day)?;
        let ev = day.events.get(self.focus.event?)?;
        let mut out = String::new();
        out.push_str(&format!("Event: {}\n", ev.summary));
        out.push_str(&format!("Date: {}\n", day.label));
        out.push_str(&format!("Time: {} to {}\n", ev.start, ev.end));
        if !ev.location.is_empty() {
            out.push_str(&format!("Location: {}\n", ev.location));
        }
        if !ev.meet_url.is_empty() {
            out.push_str(&format!("Meet URL: {}\n", ev.meet_url));
        }
        if !ev.attendees.is_empty() {
            out.push_str("\nAttendees:\n");
            for a in &ev.attendees {
                let glyph = match a.response_status {
                    ResponseStatus::Accepted => "accepted",
                    ResponseStatus::Declined => "declined",
                    ResponseStatus::Tentative => "tentative",
                    ResponseStatus::NeedsAction => "pending",
                };
                let org = if a.organizer { " [organizer]" } else { "" };
                let me = if a.is_self { " (you)" } else { "" };
                let name = if a.name.is_empty() { a.email.clone() } else { a.name.clone() };
                out.push_str(&format!("  - {} <{}>{}{} [{}]\n", name, a.email, org, me, glyph));
            }
        }
        let plain = desc::strip_html(&ev.description);
        if !plain.is_empty() {
            out.push_str("\nDescription:\n");
            for line in plain.lines() {
                out.push_str(line);
                out.push('\n');
            }
        }
        let mut links = desc::extract_urls(&ev.description);
        if !ev.html_link.is_empty() && !links.contains(&ev.html_link) {
            links.push(ev.html_link.clone());
        }
        if !links.is_empty() {
            out.push_str("\nLinks:\n");
            for l in links {
                out.push_str(&format!("  - {}\n", l));
            }
        }
        Some(out)
    }

    pub fn copy_detail(&mut self) -> CalAction {
        let Some(text) = self.detail_text() else {
            self.toast("no event focused".into());
            return CalAction::None;
        };
        let len = text.len();
        crate::clip::copy(&text);
        self.toast(format!("copied detail ({} chars)", len));
        CalAction::CopiedDetail
    }

    pub fn toggle_detail(&mut self) {
        if !self.show_detail && self.focus.event.is_none() { return; }
        self.show_detail = !self.show_detail;
    }
    pub fn close_detail(&mut self) { self.show_detail = false; }

    pub fn current_toast(&self) -> Option<&str> {
        self.last_toast.as_ref().and_then(|(s, t)| {
            if t.elapsed() < Duration::from_secs(3) { Some(s.as_str()) } else { None }
        })
    }

    fn toast(&mut self, s: String) {
        self.last_toast = Some((s, Instant::now()));
    }

    pub fn render(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        use ratatui::style::{Modifier, Style};
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Block, Borders, Paragraph};

        let width = view::WidthClass::from(area.width);
        let now = Timestamp::now();

        let mut lines: Vec<Line> = Vec::new();
        if self.days.is_empty() {
            let msg = if self.last_fetched.is_none() { "loading..." } else { "no events this week" };
            lines.push(Line::from(Span::styled(msg, crate::theme::dim())));
        }
        for (di, day) in self.days.iter().enumerate() {
            let expanded = self.expanded.contains(&day.date);
            let chev = if expanded { "▾" } else { "▸" };
            let total = day.events.len();
            let upcoming = day.events.iter().filter(|e| !e.is_past(&now)).count();
            let header_text = if width.show_header_count() {
                if day.is_today && upcoming != total {
                    format!("{} {} · {} events · {} upcoming", chev, day.label, total, upcoming)
                } else {
                    format!("{} {} · {} event{}", chev, day.label, total, if total == 1 { "" } else { "s" })
                }
            } else {
                format!("{} {}", chev, day.label)
            };
            let header_style = if self.focus.day == di && self.focus.event.is_none() {
                crate::theme::pane_header_focused()
            } else {
                crate::theme::dim()
            };
            lines.push(Line::from(Span::styled(header_text, header_style)));
            if !expanded { continue; }
            let now_idx = if day.is_today { view::now_marker_index(&day.events, &now) } else { None };
            for (ei, ev) in day.events.iter().enumerate() {
                if !self.show_past && ev.is_past(&now) { continue; }
                if Some(ei) == now_idx {
                    let width_px = area.width as usize;
                    let bar_len = width_px.saturating_sub(12);
                    let bar: String = "─".repeat(bar_len.min(60));
                    lines.push(Line::from(Span::styled(
                        format!("  ─── NOW {} ", bar),
                        Style::default().fg(crate::theme::magenta()),
                    )));
                }
                let glyph = if ev.is_past(&now) { "✓" } else { " " };
                let time_str = if ev.all_day {
                    "all day".to_string()
                } else if width.show_time_range() {
                    fmt_time_range(&ev.start, &ev.end)
                } else {
                    fmt_start(&ev.start)
                };
                let meet = if width.show_meet_glyph() && !ev.meet_url.is_empty() { "  📹" } else { "" };
                let row = format!("  {} {}  {}{}", glyph, time_str, ev.summary, meet);
                let mut style = if self.focus.day == di && self.focus.event == Some(ei) {
                    crate::theme::active_row()
                } else {
                    Style::default().fg(crate::theme::lavender())
                };
                if ev.is_past(&now) {
                    style = style.add_modifier(Modifier::DIM);
                }
                if ev.is_declined() {
                    style = style.add_modifier(Modifier::CROSSED_OUT);
                }
                lines.push(Line::from(Span::styled(row, style)));
            }
        }
        let title = if self.stale_cache { "cal (stale)" } else { "cal" };
        let p = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title));
        f.render_widget(p, area);

        if self.show_detail {
            self.render_detail_modal(f, area);
        }
    }

    fn render_detail_modal(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        use ratatui::layout::Margin;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
        let Some(day) = self.days.get(self.focus.day) else { return; };
        let Some(ei) = self.focus.event else { return; };
        let Some(ev) = day.events.get(ei) else { return; };
        let inner = area.inner(Margin { vertical: 2, horizontal: 4 });
        f.render_widget(Clear, inner);
        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled(format!("{} · {}", ev.summary, day.label), crate::theme::pane_header_focused())),
            Line::from(Span::styled(fmt_time_range(&ev.start, &ev.end), crate::theme::dim())),
            Line::from(""),
        ];
        if !ev.meet_url.is_empty() {
            lines.push(Line::from(Span::raw(format!("📹 {}", ev.meet_url))));
            lines.push(Line::from(""));
        }
        if !ev.location.is_empty() {
            lines.push(Line::from(Span::raw(format!("Location: {}", ev.location))));
            lines.push(Line::from(""));
        }
        if !ev.attendees.is_empty() {
            lines.push(Line::from(Span::styled(format!("Attendees ({})", ev.attendees.len()), crate::theme::dim())));
            for a in ev.attendees.iter().take(8) {
                let glyph = match a.response_status {
                    ResponseStatus::Accepted => "✓",
                    ResponseStatus::Declined => "✗",
                    ResponseStatus::Tentative => "~",
                    ResponseStatus::NeedsAction => "?",
                };
                let org = if a.organizer { "★" } else { " " };
                let me = if a.is_self { " (you)" } else { "" };
                let name = if a.name.is_empty() { a.email.clone() } else { a.name.clone() };
                lines.push(Line::from(Span::raw(format!("  {} {} {}{}", org, glyph, name, me))));
            }
            if ev.attendees.len() > 8 {
                lines.push(Line::from(Span::styled(format!("  ...{} more", ev.attendees.len() - 8), crate::theme::dim())));
            }
            lines.push(Line::from(""));
        }
        let plain = desc::strip_html(&ev.description);
        if !plain.is_empty() {
            lines.push(Line::from(Span::styled("Description", crate::theme::dim())));
            for l in plain.lines() {
                lines.push(Line::from(Span::raw(format!("  {}", l))));
            }
            lines.push(Line::from(""));
        }
        let mut links = desc::extract_urls(&ev.description);
        if !ev.html_link.is_empty() && !links.contains(&ev.html_link) {
            links.push(ev.html_link.clone());
        }
        if !links.is_empty() {
            lines.push(Line::from(Span::styled("Links", crate::theme::dim())));
            for l in links {
                lines.push(Line::from(Span::raw(format!("  • {}", l))));
            }
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled("Esc / Enter / q / h / Left to close", crate::theme::dim())));
        let block = Block::default().borders(Borders::ALL).title("detail");
        let p = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
        f.render_widget(p, inner);
    }
}

fn today_local() -> jiff::civil::Date {
    Timestamp::now().to_zoned(jiff::tz::TimeZone::system()).date()
}

fn fmt_time_range(start: &str, end: &str) -> String {
    format!("{}–{}", fmt_start(start), fmt_start(end))
}

fn fmt_start(s: &str) -> String {
    if let Ok(ts) = s.parse::<Timestamp>() {
        let z = ts.to_zoned(jiff::tz::TimeZone::system());
        return format!("{:02}:{:02}", z.hour(), z.minute());
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    use crate::cal::view::DayGroup;
    use jiff::civil::Date;

    fn make_event(id: &str, start: &str, end: &str) -> Event {
        Event {
            id: id.into(), summary: id.into(), description: "".into(),
            location: "".into(), start: start.into(), end: end.into(),
            all_day: false, status: "confirmed".into(),
            html_link: "".into(), hangout_link: "".into(),
            meet_url: "https://meet.google.com/abc".into(),
            attendees: vec![], is_recurring: false, recurring_event_id: "".into(),
            calendar_id: "primary".into(),
        }
    }

    fn day(date_str: &str, is_today: bool, events: Vec<Event>) -> DayGroup {
        let date: Date = date_str.parse().unwrap();
        DayGroup { date, label: "TEST".into(), is_today, events }
    }

    fn fresh_core(days: Vec<DayGroup>) -> CalCore {
        let mut expanded = HashSet::new();
        for d in &days {
            if d.is_today { expanded.insert(d.date); }
        }
        CalCore {
            days, focus: Focus { day: 0, event: Some(0) },
            expanded, show_detail: false, show_past: true,
            last_fetched: Some(Instant::now()),
            last_toast: None, stale_cache: false, last_fetch_error: None, rx: None,
        }
    }

    #[test]
    fn drill_in_and_out_walks_levels() {
        let mut core = fresh_core(vec![
            day("2026-05-26", true, vec![make_event("a", "2026-05-26T09:00:00-05:00", "2026-05-26T10:00:00-05:00")]),
        ]);
        core.expanded.clear();
        core.focus.event = None;
        core.drill_in();
        assert!(core.expanded.contains(&"2026-05-26".parse::<Date>().unwrap()));
        assert_eq!(core.focus.event, Some(0));
        core.drill_in();
        assert!(core.show_detail);
        core.drill_out();
        assert!(!core.show_detail);
        core.drill_out();
        assert_eq!(core.focus.event, None);
    }

    #[test]
    fn copy_url_yields_action_when_url_present() {
        let mut core = fresh_core(vec![
            day("2026-05-26", true, vec![make_event("a", "2026-05-26T09:00:00-05:00", "2026-05-26T10:00:00-05:00")]),
        ]);
        let action = core.copy_url();
        assert!(matches!(action, CalAction::CopiedUrl));
    }

    #[test]
    fn copy_url_noops_when_url_empty() {
        let mut ev = make_event("a", "2026-05-26T09:00:00-05:00", "2026-05-26T10:00:00-05:00");
        ev.meet_url = "".into();
        let mut core = fresh_core(vec![day("2026-05-26", true, vec![ev])]);
        let action = core.copy_url();
        assert!(matches!(action, CalAction::None));
    }

    #[test]
    fn detail_text_includes_url_and_summary() {
        let core = fresh_core(vec![
            day("2026-05-26", true, vec![make_event("standup", "2026-05-26T09:00:00-05:00", "2026-05-26T10:00:00-05:00")]),
        ]);
        let text = core.detail_text().unwrap();
        assert!(text.contains("Event: standup"));
        assert!(text.contains("Meet URL: https://meet.google.com/abc"));
    }

    #[test]
    fn toggle_past_flips_state() {
        let mut core = fresh_core(vec![]);
        assert!(core.show_past);
        core.toggle_past();
        assert!(!core.show_past);
    }

    #[test]
    fn toggle_expand_flips_focused_day() {
        let mut core = fresh_core(vec![
            day("2026-05-26", true, vec![make_event("a", "2026-05-26T09:00:00-05:00", "2026-05-26T10:00:00-05:00")]),
        ]);
        let d: Date = "2026-05-26".parse().unwrap();
        // fresh_core seeds today as expanded; collapse it.
        core.toggle_expand();
        assert!(!core.expanded.contains(&d));
        // Expand again.
        core.toggle_expand();
        assert!(core.expanded.contains(&d));
    }

    #[test]
    fn move_down_walks_visible_rows() {
        let mut core = fresh_core(vec![
            day("2026-05-26", true, vec![
                make_event("a", "2026-05-26T09:00:00-05:00", "2026-05-26T10:00:00-05:00"),
                make_event("b", "2026-05-26T11:00:00-05:00", "2026-05-26T12:00:00-05:00"),
            ]),
        ]);
        core.focus = Focus { day: 0, event: None };
        core.move_down();
        assert_eq!(core.focus.event, Some(0));
        core.move_down();
        assert_eq!(core.focus.event, Some(1));
        core.move_down();
        assert_eq!(core.focus.event, Some(1));
    }
}
