use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Row, Sparkline, Table};
use ratatui::Frame;
use std::collections::VecDeque;
use sysinfo::{MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

const HIST: usize = 60;

pub struct MemPanel {
    sys: System,
    history: VecDeque<u64>,
}

impl MemPanel {
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::new()
                .with_memory(MemoryRefreshKind::everything())
                .with_processes(ProcessRefreshKind::everything()),
        );
        Self {
            sys,
            history: VecDeque::with_capacity(HIST),
        }
    }

    pub fn used_pct(&self) -> u16 {
        let total = self.sys.total_memory().max(1);
        let used = self.sys.used_memory();
        ((used * 100) / total) as u16
    }

    fn swap_pct(&self) -> u16 {
        let total = self.sys.total_swap().max(1);
        let used = self.sys.used_swap();
        ((used * 100) / total) as u16
    }
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

impl Panel for MemPanel {
    fn name(&self) -> &str {
        "mem"
    }

    fn refresh_ms(&self) -> u64 {
        500
    }

    fn tick(&mut self) {
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        if self.history.len() == HIST {
            self.history.pop_front();
        }
        self.history.push_back(self.used_pct() as u64);
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        // Show the per-process RAM table only when the pane is tall enough
        // (gauges 6 + min history 3 + table 7). Collapses on short/phone panes.
        let show_procs = area.height >= 16;
        let mut constraints = vec![
            Constraint::Length(3), // RAM gauge
            Constraint::Length(3), // Swap gauge
            Constraint::Min(3),    // RAM history
        ];
        if show_procs {
            constraints.push(Constraint::Length(7)); // Top processes (RAM)
        }
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        let used = self.sys.used_memory();
        let total = self.sys.total_memory();
        let pct = self.used_pct();
        let style = if pct >= 90 {
            theme::alert()
        } else if pct >= 70 {
            theme::now()
        } else {
            theme::historical()
        };
        let ram = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::NONE)
                    .title(Line::from(vec![
                        Span::styled(" RAM ", theme::pane_header()),
                        Span::styled(format!("{} / {}", human(used), human(total)), theme::dim()),
                    ])),
            )
            .gauge_style(style)
            .percent(pct);
        f.render_widget(ram, chunks[0]);

        let swap_used = self.sys.used_swap();
        let swap_total = self.sys.total_swap();
        let swap_pct = self.swap_pct();
        let swap_style = if swap_pct >= 50 {
            theme::alert()
        } else if swap_pct >= 10 {
            theme::now()
        } else {
            theme::historical()
        };
        let swap = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::NONE)
                    .title(Line::from(vec![
                        Span::styled(" Swap ", theme::pane_header()),
                        Span::styled(
                            format!("{} / {}", human(swap_used), human(swap_total)),
                            theme::dim(),
                        ),
                    ])),
            )
            .gauge_style(swap_style)
            .percent(swap_pct.min(100));
        f.render_widget(swap, chunks[1]);

        let data: Vec<u64> = self.history.iter().copied().collect();
        let spark = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(theme::dim())
                    .title(Line::from(Span::styled(" RAM history (5 min) ", theme::pane_header()))),
            )
            .data(&data)
            .max(100)
            .style(theme::historical());
        f.render_widget(spark, chunks[2]);

        if show_procs {
            if let Some(proc_area) = chunks.get(3).copied() {
                let mut procs: Vec<_> = self
                    .sys
                    .processes()
                    .values()
                    .map(|p| (p.memory(), p.name().to_string_lossy().into_owned(), p.pid().as_u32()))
                    .collect();
                procs.sort_by(|a, b| b.0.cmp(&a.0));
                procs.truncate(5);
                let table = Table::new(
                    procs.into_iter().map(|(rss, name, pid)| {
                        Row::new(vec![
                            Cell::from(format!("{:>6}", human(rss))).style(theme::now()),
                            Cell::from(format!("{pid:>6}")).style(theme::dim()),
                            Cell::from(name),
                        ])
                    }),
                    [Constraint::Length(8), Constraint::Length(7), Constraint::Min(8)],
                )
                .header(Row::new(vec!["RSS", "PID", "Command"]).style(theme::pane_header()))
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
    fn used_pct_is_a_percentage() {
        // new() refreshes memory, so total_memory > 0 and the ratio is in range.
        assert!(MemPanel::new().used_pct() <= 100);
    }
}
