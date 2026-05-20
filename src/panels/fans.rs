//! Fan speeds from /sys/class/hwmon. One gauge per fan (scaled to a nominal
//! max RPM). Graceful empty state when no fan sensors expose a reading.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge};
use ratatui::Frame;
use std::fs;
use std::path::PathBuf;

const NOMINAL_MAX_RPM: u64 = 6000;

struct Fan {
    label: String,
    rpm: u64,
}

pub struct FansPanel {
    fans: Vec<Fan>,
}

impl FansPanel {
    pub fn new() -> Self {
        Self { fans: Vec::new() }
    }
}

fn read_fans() -> Vec<Fan> {
    let mut fans = Vec::new();
    let base = PathBuf::from("/sys/class/hwmon");
    let Ok(entries) = fs::read_dir(&base) else {
        return fans;
    };
    for e in entries.flatten() {
        let dir = e.path();
        let chip = fs::read_to_string(dir.join("name"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        // Look for fan1_input, fan2_input, ...
        let Ok(files) = fs::read_dir(&dir) else { continue };
        let mut names: Vec<String> = files
            .flatten()
            .filter_map(|f| {
                let n = f.file_name().to_string_lossy().into_owned();
                if n.starts_with("fan") && n.ends_with("_input") {
                    Some(n)
                } else {
                    None
                }
            })
            .collect();
        names.sort();
        for n in names {
            let path = dir.join(&n);
            let rpm: Option<u64> = fs::read_to_string(&path)
                .ok()
                .and_then(|s| s.trim().parse().ok());
            // Skip sensors that report nothing or a parse failure.
            let Some(rpm) = rpm else { continue };
            let idx = n
                .trim_start_matches("fan")
                .trim_end_matches("_input")
                .to_string();
            let label = if chip.is_empty() {
                format!("fan{idx}")
            } else {
                format!("{chip} fan{idx}")
            };
            fans.push(Fan { label, rpm });
        }
    }
    fans
}

impl Panel for FansPanel {
    fn name(&self) -> &str {
        "fans"
    }

    fn refresh_ms(&self) -> u64 {
        2_000
    }

    fn tick(&mut self) {
        self.fans = read_fans();
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(3)])
            .split(area);

        let title = Line::from(vec![
            Span::styled(" fans ", theme::pane_header()),
            Span::styled("/sys/class/hwmon", theme::dim()),
        ]);
        f.render_widget(ratatui::widgets::Paragraph::new(title), chunks[0]);

        let body = chunks[1];
        let active: Vec<&Fan> = self.fans.iter().filter(|f| f.rpm > 0).collect();

        if active.is_empty() {
            // Either no fan sensors, or they report 0 (idle / not spun up).
            let msg = if self.fans.is_empty() {
                "no fan sensors exposed on this machine"
            } else {
                "fan sensors present but reporting 0 rpm (idle or unsupported)"
            };
            f.render_widget(crate::widgets::empty(msg), body);
            return;
        }

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(2); active.len()])
            .split(body);
        for (i, fan) in active.iter().enumerate() {
            if i >= rows.len() {
                break;
            }
            let pct = ((fan.rpm * 100) / NOMINAL_MAX_RPM).min(100) as u16;
            let style = if pct >= 85 {
                theme::alert()
            } else if pct >= 50 {
                Style::default().fg(theme::pink())
            } else {
                Style::default().fg(theme::lavender())
            };
            let g = Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::NONE)
                        .title(Line::from(vec![
                            Span::styled(format!(" {} ", fan.label), theme::pane_header()),
                            Span::styled(format!("{} rpm", fan.rpm), style),
                        ])),
                )
                .gauge_style(style)
                .percent(pct);
            f.render_widget(g, rows[i]);
        }
    }
}
