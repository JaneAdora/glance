use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Sparkline};
use ratatui::Frame;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::time::Instant;

const HIST: usize = 60;

struct IfStats {
    rx: VecDeque<u64>,
    tx: VecDeque<u64>,
    last_rx_bytes: u64,
    last_tx_bytes: u64,
}

pub struct NetPanel {
    last_tick: Option<Instant>,
    interfaces: HashMap<String, IfStats>,
    order: Vec<String>,
}

impl NetPanel {
    pub fn new() -> Self {
        Self {
            last_tick: None,
            interfaces: HashMap::new(),
            order: Vec::new(),
        }
    }
}

fn read_proc_net_dev() -> Vec<(String, u64, u64)> {
    let s = match fs::read_to_string("/proc/net/dev") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in s.lines().skip(2) {
        let (name_part, rest) = match line.split_once(':') {
            Some(x) => x,
            None => continue,
        };
        let name = name_part.trim().to_string();
        if name == "lo" {
            continue;
        }
        let nums: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        if nums.len() < 9 {
            continue;
        }
        let rx_bytes = nums[0];
        let tx_bytes = nums[8];
        out.push((name, rx_bytes, tx_bytes));
    }
    out
}

fn human_rate(bps: u64) -> String {
    const U: &[&str] = &["B/s", "K/s", "M/s", "G/s"];
    let mut v = bps as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < U.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{:>4}{}", bps, U[0])
    } else if v >= 100.0 {
        format!("{:>4.0}{}", v, U[i])
    } else if v >= 10.0 {
        format!("{:>4.1}{}", v, U[i])
    } else {
        format!("{:>4.2}{}", v, U[i])
    }
}

impl Panel for NetPanel {
    fn name(&self) -> &str {
        "net"
    }

    fn refresh_ms(&self) -> u64 {
        500
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = self
            .last_tick
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);
        let snapshot = read_proc_net_dev();

        for (name, rx_bytes, tx_bytes) in snapshot {
            let stats = self
                .interfaces
                .entry(name.clone())
                .or_insert_with(|| IfStats {
                    rx: VecDeque::with_capacity(HIST),
                    tx: VecDeque::with_capacity(HIST),
                    last_rx_bytes: rx_bytes,
                    last_tx_bytes: tx_bytes,
                });
            if !self.order.contains(&name) {
                self.order.push(name.clone());
            }
            if dt > 0.0 {
                let rx_rate = ((rx_bytes.saturating_sub(stats.last_rx_bytes)) as f64 / dt) as u64;
                let tx_rate = ((tx_bytes.saturating_sub(stats.last_tx_bytes)) as f64 / dt) as u64;
                if stats.rx.len() == HIST {
                    stats.rx.pop_front();
                }
                if stats.tx.len() == HIST {
                    stats.tx.pop_front();
                }
                stats.rx.push_back(rx_rate);
                stats.tx.push_back(tx_rate);
            }
            stats.last_rx_bytes = rx_bytes;
            stats.last_tx_bytes = tx_bytes;
        }
        self.last_tick = Some(now);
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        if self.order.is_empty() {
            f.render_widget(
                ratatui::widgets::Paragraph::new("(no interfaces)").style(theme::dim()),
                area,
            );
            return;
        }
        let per_if = 3u16;
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                self.order
                    .iter()
                    .map(|_| Constraint::Length(per_if))
                    .collect::<Vec<_>>(),
            )
            .split(area);
        for (i, name) in self.order.iter().enumerate() {
            let stats = match self.interfaces.get(name) {
                Some(s) => s,
                None => continue,
            };
            let row = rows[i];
            let last_rx = stats.rx.back().copied().unwrap_or(0);
            let last_tx = stats.tx.back().copied().unwrap_or(0);
            let label = format!(
                " {} ↓ {} ↑ {} ",
                name,
                human_rate(last_rx),
                human_rate(last_tx)
            );
            let header_style = if last_rx > 1024 * 1024 || last_tx > 1024 * 1024 {
                theme::now()
            } else {
                theme::pane_header()
            };
            let block = Block::default()
                .borders(Borders::TOP)
                .border_style(theme::dim())
                .title(Line::from(Span::styled(label, header_style)));
            f.render_widget(block, row);

            let inner = Rect {
                x: row.x,
                y: row.y + 1,
                width: row.width,
                height: row.height.saturating_sub(1),
            };
            if inner.height >= 2 {
                let halves = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Length(1)])
                    .split(inner);
                let rx_data: Vec<u64> = stats.rx.iter().copied().collect();
                let tx_data: Vec<u64> = stats.tx.iter().copied().collect();
                let max_rate = rx_data
                    .iter()
                    .chain(tx_data.iter())
                    .copied()
                    .max()
                    .unwrap_or(1)
                    .max(1024);
                f.render_widget(
                    Sparkline::default()
                        .data(&rx_data)
                        .max(max_rate)
                        .style(theme::now()),
                    halves[0],
                );
                f.render_widget(
                    Sparkline::default()
                        .data(&tx_data)
                        .max(max_rate)
                        .style(theme::historical()),
                    halves[1],
                );
            }
        }
    }
}
