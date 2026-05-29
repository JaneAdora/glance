use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge};
use ratatui::Frame;
use std::process::Command;

pub struct DiskPanel {
    mounts: Vec<Mount>,
}

struct Mount {
    target: String,
    used: u64,
    total: u64,
    pct: u16,
}

impl DiskPanel {
    pub fn new() -> Self {
        Self { mounts: Vec::new() }
    }
}

fn parse_df() -> Vec<Mount> {
    let out = Command::new("df")
        .args([
            "-PB1",
            "-x", "tmpfs",
            "-x", "devtmpfs",
            "-x", "squashfs",
            "-x", "overlay",
            "-x", "fuse.gvfsd-fuse",
        ])
        .output();
    let stdout = match out {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&stdout);
    let mut mounts = Vec::new();
    for line in text.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }
        let total: u64 = match parts[1].parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let used: u64 = match parts[2].parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let target = parts[5..].join(" ");
        if total == 0 {
            continue;
        }
        let pct = ((used.saturating_mul(100)) / total).min(100) as u16;
        mounts.push(Mount {
            target,
            used,
            total,
            pct,
        });
    }
    mounts.sort_by(|a, b| b.pct.cmp(&a.pct));
    mounts
}

fn human(bytes: u64) -> String {
    const U: &[&str] = &["B", "K", "M", "G", "T"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < U.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{}{}", bytes, U[0])
    } else if v >= 100.0 {
        format!("{:.0}{}", v, U[i])
    } else if v >= 10.0 {
        format!("{:.1}{}", v, U[i])
    } else {
        format!("{:.2}{}", v, U[i])
    }
}

impl Panel for DiskPanel {
    fn name(&self) -> &str {
        "disk"
    }

    fn refresh_ms(&self) -> u64 {
        5_000
    }

    fn tick(&mut self) {
        self.mounts = parse_df();
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        if self.mounts.is_empty() {
            f.render_widget(crate::widgets::empty("no mounts"), area);
            return;
        }
        let constraints: Vec<Constraint> = self
            .mounts
            .iter()
            .map(|_| Constraint::Length(2))
            .collect();
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        for (i, m) in self.mounts.iter().enumerate() {
            if i >= rows.len() {
                break;
            }
            let style = if m.pct >= 90 {
                theme::alert()
            } else if m.pct >= 70 {
                theme::now()
            } else {
                theme::historical()
            };
            let g = Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::NONE)
                        .title(Line::from(vec![
                            Span::styled(format!(" {} ", m.target), theme::pane_header()),
                            Span::styled(
                                format!("{} / {}", human(m.used), human(m.total)),
                                theme::dim(),
                            ),
                        ])),
                )
                .gauge_style(style)
                .percent(m.pct);
            f.render_widget(g, rows[i]);
        }
    }
}
