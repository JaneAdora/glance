use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Sparkline};
use ratatui::Frame;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

const HIST: usize = 60;

pub struct BatteryPanel {
    available: bool,
    name: String,
    capacity_pct: u16,
    status: String,
    history: VecDeque<u64>,
}

impl BatteryPanel {
    pub fn new() -> Self {
        Self {
            available: false,
            name: String::new(),
            capacity_pct: 0,
            status: String::new(),
            history: VecDeque::with_capacity(HIST),
        }
    }
}

fn find_battery() -> Option<PathBuf> {
    let base = PathBuf::from("/sys/class/power_supply");
    let entries = fs::read_dir(&base).ok()?;
    for e in entries.flatten() {
        let name = e.file_name();
        let n = name.to_string_lossy();
        if n.starts_with("BAT") {
            return Some(e.path());
        }
    }
    None
}

fn read_pct(path: &PathBuf) -> Option<u16> {
    let s = fs::read_to_string(path.join("capacity")).ok()?;
    s.trim().parse::<u16>().ok()
}

fn read_status(path: &PathBuf) -> String {
    fs::read_to_string(path.join("status"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "Unknown".to_string())
}

impl Panel for BatteryPanel {
    fn name(&self) -> &str {
        "battery"
    }

    fn refresh_ms(&self) -> u64 {
        10_000
    }

    fn tick(&mut self) {
        let bat = match find_battery() {
            Some(p) => p,
            None => {
                self.available = false;
                return;
            }
        };
        self.available = true;
        self.name = bat
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "BAT".to_string());
        self.capacity_pct = read_pct(&bat).unwrap_or(0);
        self.status = read_status(&bat);
        if self.history.len() == HIST {
            self.history.pop_front();
        }
        self.history.push_back(self.capacity_pct as u64);
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        if !self.available {
            f.render_widget(
                ratatui::widgets::Paragraph::new(Line::from(vec![
                    Span::styled("(no battery detected) ", theme::dim()),
                    Span::styled("desktop or VM?", theme::historical()),
                ])),
                area,
            );
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(2)])
            .split(area);

        let style = if self.capacity_pct <= 15 {
            theme::alert()
        } else if self.capacity_pct <= 30 {
            theme::now()
        } else {
            theme::historical()
        };
        let charge_marker = match self.status.as_str() {
            "Charging" => "⚡",
            "Discharging" => "↓",
            "Full" => "✓",
            _ => "·",
        };
        let g = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::NONE)
                    .title(Line::from(vec![
                        Span::styled(format!(" {} {} ", self.name, charge_marker), theme::pane_header()),
                        Span::styled(self.status.clone(), theme::dim()),
                    ])),
            )
            .gauge_style(style)
            .percent(self.capacity_pct);
        f.render_widget(g, chunks[0]);

        let data: Vec<u64> = self.history.iter().copied().collect();
        let spark = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(theme::dim())
                    .title(Line::from(Span::styled(" charge history ", theme::pane_header()))),
            )
            .data(&data)
            .max(100)
            .style(style);
        f.render_widget(spark, chunks[1]);
    }
}
