use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Row, Sparkline, Table};
use ratatui::Frame;
use std::collections::VecDeque;
use sysinfo::{CpuRefreshKind, ProcessRefreshKind, RefreshKind, System};

const HIST: usize = 60;

/// Columns and rows-per-column to pack `n` cores into at most `avail_rows` rows.
/// Columns grow only as needed: when every core fits in `avail_rows`, this is a
/// single column. Guarantees `rows_per_col <= avail_rows` and `cols*rows >= n`,
/// so no core is ever dropped for lack of vertical space.
fn grid_dims(n: usize, avail_rows: usize) -> (usize, usize) {
    let avail = avail_rows.max(1);
    let cols = n.div_ceil(avail).max(1);
    let rows_per_col = n.div_ceil(cols);
    (cols, rows_per_col)
}

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

        // Show the "Top processes" table only when the pane is tall enough to
        // also fit every core one-per-row. Otherwise give the whole pane to the
        // cores and pack them into a compact multi-column grid so none are lost.
        let want_procs = area.height >= n as u16 + 2 + 5;
        let (core_area, proc_area) = if want_procs {
            let s = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(n as u16 + 2), Constraint::Min(3)])
                .split(area);
            (s[0], Some(s[1]))
        } else {
            (area, None)
        };

        let inner_block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(theme::dim())
            .title(Line::from(Span::styled(
                format!(" CPU per core ({n}) "),
                theme::pane_header(),
            )));
        f.render_widget(inner_block, core_area);

        let inner = Rect {
            x: core_area.x,
            y: core_area.y + 1,
            width: core_area.width,
            height: core_area.height.saturating_sub(2),
        };

        let avail_rows = inner.height.max(1) as usize;
        let (cols, rows_per_col) = grid_dims(n, avail_rows);
        let col_w = (inner.width / cols as u16).max(1);
        let label_width = 7u16;

        for i in 0..n {
            let col = (i / rows_per_col) as u16;
            let row = (i % rows_per_col) as u16;
            if row >= inner.height {
                continue;
            }
            let cx = inner.x + col * col_w;
            if cx >= inner.x + inner.width {
                continue;
            }
            let cy = inner.y + row;
            let cell_w = col_w.min(inner.x + inner.width - cx);

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
                x: cx,
                y: cy,
                width: label_width.min(cell_w),
                height: 1,
            };
            f.render_widget(
                ratatui::widgets::Paragraph::new(Line::from(vec![
                    Span::styled(format!("c{:>2} ", i), theme::dim()),
                    Span::styled(format!("{:>2}%", last), style),
                ])),
                label_area,
            );

            // Sparkline only in single-column mode, where there is room for it.
            if cols == 1 && cell_w > label_width {
                let spark_area = Rect {
                    x: cx + label_width,
                    y: cy,
                    width: cell_w - label_width,
                    height: 1,
                };
                let sl = Sparkline::default().data(&data).max(100).style(style);
                f.render_widget(sl, spark_area);
            }
        }

        let Some(proc_area) = proc_area else {
            return;
        };

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
        f.render_widget(table, proc_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overall_pct_is_zero_before_tick() {
        assert_eq!(CpuPanel::new().overall_pct(), 0);
    }

    #[test]
    fn grid_dims_single_column_when_tall_enough() {
        assert_eq!(grid_dims(24, 24), (1, 24));
        assert_eq!(grid_dims(24, 30), (1, 24));
        assert_eq!(grid_dims(1, 5), (1, 1));
    }

    #[test]
    fn grid_dims_packs_into_columns_when_short() {
        // 24 cores in 10 rows -> 3 columns of 8 (8 <= 10).
        assert_eq!(grid_dims(24, 10), (3, 8));
        // 24 cores in 7 rows -> 4 columns of 6 (6 <= 7).
        assert_eq!(grid_dims(24, 7), (4, 6));
    }

    #[test]
    fn grid_dims_never_drops_a_core_or_overflows_rows() {
        for n in 1..=64usize {
            for avail in 1..=40usize {
                let (cols, rows) = grid_dims(n, avail);
                assert!(cols >= 1 && rows >= 1, "n={n} avail={avail}");
                assert!(rows <= avail, "rows {rows} exceed avail {avail} (n={n})");
                assert!(cols * rows >= n, "grid too small n={n} cols={cols} rows={rows}");
            }
        }
    }
}
