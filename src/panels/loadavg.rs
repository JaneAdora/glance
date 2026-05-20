//! System load average: 1/5/15-minute gauges (relative to core count) plus a
//! sparkline of recent 1-minute load. Reads /proc/loadavg.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Sparkline};
use ratatui::Frame;
use std::collections::VecDeque;
use std::fs;

const HIST: usize = 60;

pub struct LoadavgPanel {
    one: f64,
    five: f64,
    fifteen: f64,
    ncpu: usize,
    history: VecDeque<u64>, // 1-min load × 100, for the sparkline
}

impl LoadavgPanel {
    pub fn new() -> Self {
        Self {
            one: 0.0,
            five: 0.0,
            fifteen: 0.0,
            ncpu: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            history: VecDeque::with_capacity(HIST),
        }
    }
}

fn read_loadavg() -> Option<(f64, f64, f64)> {
    let s = fs::read_to_string("/proc/loadavg").ok()?;
    let mut it = s.split_whitespace();
    let one = it.next()?.parse().ok()?;
    let five = it.next()?.parse().ok()?;
    let fifteen = it.next()?.parse().ok()?;
    Some((one, five, fifteen))
}

fn load_style(load: f64, ncpu: f64) -> Style {
    let ratio = load / ncpu.max(1.0);
    if ratio >= 1.0 {
        theme::alert()
    } else if ratio >= 0.6 {
        Style::default().fg(theme::pink())
    } else {
        Style::default().fg(theme::lavender())
    }
}

impl Panel for LoadavgPanel {
    fn name(&self) -> &str {
        "loadavg"
    }

    fn refresh_ms(&self) -> u64 {
        2_000
    }

    fn tick(&mut self) {
        if let Some((a, b, c)) = read_loadavg() {
            self.one = a;
            self.five = b;
            self.fifteen = c;
            if self.history.len() == HIST {
                self.history.pop_front();
            }
            self.history.push_back((a * 100.0) as u64);
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(2), // 1-min gauge
                Constraint::Length(2), // 5-min gauge
                Constraint::Length(2), // 15-min gauge
                Constraint::Min(3),    // sparkline
            ])
            .split(area);

        let title = Line::from(vec![
            Span::styled(" loadavg ", theme::pane_header()),
            Span::styled(format!("{} cores", self.ncpu), theme::dim()),
        ]);
        f.render_widget(ratatui::widgets::Paragraph::new(title), chunks[0]);

        let ncpu = self.ncpu as f64;
        for (i, (label, load)) in [
            ("1 min", self.one),
            ("5 min", self.five),
            ("15 min", self.fifteen),
        ]
        .iter()
        .enumerate()
        {
            let pct = ((load / ncpu) * 100.0).min(100.0) as u16;
            let g = Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::NONE)
                        .title(Line::from(vec![
                            Span::styled(format!(" {label} "), theme::pane_header()),
                            Span::styled(format!("{:.2}", load), load_style(*load, ncpu)),
                        ])),
                )
                .gauge_style(load_style(*load, ncpu))
                .percent(pct);
            f.render_widget(g, chunks[i + 1]);
        }

        let data: Vec<u64> = self.history.iter().copied().collect();
        let max = (ncpu * 100.0) as u64;
        let spark = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(theme::dim())
                    .title(Line::from(Span::styled(
                        " 1-min load history ",
                        theme::pane_header(),
                    ))),
            )
            .data(&data)
            .max(max.max(100))
            .style(Style::default().fg(theme::pink()));
        f.render_widget(spark, chunks[4]);
    }
}
