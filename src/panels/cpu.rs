use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Row, Sparkline, Table};
use ratatui::Frame;
use std::collections::VecDeque;
use sysinfo::{CpuRefreshKind, ProcessRefreshKind, RefreshKind, System};

const HIST: usize = 60;

pub struct CpuPanel {
    sys: System,
    cores: Vec<VecDeque<u64>>,
}

impl CpuPanel {
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::everything())
                .with_processes(ProcessRefreshKind::everything()),
        );
        let cores = (0..sys.cpus().len()).map(|_| VecDeque::with_capacity(HIST)).collect();
        Self { sys, cores }
    }

    /// Overall CPU usage: the mean of the latest per-core sample. Returns 0
    /// before the first tick (no history yet).
    pub fn overall_pct(&self) -> u16 {
        if self.cores.is_empty() {
            return 0;
        }
        let sum: u64 = self.cores.iter().map(|c| c.back().copied().unwrap_or(0)).sum();
        (sum / self.cores.len() as u64) as u16
    }

    /// Number of logical CPU cores tracked. Used by the vitals cockpit to size
    /// the CPU panel's row in the scrollable column so every core shows.
    pub fn core_count(&self) -> usize {
        self.cores.len()
    }
}

impl Panel for CpuPanel {
    fn name(&self) -> &str {
        "cpu"
    }

    fn refresh_ms(&self) -> u64 {
        500
    }

    fn tick(&mut self) {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        for (i, cpu) in self.sys.cpus().iter().enumerate() {
            if self.cores[i].len() == HIST {
                self.cores[i].pop_front();
            }
            self.cores[i].push_back(cpu.cpu_usage().round().clamp(0.0, 100.0) as u64);
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let n = self.cores.len().max(1);
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(n as u16 + 2), Constraint::Min(3)])
            .split(area);

        let core_area = split[0];
        let inner_block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(theme::dim())
            .title(Line::from(Span::styled(" CPU per core ", theme::pane_header())));
        f.render_widget(inner_block, core_area);

        let inner = Rect {
            x: core_area.x,
            y: core_area.y + 1,
            width: core_area.width,
            height: core_area.height.saturating_sub(2),
        };

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(1); n])
            .split(inner);

        for (i, row) in rows.iter().enumerate() {
            let label_width = 7u16;
            let data: Vec<u64> = self.cores[i].iter().copied().collect();
            let last = data.last().copied().unwrap_or(0);
            let style = if last >= 85 {
                theme::alert()
            } else if last >= 50 {
                theme::now()
            } else {
                theme::historical()
            };

            let label_area = Rect {
                x: row.x,
                y: row.y,
                width: label_width.min(row.width),
                height: 1,
            };
            f.render_widget(
                ratatui::widgets::Paragraph::new(Line::from(vec![
                    Span::styled(format!("c{:>2} ", i), theme::dim()),
                    Span::styled(format!("{:>2}%", last), style),
                ])),
                label_area,
            );

            if row.width > label_width {
                let spark_area = Rect {
                    x: row.x + label_width,
                    y: row.y,
                    width: row.width - label_width,
                    height: 1,
                };
                let sl = Sparkline::default()
                    .data(&data)
                    .max(100)
                    .style(style);
                f.render_widget(sl, spark_area);
            }
        }

        let mut procs: Vec<_> = self
            .sys
            .processes()
            .values()
            .map(|p| (p.cpu_usage(), p.name().to_string_lossy().into_owned(), p.pid().as_u32()))
            .collect();
        procs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        procs.truncate(5);

        let table = Table::new(
            procs.into_iter().map(|(pct, name, pid)| {
                let pct_style = if pct >= 50.0 { theme::now() } else { theme::historical() };
                Row::new(vec![
                    Cell::from(format!("{:>5.1}%", pct)).style(pct_style),
                    Cell::from(format!("{:>6}", pid)).style(theme::dim()),
                    Cell::from(name),
                ])
            }),
            [Constraint::Length(7), Constraint::Length(7), Constraint::Min(8)],
        )
        .header(
            Row::new(vec!["CPU%", "PID", "Command"])
                .style(theme::pane_header()),
        )
        .block(
            Block::default()
                .borders(Borders::NONE)
                .title(Line::from(Span::styled(" Top processes ", theme::pane_header()))),
        );
        f.render_widget(table, split[1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overall_pct_is_zero_before_tick() {
        assert_eq!(CpuPanel::new().overall_pct(), 0);
    }
}
