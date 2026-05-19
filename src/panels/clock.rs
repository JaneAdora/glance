//! Big digital clock with date, ISO week / day-of-year / TZ metadata, and a
//! day-progress gauge. Per-component color hierarchy. Blinking colons.
//! `f` toggles 12-hour / 24-hour format.
use crate::panels::Panel;
use crate::theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::Frame;

const DIGITS: [[&str; 5]; 10] = [
    [
        // 0
        "███", "█ █", "█ █", "█ █", "███",
    ],
    [
        // 1
        "  █", "  █", "  █", "  █", "  █",
    ],
    [
        // 2
        "███", "  █", "███", "█  ", "███",
    ],
    [
        // 3
        "███", "  █", "███", "  █", "███",
    ],
    [
        // 4
        "█ █", "█ █", "███", "  █", "  █",
    ],
    [
        // 5
        "███", "█  ", "███", "  █", "███",
    ],
    [
        // 6
        "███", "█  ", "███", "█ █", "███",
    ],
    [
        // 7
        "███", "  █", "  █", "  █", "  █",
    ],
    [
        // 8
        "███", "█ █", "███", "█ █", "███",
    ],
    [
        // 9
        "███", "█ █", "███", "  █", "███",
    ],
];

const COLON_ROWS: [&str; 5] = [" ", "█", " ", "█", " "];

pub struct ClockPanel {
    format_24h: bool,
    // Cached "current" components, refreshed on tick.
    h24: u8,
    minute: u8,
    second: u8,
    year: i32,
    month: u8,
    day: u8,
    weekday: u8,
    day_of_year: u16,
    iso_week: u8,
    tz_label: String,
    seconds_into_day: u32,
}

impl ClockPanel {
    pub fn new() -> Self {
        let format_24h = std::env::var("GLANCE_CLOCK_FORMAT")
            .map(|s| s.trim() != "12")
            .unwrap_or(true);
        Self {
            format_24h,
            h24: 0,
            minute: 0,
            second: 0,
            year: 1970,
            month: 1,
            day: 1,
            weekday: 0,
            day_of_year: 1,
            iso_week: 1,
            tz_label: String::from("UTC"),
            seconds_into_day: 0,
        }
    }

    fn refresh_now(&mut self) {
        let zoned = jiff::Zoned::now();
        let date = zoned.date();
        let time = zoned.time();
        self.h24 = time.hour() as u8;
        self.minute = time.minute() as u8;
        self.second = time.second() as u8;
        self.year = date.year() as i32;
        self.month = date.month() as u8;
        self.day = date.day() as u8;
        // weekday: Monday=1 .. Sunday=7 from jiff; we just store 0..6 with Sun=0 for our use
        let wd_num = date.weekday().to_sunday_zero_offset(); // 0=Sun..6=Sat
        self.weekday = wd_num as u8;
        self.day_of_year = date.day_of_year() as u16;
        self.iso_week = date.iso_week_date().week() as u8;
        self.tz_label = zoned.time_zone().iana_name().unwrap_or("local").to_string();
        self.seconds_into_day =
            self.h24 as u32 * 3600 + self.minute as u32 * 60 + self.second as u32;
    }

    fn display_hour(&self) -> u8 {
        if self.format_24h {
            self.h24
        } else {
            match self.h24 {
                0 => 12,
                h if h > 12 => h - 12,
                h => h,
            }
        }
    }

    fn am_pm(&self) -> &'static str {
        if self.h24 < 12 {
            "AM"
        } else {
            "PM"
        }
    }
}

fn weekday_name(wd: u8) -> &'static str {
    // 0=Sun..6=Sat
    ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]
        .get(wd as usize)
        .copied()
        .unwrap_or("?")
}

fn month_name(m: u8) -> &'static str {
    [
        "?", "January", "February", "March", "April", "May", "June",
        "July", "August", "September", "October", "November", "December",
    ]
    .get(m as usize)
    .copied()
    .unwrap_or("?")
}

/// Compose a single row of the big-digit clock as a Vec<Span>.
fn time_row(
    hour: u8,
    minute: u8,
    second: u8,
    show_seconds: bool,
    blink_on: bool,
    row: usize,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let hour_style = Style::default()
        .fg(theme::magenta())
        .add_modifier(ratatui::style::Modifier::BOLD);
    let min_style = Style::default().fg(theme::pink());
    let sec_style = theme::historical();
    let colon_style = theme::dim();

    let hour_str = format!("{:02}", hour);
    let min_str = format!("{:02}", minute);
    let sec_str = format!("{:02}", second);

    // HH
    for ch in hour_str.chars() {
        let d = ch.to_digit(10).unwrap_or(0) as usize;
        spans.push(Span::styled(DIGITS[d][row].to_string(), hour_style));
        spans.push(Span::raw(" "));
    }
    // :
    let colon = if blink_on { COLON_ROWS[row] } else { " " };
    spans.push(Span::styled(colon.to_string(), colon_style));
    spans.push(Span::raw(" "));
    // MM
    for ch in min_str.chars() {
        let d = ch.to_digit(10).unwrap_or(0) as usize;
        spans.push(Span::styled(DIGITS[d][row].to_string(), min_style));
        spans.push(Span::raw(" "));
    }
    if show_seconds {
        // :
        spans.push(Span::styled(colon.to_string(), colon_style));
        spans.push(Span::raw(" "));
        // SS
        for ch in sec_str.chars() {
            let d = ch.to_digit(10).unwrap_or(0) as usize;
            spans.push(Span::styled(DIGITS[d][row].to_string(), sec_style));
            spans.push(Span::raw(" "));
        }
    }
    spans
}

impl Panel for ClockPanel {
    fn name(&self) -> &str {
        "clock"
    }

    fn refresh_ms(&self) -> u64 {
        500
    }

    fn tick(&mut self) {
        self.refresh_now();
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if let KeyCode::Char('f') = key.code {
            self.format_24h = !self.format_24h;
            return true;
        }
        false
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        // Width-aware: show seconds when wide enough for HH MM SS plus colons.
        // Big digits: per digit = 3 cols + 1 gap. HH MM = 3+1+3+1+1+1+3+1+3 = 17 cols.
        // HH MM SS adds another 1+1+1+3+1+3 = 10 cols → 27 total.
        let show_seconds = area.width >= 32;
        let show_progress = area.height >= 9;
        let show_meta = area.height >= 7;

        // Layout: title bar (1 row) + big clock (5 rows) + date (1 row) + meta (1 row, optional) + progress (1 row, optional)
        let mut constraints: Vec<Constraint> = vec![
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(2),
        ];
        if show_meta {
            constraints.push(Constraint::Length(1));
        }
        if show_progress {
            constraints.push(Constraint::Length(1));
        }
        constraints.push(Constraint::Min(0));

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Title bar with format chip
        let fmt_label = if self.format_24h { "24h" } else { "12h" };
        let title = Line::from(vec![
            Span::styled(" clock ", theme::pane_header()),
            Span::styled(
                format!("[{fmt_label}]"),
                theme::pane_header_focused(),
            ),
            Span::styled(format!("  {}  press ", self.tz_label), theme::dim()),
            Span::styled("f", theme::pane_header_focused()),
            Span::styled(" to toggle", theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        // Big digital clock: 5 rows, blinking colon synced to seconds
        let blink_on = (self.second & 1) == 0;
        let hour_display = self.display_hour();
        let mut lines = Vec::with_capacity(5);
        for row in 0..5 {
            lines.push(Line::from(time_row(
                hour_display,
                self.minute,
                self.second,
                show_seconds,
                blink_on,
                row,
            )));
        }
        f.render_widget(
            Paragraph::new(lines).alignment(Alignment::Center),
            chunks[1],
        );

        // Date line + AM/PM badge if 12h
        let date_line_text = format!(
            "{}, {} {}, {}",
            weekday_name(self.weekday),
            month_name(self.month),
            self.day,
            self.year
        );
        let mut date_spans = vec![Span::styled(
            date_line_text,
            Style::default()
                .fg(theme::pink())
                .add_modifier(ratatui::style::Modifier::BOLD),
        )];
        if !self.format_24h {
            date_spans.push(Span::raw("  "));
            date_spans.push(Span::styled(self.am_pm(), theme::pane_header_focused()));
        }
        f.render_widget(
            Paragraph::new(Line::from(date_spans)).alignment(Alignment::Center),
            chunks[2],
        );

        let mut next_idx = 3;
        if show_meta {
            let meta = Line::from(vec![
                Span::styled("ISO week ", theme::dim()),
                Span::styled(format!("{:02}", self.iso_week), theme::historical()),
                Span::styled("   day ", theme::dim()),
                Span::styled(
                    format!("{}/365", self.day_of_year),
                    theme::historical(),
                ),
                Span::styled("   tz ", theme::dim()),
                Span::styled(self.tz_label.clone(), theme::historical()),
            ]);
            f.render_widget(
                Paragraph::new(meta).alignment(Alignment::Center),
                chunks[next_idx],
            );
            next_idx += 1;
        }

        if show_progress {
            let day_pct = ((self.seconds_into_day as f64 / 86400.0) * 100.0).round() as u16;
            let gauge = Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::NONE)
                        .title(Line::from(vec![
                            Span::styled(" day progress  ", theme::dim()),
                            Span::styled(format!("{day_pct}%"), theme::historical()),
                        ])),
                )
                .gauge_style(Style::default().fg(theme::magenta()))
                .percent(day_pct);
            f.render_widget(gauge, chunks[next_idx]);
        }
    }
}
