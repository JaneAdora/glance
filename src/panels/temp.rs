use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, Borders, Paragraph};
use ratatui::Frame;
use std::fs;
use std::path::PathBuf;

pub struct TempPanel {
    zones: Vec<Zone>,
}

struct Zone {
    label: String,
    celsius: f64,
}

impl TempPanel {
    pub fn new() -> Self {
        Self { zones: Vec::new() }
    }
}

fn read_zones() -> Vec<Zone> {
    let dir = PathBuf::from("/sys/class/thermal");
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut zones = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name();
        let n = name.to_string_lossy();
        if !n.starts_with("thermal_zone") {
            continue;
        }
        let path = e.path();
        let kind = fs::read_to_string(path.join("type"))
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| n.to_string());
        let mc: i64 = match fs::read_to_string(path.join("temp")) {
            Ok(s) => s.trim().parse().unwrap_or(0),
            Err(_) => continue,
        };
        let c = mc as f64 / 1000.0;
        // Filter obviously bogus readings (some sensors report 0 or weird negatives when idle).
        if !c.is_finite() || c < -20.0 || c > 200.0 {
            continue;
        }
        zones.push(Zone {
            label: kind,
            celsius: c,
        });
    }
    // Sort hottest first so the most interesting bar is visually first.
    zones.sort_by(|a, b| b.celsius.partial_cmp(&a.celsius).unwrap_or(std::cmp::Ordering::Equal));
    zones
}

fn color_for(c: f64) -> Color {
    if c >= 80.0 {
        theme::magenta() // magenta alert
    } else if c >= 60.0 {
        theme::pink() // pink warm
    } else {
        theme::lavender() // lavender cool
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

impl Panel for TempPanel {
    fn name(&self) -> &str {
        "temp"
    }

    fn refresh_ms(&self) -> u64 {
        2_000
    }

    fn tick(&mut self) {
        self.zones = read_zones();
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        if self.zones.is_empty() {
            f.render_widget(crate::widgets::empty("no thermal zones; /sys/class/thermal unavailable"), area);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(3)])
            .split(area);

        let hottest = &self.zones[0];
        let header = Line::from(vec![
            Span::styled(" hottest: ", theme::pane_header()),
            Span::styled(
                format!("{} {:.1}°C", hottest.label, hottest.celsius),
                Style::default().fg(color_for(hottest.celsius)),
            ),
            Span::styled(
                format!("    {} zones", self.zones.len()),
                theme::dim(),
            ),
        ]);
        f.render_widget(Paragraph::new(header), chunks[0]);

        // Bar chart: one bar per zone, label is the zone type (truncated).
        let max_temp = self
            .zones
            .iter()
            .map(|z| z.celsius)
            .fold(40.0_f64, f64::max);
        let y_max = ((max_temp / 10.0).ceil() * 10.0).max(40.0) as u64;

        let bars: Vec<Bar> = self
            .zones
            .iter()
            .map(|z| {
                let val = z.celsius.max(0.0).round() as u64;
                let label_short = truncate(&z.label.to_lowercase(), 10);
                Bar::default()
                    .value(val)
                    .text_value(format!("{:.0}°", z.celsius))
                    .label(Line::from(label_short))
                    .style(Style::default().fg(color_for(z.celsius)))
                    .value_style(Style::default().fg(color_for(z.celsius)))
            })
            .collect();

        let chart = BarChart::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(theme::dim())
                    .title(Line::from(Span::styled(
                        format!(" thermal zones (°C, 0–{}) ", y_max),
                        theme::pane_header(),
                    ))),
            )
            .bar_width(8)
            .bar_gap(1)
            .max(y_max)
            .data(BarGroup::default().bars(&bars))
            .label_style(theme::dim());
        f.render_widget(chart, chunks[1]);
    }
}
