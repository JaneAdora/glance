//! Pomodoro / countdown timer with a shrinking Canvas ring. First panel with
//! genuine interactive state. Keys: space start/pause, x reset, w work, b break.
use crate::layout::braille_aspect_bounds;
use crate::panels::Panel;
use crate::theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::f64::consts::TAU;
use std::time::Instant;

const DIGITS: [[&str; 5]; 10] = [
    ["███", "█ █", "█ █", "█ █", "███"],
    ["  █", "  █", "  █", "  █", "  █"],
    ["███", "  █", "███", "█  ", "███"],
    ["███", "  █", "███", "  █", "███"],
    ["█ █", "█ █", "███", "  █", "  █"],
    ["███", "█  ", "███", "  █", "███"],
    ["███", "█  ", "███", "█ █", "███"],
    ["███", "  █", "  █", "  █", "  █"],
    ["███", "█ █", "███", "█ █", "███"],
    ["███", "█ █", "███", "  █", "███"],
];
const COLON: [&str; 5] = [" ", "█", " ", "█", " "];

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Work,
    Break,
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Idle,
    Running,
    Paused,
    Done,
}

pub struct TimerPanel {
    mode: Mode,
    status: Status,
    work_secs: u64,
    break_secs: u64,
    remaining_secs: u64,
    resumed_at: Option<Instant>,
    completed: u32,
}

impl TimerPanel {
    pub fn new() -> Self {
        let work = std::env::var("GLANCE_POMODORO_WORK")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(25u64)
            * 60;
        let brk = std::env::var("GLANCE_POMODORO_BREAK")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5u64)
            * 60;
        Self {
            mode: Mode::Work,
            status: Status::Idle,
            work_secs: work,
            break_secs: brk,
            remaining_secs: work,
            resumed_at: None,
            completed: 0,
        }
    }

    fn duration(&self) -> u64 {
        match self.mode {
            Mode::Work => self.work_secs,
            Mode::Break => self.break_secs,
        }
    }

    fn current_remaining(&self) -> u64 {
        match self.status {
            Status::Running => {
                let elapsed = self.resumed_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);
                self.remaining_secs.saturating_sub(elapsed)
            }
            _ => self.remaining_secs,
        }
    }

    fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.status = Status::Idle;
        self.remaining_secs = self.duration();
        self.resumed_at = None;
    }

    fn toggle(&mut self) {
        match self.status {
            Status::Idle | Status::Paused => {
                self.status = Status::Running;
                self.resumed_at = Some(Instant::now());
            }
            Status::Running => {
                self.remaining_secs = self.current_remaining();
                self.status = Status::Paused;
                self.resumed_at = None;
            }
            Status::Done => {
                self.remaining_secs = self.duration();
                self.status = Status::Running;
                self.resumed_at = Some(Instant::now());
            }
        }
    }

    fn reset(&mut self) {
        self.status = Status::Idle;
        self.remaining_secs = self.duration();
        self.resumed_at = None;
    }
}

fn status_color(status: Status) -> Color {
    match status {
        Status::Running => theme::magenta(),
        Status::Paused => theme::lavender(),
        Status::Done => theme::pink(),
        Status::Idle => theme::lavender(),
    }
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Running => "running",
        Status::Paused => "paused",
        Status::Done => "done!",
        Status::Idle => "ready",
    }
}

fn time_row(mins: u64, secs: u64, row: usize, color: Color) -> Vec<Span<'static>> {
    let style = Style::default().fg(color);
    let mut spans = Vec::new();
    for ch in format!("{:02}", mins).chars() {
        let d = ch.to_digit(10).unwrap_or(0) as usize;
        spans.push(Span::styled(DIGITS[d][row].to_string(), style));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(COLON[row].to_string(), theme::dim()));
    spans.push(Span::raw(" "));
    for ch in format!("{:02}", secs).chars() {
        let d = ch.to_digit(10).unwrap_or(0) as usize;
        spans.push(Span::styled(DIGITS[d][row].to_string(), style));
        spans.push(Span::raw(" "));
    }
    spans
}

impl Panel for TimerPanel {
    fn name(&self) -> &str {
        "timer"
    }

    fn refresh_ms(&self) -> u64 {
        250
    }

    fn tick(&mut self) {
        if self.status == Status::Running && self.current_remaining() == 0 {
            self.status = Status::Done;
            self.remaining_secs = 0;
            self.resumed_at = None;
            if self.mode == Mode::Work {
                self.completed += 1;
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(' ') => {
                self.toggle();
                true
            }
            KeyCode::Char('x') => {
                self.reset();
                true
            }
            KeyCode::Char('w') => {
                self.set_mode(Mode::Work);
                true
            }
            KeyCode::Char('b') => {
                self.set_mode(Mode::Break);
                true
            }
            _ => false,
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let remaining = self.current_remaining();
        let mins = remaining / 60;
        let secs = remaining % 60;
        let color = status_color(self.status);
        let frac_remaining = if self.duration() > 0 {
            remaining as f64 / self.duration() as f64
        } else {
            0.0
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title (TOP)
                Constraint::Min(0),    // spacer pushes content to bottom
                Constraint::Length(1), // gap
                Constraint::Length(7), // ring + digits area
                Constraint::Length(1), // status label
                Constraint::Length(1), // gap
                Constraint::Length(1), // hint
            ])
            .split(area);

        let mode_label = match self.mode {
            Mode::Work => "work",
            Mode::Break => "break",
        };
        let title = Line::from(vec![
            Span::styled(" timer ", theme::pane_header()),
            Span::styled(format!("[{mode_label}]"), theme::pane_header_focused()),
            Span::styled(format!("   {} done today", self.completed), theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        // Ring (Canvas) behind, digits (Paragraph) overlaid centered.
        let ring_area = chunks[3];
        let mut ring_active: Vec<(f64, f64)> = Vec::new();
        let mut ring_spent: Vec<(f64, f64)> = Vec::new();
        let n = 120;
        for i in 0..n {
            // Start at top (12 o'clock), go clockwise.
            let frac = i as f64 / n as f64;
            let ang = TAU * 0.25 - frac * TAU; // clockwise from top
            let pt = (ang.cos(), ang.sin());
            if frac <= frac_remaining {
                ring_active.push(pt);
            } else {
                ring_spent.push(pt);
            }
        }
        let (xb, yb) = braille_aspect_bounds(ring_area, 1.05, 1.05);
        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds(xb)
            .y_bounds(yb)
            .paint(move |ctx| {
                ctx.draw(&Points {
                    coords: &ring_spent,
                    color: theme::map_border(),
                });
                ctx.layer();
                ctx.draw(&Points {
                    coords: &ring_active,
                    color,
                });
            });
        f.render_widget(canvas, ring_area);

        // Digits overlaid in the vertical center of the ring area.
        let digit_rows = 5u16;
        if ring_area.height >= digit_rows {
            let pad = (ring_area.height - digit_rows) / 2;
            let digit_area = Rect {
                x: ring_area.x,
                y: ring_area.y + pad,
                width: ring_area.width,
                height: digit_rows,
            };
            let lines: Vec<Line> = (0..5)
                .map(|row| Line::from(time_row(mins, secs, row, color)))
                .collect();
            f.render_widget(
                Paragraph::new(lines).alignment(Alignment::Center),
                digit_area,
            );
        }

        let status_line = Line::from(Span::styled(status_label(self.status), Style::default().fg(color)));
        f.render_widget(
            Paragraph::new(status_line).alignment(Alignment::Center),
            chunks[4],
        );

        let hint = Line::from(vec![
            Span::styled("space", theme::pane_header_focused()),
            Span::styled(" start/pause  ", theme::dim()),
            Span::styled("x", theme::pane_header_focused()),
            Span::styled(" reset  ", theme::dim()),
            Span::styled("w", theme::pane_header_focused()),
            Span::styled(" work  ", theme::dim()),
            Span::styled("b", theme::pane_header_focused()),
            Span::styled(" break", theme::dim()),
        ]);
        f.render_widget(Paragraph::new(hint).alignment(Alignment::Center), chunks[6]);
    }
}
