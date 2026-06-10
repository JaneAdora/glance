//! NVIDIA GPU stats via nvidia-smi. Utilization + memory + temperature gauges
//! plus a utilization-history sparkline. Graceful absent on non-NVIDIA boxes.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Row, Sparkline, Table};
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
    procs: Vec<(u32, String, u64)>, // (pid, command, vram MiB), top-N desc
}

impl GpuPanel {
    pub fn new() -> Self {
        Self {
            available: false,
            gpu: GpuStat::default(),
            util_hist: VecDeque::with_capacity(HIST),
            procs: Vec::new(),
        }
    }

    /// GPU utilization percent, or None when no NVIDIA GPU is available.
    pub fn util(&self) -> Option<u16> {
        if self.available {
            Some(self.gpu.util)
        } else {
            None
        }
    }

    /// VRAM (used, total) in MiB, or None when unavailable.
    pub fn vram(&self) -> Option<(u64, u64)> {
        if self.available {
            Some((self.gpu.mem_used, self.gpu.mem_total))
        } else {
            None
        }
    }

    /// GPU temperature in celsius, or None when unavailable.
    pub fn temp(&self) -> Option<u16> {
        if self.available {
            Some(self.gpu.temp)
        } else {
            None
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

/// Parse `nvidia-smi`'s text process table into (pid, vram MiB) pairs, summed
/// per pid and sorted by VRAM descending. Skips header/border/separator lines
/// and the "No running processes found" case. Robust to MIG `N/A` GI/CI columns
/// and multi-word / `...`-truncated names: the Type token (`C`, `G`, or `C+G`)
/// anchors the row, the PID is the token before it, and the trailing `<n>MiB`
/// token is the memory. We parse the full text table (not the CSV
/// `--query-compute-apps`) because the CSV omits graphics contexts, which on a
/// desktop are the biggest VRAM consumers (compositor, browser).
fn parse_gpu_procs(text: &str) -> Vec<(u32, u64)> {
    use std::collections::BTreeMap;
    let mut by_pid: BTreeMap<u32, u64> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let tokens: Vec<&str> = line.trim_matches('|').split_whitespace().collect();
        let Some(type_idx) = tokens.iter().position(|t| matches!(*t, "C" | "G" | "C+G")) else {
            continue;
        };
        if type_idx == 0 {
            continue;
        }
        let Ok(pid) = tokens[type_idx - 1].parse::<u32>() else {
            continue;
        };
        let Some(mib_str) = tokens.last().and_then(|t| t.strip_suffix("MiB")) else {
            continue;
        };
        let Ok(mib) = mib_str.parse::<u64>() else {
            continue;
        };
        *by_pid.entry(pid).or_insert(0) += mib;
    }
    let mut v: Vec<(u32, u64)> = by_pid.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// Clean command name for a pid from /proc/<pid>/comm (e.g. `chrome` instead of
/// the truncated `...rack-uuid=...` shown in nvidia-smi).
fn proc_comm(pid: u32) -> Option<String> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Top-5 processes by VRAM via nvidia-smi's text process table, with clean names.
fn query_gpu_procs() -> Vec<(u32, String, u64)> {
    let Ok(out) = Command::new("nvidia-smi").output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_gpu_procs(&text)
        .into_iter()
        .take(5)
        .map(|(pid, mib)| (pid, proc_comm(pid).unwrap_or_else(|| "?".to_string()), mib))
        .collect()
}

/// VRAM MiB as a compact column value: `1.6G` above 1024 MiB, else `672M`.
fn fmt_mib(mib: u64) -> String {
    if mib >= 1024 {
        format!("{:.1}G", mib as f64 / 1024.0)
    } else {
        format!("{mib}M")
    }
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
                self.procs = query_gpu_procs();
            }
            None => {
                self.available = false;
                self.procs.clear();
            }
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        // Show the per-process VRAM table only when the pane is tall enough
        // (fixed 7 + min history 3 + table 7). Collapses on short/phone panes.
        let show_procs = self.available && !self.procs.is_empty() && area.height >= 17;
        let mut constraints = vec![
            Constraint::Length(1), // title
            Constraint::Length(2), // util gauge
            Constraint::Length(2), // mem gauge
            Constraint::Length(2), // temp gauge
            Constraint::Min(3),    // util history
        ];
        if show_procs {
            constraints.push(Constraint::Length(7)); // Top processes (VRAM)
        }
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
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

        if show_procs {
            if let Some(proc_area) = chunks.get(5).copied() {
                let table = Table::new(
                    self.procs.iter().map(|(pid, name, mib)| {
                        Row::new(vec![
                            Cell::from(format!("{:>6}", fmt_mib(*mib))).style(theme::now()),
                            Cell::from(format!("{pid:>6}")).style(theme::dim()),
                            Cell::from(name.clone()),
                        ])
                    }),
                    [Constraint::Length(8), Constraint::Length(7), Constraint::Min(8)],
                )
                .header(Row::new(vec!["VRAM", "PID", "Command"]).style(theme::pane_header()))
                .block(Block::default().borders(Borders::NONE).title(Line::from(Span::styled(
                    " Top processes ",
                    theme::pane_header(),
                ))));
                f.render_widget(table, proc_area);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gpu_procs_reads_text_table_and_sorts_desc() {
        let text = "\
+-----------------------------------------------------------------------------------------+
| Processes:                                                                              |
|  GPU   GI   CI              PID   Type   Process name                        GPU Memory |
|        ID   ID                                                               Usage      |
|=========================================================================================|
|    0   N/A  N/A         1002188      G   cosmic-comp                             672MiB |
|    0   N/A  N/A         1002241      G   Xwayland                                  6MiB |
|    0   N/A  N/A         1002321    C+G   cosmic-workspaces                        20MiB |
|    0   N/A  N/A         1003758      G   ...rack-uuid=3190708988185955192       1664MiB |
|    0   N/A  N/A         1004268      G   /usr/bin/ghostty                        189MiB |
+-----------------------------------------------------------------------------------------+";
        let procs = parse_gpu_procs(text);
        assert_eq!(procs.len(), 5, "five process rows, header/border skipped");
        assert_eq!(procs[0], (1003758, 1664), "sorted by VRAM desc");
        assert_eq!(procs[1], (1002188, 672));
        assert_eq!(procs[2], (1004268, 189));
        assert_eq!(procs[4], (1002241, 6));
    }

    #[test]
    fn parse_gpu_procs_empty_when_no_processes() {
        let text = "\
|=========================================================================================|
|  No running processes found                                                             |
+-----------------------------------------------------------------------------------------+";
        assert!(parse_gpu_procs(text).is_empty());
    }

    #[test]
    fn fmt_mib_compact() {
        assert_eq!(fmt_mib(672), "672M");
        assert_eq!(fmt_mib(1664), "1.6G");
    }

    #[test]
    fn accessors_are_none_before_tick() {
        let p = GpuPanel::new();
        assert_eq!(p.util(), None);
        assert_eq!(p.vram(), None);
        assert_eq!(p.temp(), None);
    }
}
