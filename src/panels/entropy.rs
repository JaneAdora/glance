//! Kernel entropy pool. Reads /proc/sys/kernel/random/entropy_avail. On modern
//! kernels this maxes at 256; on older ones up to 4096. Sparkline + gauge.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Sparkline};
use ratatui::Frame;
use std::collections::VecDeque;
use std::fs;

const HIST: usize = 60;

pub struct EntropyPanel {
    current: u32,
    max_seen: u32,
    history: VecDeque<u64>,
}

impl EntropyPanel {
    pub fn new() -> Self {
        Self {
            current: 0,
            max_seen: 256,
            history: VecDeque::with_capacity(HIST),
        }
    }
}

fn read_entropy() -> Option<u32> {
    fs::read_to_string("/proc/sys/kernel/random/entropy_avail")
        .ok()?
        .trim()
        .parse()
        .ok()
}

impl Panel for EntropyPanel {
    fn name(&self) -> &str {
        "entropy"
    }

    fn refresh_ms(&self) -> u64 {
        1_000
    }

    fn tick(&mut self) {
        if let Some(v) = read_entropy() {
            self.current = v;
            // Pool ceiling: 256 on modern kernels, 4096 on old ones. Track the max.
            if v > self.max_seen {
                self.max_seen = if v > 256 { 4096 } else { 256 };
            }
            if self.history.len() == HIST {
                self.history.pop_front();
            }
            self.history.push_back(v as u64);
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(2), // big number
                Constraint::Length(2), // gauge
                Constraint::Min(3),    // sparkline
            ])
            .split(area);

        let title = Line::from(vec![
            Span::styled(" entropy ", theme::pane_header()),
            Span::styled("/proc/sys/kernel/random", theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        let big = Line::from(vec![
            Span::styled(
                format!("{}", self.current),
                Style::default()
                    .fg(theme::magenta())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(format!(" / {} bits", self.max_seen), theme::historical()),
        ]);
        f.render_widget(Paragraph::new(big).alignment(Alignment::Center), chunks[1]);

        let pct = ((self.current as u64 * 100) / self.max_seen.max(1) as u64).min(100) as u16;
        let style = if pct < 25 {
            theme::alert()
        } else {
            Style::default().fg(theme::pink())
        };
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::NONE))
            .gauge_style(style)
            .percent(pct);
        f.render_widget(gauge, chunks[2]);

        let data: Vec<u64> = self.history.iter().copied().collect();
        let spark = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(theme::dim())
                    .title(Line::from(Span::styled(" pool history ", theme::pane_header()))),
            )
            .data(&data)
            .max(self.max_seen as u64)
            .style(Style::default().fg(theme::pink()));
        f.render_widget(spark, chunks[3]);
    }
}
