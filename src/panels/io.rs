//! Per-disk I/O throughput from /proc/diskstats. Read + write sparklines per
//! physical disk (partitions and virtual devices filtered out).
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
const SECTOR_BYTES: u64 = 512;

struct DiskHist {
    read: VecDeque<u64>,  // bytes/sec
    write: VecDeque<u64>, // bytes/sec
    last_read_bytes: u64,
    last_write_bytes: u64,
}

pub struct IoPanel {
    disks: HashMap<String, DiskHist>,
    order: Vec<String>,
    last_tick: Option<Instant>,
}

impl IoPanel {
    pub fn new() -> Self {
        Self {
            disks: HashMap::new(),
            order: Vec::new(),
            last_tick: None,
        }
    }
}

fn is_whole_disk(name: &str) -> bool {
    // sda, nvme0n1, vda, mmcblk0, xvda — exclude partitions + loop/ram/dm.
    let bytes = name.as_bytes();
    if name.starts_with("sd") || name.starts_with("vd") || name.starts_with("xvd") || name.starts_with("hd") {
        // sdX (no trailing digits)
        !bytes.last().map(|b| b.is_ascii_digit()).unwrap_or(false)
    } else if name.starts_with("nvme") {
        // nvme0n1 yes, nvme0n1p1 no
        !name.contains('p')
    } else if name.starts_with("mmcblk") {
        !name.contains('p')
    } else {
        false
    }
}

fn read_diskstats() -> Vec<(String, u64, u64)> {
    let s = match fs::read_to_string("/proc/diskstats") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in s.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 10 {
            continue;
        }
        let name = f[2];
        if !is_whole_disk(name) {
            continue;
        }
        let sectors_read: u64 = f[5].parse().unwrap_or(0);
        let sectors_written: u64 = f[9].parse().unwrap_or(0);
        out.push((
            name.to_string(),
            sectors_read * SECTOR_BYTES,
            sectors_written * SECTOR_BYTES,
        ));
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

impl Panel for IoPanel {
    fn name(&self) -> &str {
        "io"
    }

    fn refresh_ms(&self) -> u64 {
        1_000
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = self
            .last_tick
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);
        for (name, read_bytes, write_bytes) in read_diskstats() {
            let d = self.disks.entry(name.clone()).or_insert_with(|| DiskHist {
                read: VecDeque::with_capacity(HIST),
                write: VecDeque::with_capacity(HIST),
                last_read_bytes: read_bytes,
                last_write_bytes: write_bytes,
            });
            if !self.order.contains(&name) {
                self.order.push(name.clone());
            }
            if dt > 0.0 {
                let r = ((read_bytes.saturating_sub(d.last_read_bytes)) as f64 / dt) as u64;
                let w = ((write_bytes.saturating_sub(d.last_write_bytes)) as f64 / dt) as u64;
                if d.read.len() == HIST {
                    d.read.pop_front();
                }
                if d.write.len() == HIST {
                    d.write.pop_front();
                }
                d.read.push_back(r);
                d.write.push_back(w);
            }
            d.last_read_bytes = read_bytes;
            d.last_write_bytes = write_bytes;
        }
        self.last_tick = Some(now);
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        if self.order.is_empty() {
            f.render_widget(crate::widgets::empty("no disks in /proc/diskstats"), area);
            return;
        }
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(self.order.iter().map(|_| Constraint::Length(3)).collect::<Vec<_>>())
            .split(area);
        for (i, name) in self.order.iter().enumerate() {
            if i >= rows.len() {
                break;
            }
            let d = match self.disks.get(name) {
                Some(d) => d,
                None => continue,
            };
            let last_r = d.read.back().copied().unwrap_or(0);
            let last_w = d.write.back().copied().unwrap_or(0);
            let block = Block::default()
                .borders(Borders::TOP)
                .border_style(theme::dim())
                .title(Line::from(vec![
                    Span::styled(format!(" {name} "), theme::pane_header()),
                    Span::styled(format!("r {} ", human_rate(last_r)), theme::now()),
                    Span::styled(format!("w {}", human_rate(last_w)), theme::historical()),
                ]));
            f.render_widget(block, rows[i]);
            let inner = Rect {
                x: rows[i].x,
                y: rows[i].y + 1,
                width: rows[i].width,
                height: rows[i].height.saturating_sub(1),
            };
            if inner.height >= 2 {
                let halves = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Length(1)])
                    .split(inner);
                let rd: Vec<u64> = d.read.iter().copied().collect();
                let wd: Vec<u64> = d.write.iter().copied().collect();
                let max = rd.iter().chain(wd.iter()).copied().max().unwrap_or(1).max(1024);
                f.render_widget(
                    Sparkline::default().data(&rd).max(max).style(theme::now()),
                    halves[0],
                );
                f.render_widget(
                    Sparkline::default().data(&wd).max(max).style(theme::historical()),
                    halves[1],
                );
            }
        }
    }
}
