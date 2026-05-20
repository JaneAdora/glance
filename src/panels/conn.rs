//! TCP connection breakdown by state via `ss -tan`. BarChart of state counts.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, Borders, Paragraph};
use ratatui::Frame;
use std::collections::BTreeMap;
use std::process::Command;

pub struct ConnPanel {
    counts: BTreeMap<String, u64>,
    total: u64,
    available: bool,
}

impl ConnPanel {
    pub fn new() -> Self {
        Self {
            counts: BTreeMap::new(),
            total: 0,
            available: true,
        }
    }
}

fn read_conn_states() -> Option<BTreeMap<String, u64>> {
    let out = Command::new("ss").args(["-tan"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 {
            continue; // header
        }
        if let Some(state) = line.split_whitespace().next() {
            *counts.entry(state.to_string()).or_insert(0) += 1;
        }
    }
    Some(counts)
}

fn state_color(state: &str) -> Color {
    match state {
        "ESTAB" => theme::pink(),
        "LISTEN" => theme::lavender(),
        "TIME-WAIT" => theme::dim_purple(),
        "CLOSE-WAIT" | "CLOSING" => theme::magenta(),
        "SYN-SENT" | "SYN-RECV" => theme::sage(),
        _ => theme::dim_purple(),
    }
}

impl Panel for ConnPanel {
    fn name(&self) -> &str {
        "conn"
    }

    fn refresh_ms(&self) -> u64 {
        2_000
    }

    fn tick(&mut self) {
        match read_conn_states() {
            Some(c) => {
                self.total = c.values().sum();
                self.counts = c;
                self.available = true;
            }
            None => self.available = false,
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(3)])
            .split(area);

        let title = Line::from(vec![
            Span::styled(" conn ", theme::pane_header()),
            Span::styled("TCP via ss -tan", theme::dim()),
            Span::styled(format!("   {} total", self.total), theme::historical()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        if !self.available {
            f.render_widget(crate::widgets::error("ss unavailable"), chunks[1]);
            return;
        }
        if self.counts.is_empty() {
            f.render_widget(crate::widgets::empty("no TCP connections"), chunks[1]);
            return;
        }

        // Sort states by count desc for visual priority.
        let mut entries: Vec<(&String, &u64)> = self.counts.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));

        let max = entries.iter().map(|(_, c)| **c).max().unwrap_or(1);
        let bars: Vec<Bar> = entries
            .iter()
            .map(|(state, count)| {
                let color = state_color(state);
                Bar::default()
                    .value(**count)
                    .text_value(format!("{count}"))
                    .label(Line::from(state.to_lowercase()))
                    .style(Style::default().fg(color))
                    .value_style(Style::default().fg(color))
            })
            .collect();

        let chart = BarChart::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(theme::dim())
                    .title(Line::from(Span::styled(" by state ", theme::pane_header()))),
            )
            .bar_width(9)
            .bar_gap(1)
            .max(max)
            .data(BarGroup::default().bars(&bars))
            .label_style(theme::dim());
        f.render_widget(chart, chunks[1]);
    }
}
