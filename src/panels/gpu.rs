//! NVIDIA GPU stats via nvidia-smi. Utilization + memory + temperature gauges
//! plus a utilization-history sparkline. Graceful absent on non-NVIDIA boxes.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Sparkline};
use ratatui::Frame;
use std::collections::VecDeque;
use std::process::Command;

const HIST: usize = 60;

#[derive(Clone, Default)]
struct GpuStat {
    name: String,
    util: u16,
    mem_used: u64,
    mem_total: u64,
    temp: u16,
}

pub struct GpuPanel {
    available: bool,
    gpu: GpuStat,
    util_hist: VecDeque<u64>,
}

impl GpuPanel {
    pub fn new() -> Self {
        Self {
            available: false,
            gpu: GpuStat::default(),
            util_hist: VecDeque::with_capacity(HIST),
        }
    }
}

fn query_gpu() -> Option<GpuStat> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let first = line.lines().next()?;
    let f: Vec<&str> = first.split(',').map(|s| s.trim()).collect();
    if f.len() < 5 {
        return None;
    }
    Some(GpuStat {
        name: f[0].to_string(),
        util: f[1].parse().unwrap_or(0),
        mem_used: f[2].parse().unwrap_or(0),
        mem_total: f[3].parse::<u64>().unwrap_or(1).max(1),
        temp: f[4].parse().unwrap_or(0),
    })
}

fn util_style(pct: u16) -> Style {
    if pct >= 85 {
        theme::alert()
    } else if pct >= 50 {
        Style::default().fg(theme::pink())
    } else {
        Style::default().fg(theme::lavender())
    }
}

impl Panel for GpuPanel {
    fn name(&self) -> &str {
        "gpu"
    }

    fn refresh_ms(&self) -> u64 {
        1_500
    }

    fn tick(&mut self) {
        match query_gpu() {
            Some(g) => {
                self.available = true;
                if self.util_hist.len() == HIST {
                    self.util_hist.pop_front();
                }
                self.util_hist.push_back(g.util as u64);
                self.gpu = g;
            }
            None => self.available = false,
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(2), // util gauge
                Constraint::Length(2), // mem gauge
                Constraint::Length(2), // temp gauge
                Constraint::Min(3),    // util history
            ])
            .split(area);

        if !self.available {
            f.render_widget(
                ratatui::widgets::Paragraph::new(" gpu ").style(theme::pane_header()),
                chunks[0],
            );
            f.render_widget(
                crate::widgets::empty("no NVIDIA GPU (nvidia-smi unavailable)"),
                chunks[1],
            );
            return;
        }

        let g = &self.gpu;
        let title = Line::from(vec![
            Span::styled(" gpu ", theme::pane_header()),
            Span::styled(g.name.clone(), theme::pane_header_focused()),
        ]);
        f.render_widget(ratatui::widgets::Paragraph::new(title), chunks[0]);

        // Utilization
        let util_g = Gauge::default()
            .block(Block::default().borders(Borders::NONE).title(Line::from(vec![
                Span::styled(" util ", theme::pane_header()),
                Span::styled(format!("{}%", g.util), util_style(g.util)),
            ])))
            .gauge_style(util_style(g.util))
            .percent(g.util.min(100));
        f.render_widget(util_g, chunks[1]);

        // Memory
        let mem_pct = ((g.mem_used * 100) / g.mem_total).min(100) as u16;
        let mem_g = Gauge::default()
            .block(Block::default().borders(Borders::NONE).title(Line::from(vec![
                Span::styled(" vram ", theme::pane_header()),
                Span::styled(
                    format!("{} / {} MiB", g.mem_used, g.mem_total),
                    theme::dim(),
                ),
            ])))
            .gauge_style(util_style(mem_pct))
            .percent(mem_pct);
        f.render_widget(mem_g, chunks[2]);

        // Temperature (scaled to 100C)
        let temp_pct = g.temp.min(100);
        let temp_style = if g.temp >= 80 {
            theme::alert()
        } else if g.temp >= 60 {
            Style::default().fg(theme::pink())
        } else {
            Style::default().fg(theme::lavender())
        };
        let temp_g = Gauge::default()
            .block(Block::default().borders(Borders::NONE).title(Line::from(vec![
                Span::styled(" temp ", theme::pane_header()),
                Span::styled(format!("{}°C", g.temp), temp_style),
            ])))
            .gauge_style(temp_style)
            .percent(temp_pct);
        f.render_widget(temp_g, chunks[3]);

        // Utilization history
        let data: Vec<u64> = self.util_hist.iter().copied().collect();
        let spark = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(theme::dim())
                    .title(Line::from(Span::styled(" util history ", theme::pane_header()))),
            )
            .data(&data)
            .max(100)
            .style(Style::default().fg(theme::pink()));
        f.render_widget(spark, chunks[4]);
    }
}
