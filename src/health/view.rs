//! View renderers for the four health views.
use crate::health::config::{fmt_count, HealthConfig};
use crate::health::store::{self, Event};
use crate::theme;
use crate::widgets;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Sparkline};
use ratatui::Frame;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HealthView {
    Today,
    Weekly,
    Grid,
    AllTime,
}

impl HealthView {
    pub fn next(self) -> Self {
        match self {
            HealthView::Today => HealthView::Weekly,
            HealthView::Weekly => HealthView::Grid,
            HealthView::Grid => HealthView::AllTime,
            HealthView::AllTime => HealthView::Today,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            HealthView::Today => "today",
            HealthView::Weekly => "weekly",
            HealthView::Grid => "30-day",
            HealthView::AllTime => "all-time",
        }
    }
}

fn pct_style(pct: f64) -> Style {
    if pct >= 100.0 {
        Style::default().fg(theme::magenta())
    } else if pct >= 50.0 {
        Style::default().fg(theme::pink())
    } else {
        Style::default().fg(theme::lavender())
    }
}

const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
fn bar_glyph(frac: f64) -> char {
    if frac <= 0.0 {
        return '▁';
    }
    let idx = ((frac * 8.0).ceil() as usize).clamp(1, 8) - 1;
    BARS[idx]
}

fn weekday(date: &str) -> &'static str {
    use jiff::civil::Weekday::*;
    match date.parse::<jiff::civil::Date>().map(|d| d.weekday()) {
        Ok(Monday) => "Mo",
        Ok(Tuesday) => "Tu",
        Ok(Wednesday) => "We",
        Ok(Thursday) => "Th",
        Ok(Friday) => "Fr",
        Ok(Saturday) => "Sa",
        Ok(Sunday) => "Su",
        Err(_) => "??",
    }
}

pub fn render_today(
    f: &mut Frame,
    area: Rect,
    cfg: &HealthConfig,
    events: &[Event],
    today: &str,
    focus: usize,
) {
    let acts = &cfg.activities;
    if acts.is_empty() {
        f.render_widget(widgets::empty("no activities configured"), area);
        return;
    }
    let mut constraints = vec![Constraint::Length(2)];
    for _ in acts {
        constraints.push(Constraint::Length(3));
    }
    constraints.push(Constraint::Min(1));
    let chunks = Layout::vertical(constraints).split(area);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" health · {today} "),
            theme::pane_header_focused(),
        ))),
        chunks[0],
    );

    let mut sum_pct = 0.0_f64;
    for (i, a) in acts.iter().enumerate() {
        let done = store::today_total(events, &a.name, today);
        let goal = a.goal.max(1.0);
        let pct = ((done / goal) * 100.0).clamp(0.0, 100.0);
        sum_pct += pct;
        let marker = if i == focus { "▸ " } else { "  " };
        let name_style = if i == focus {
            theme::active_row()
        } else {
            theme::pane_header()
        };
        let g = Gauge::default()
            .block(Block::default().borders(Borders::NONE).title(Line::from(vec![
                Span::styled(format!("{marker}{} ", a.name), name_style),
                Span::styled(
                    format!("{}/{} {}", fmt_count(done), fmt_count(a.goal), a.unit),
                    theme::dim(),
                ),
            ])))
            .gauge_style(pct_style(pct))
            .ratio((done / goal).clamp(0.0, 1.0));
        f.render_widget(g, chunks[i + 1]);
    }

    let total = sum_pct / acts.len() as f64;
    let streak = store::streak(events, acts, today);
    let last = chunks.len() - 1;
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" total ", theme::pane_header()),
            Span::styled(
                format!("{}%", total.round() as i64),
                if total >= 100.0 { theme::now() } else { theme::dim() },
            ),
            Span::raw("   "),
            Span::styled(format!("streak {streak}d"), theme::historical()),
        ])),
        chunks[last],
    );
}

pub fn render_weekly(f: &mut Frame, area: Rect, cfg: &HealthConfig, events: &[Event], today: &str) {
    let acts = &cfg.activities;
    if acts.is_empty() {
        f.render_widget(widgets::empty("no activities configured"), area);
        return;
    }
    let dates = store::last_n_dates(today, 7);
    let labels: Vec<&'static str> = dates.iter().map(|d| weekday(d)).collect();
    let constraints: Vec<Constraint> = acts.iter().map(|_| Constraint::Length(4)).collect();
    let chunks = Layout::vertical(constraints).split(area);

    for (i, a) in acts.iter().enumerate() {
        let vals = store::series(events, &a.name, &dates);
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(chunks[i]);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{} ", a.name), theme::pane_header()),
                Span::styled(format!("goal {}/day", fmt_count(a.goal)), theme::dim()),
            ])),
            rows[0],
        );
        let max = a
            .goal
            .max(vals.iter().cloned().fold(0.0_f64, f64::max))
            .max(1.0);
        let mut bar_spans = Vec::new();
        for v in &vals {
            let frac = (v / max).clamp(0.0, 1.0);
            let style = if *v >= a.goal {
                Style::default().fg(theme::magenta())
            } else {
                theme::now()
            };
            bar_spans.push(Span::styled(format!("{}  ", bar_glyph(frac)), style));
        }
        f.render_widget(Paragraph::new(Line::from(bar_spans)), rows[1]);
        let lab_spans: Vec<Span> = labels
            .iter()
            .map(|l| Span::styled(format!("{l} "), theme::dim()))
            .collect();
        f.render_widget(Paragraph::new(Line::from(lab_spans)), rows[2]);
    }
}

pub fn render_grid(f: &mut Frame, area: Rect, cfg: &HealthConfig, events: &[Event], today: &str) {
    let acts = &cfg.activities;
    if acts.is_empty() {
        f.render_widget(widgets::empty("no activities configured"), area);
        return;
    }
    let dates = store::last_n_dates(today, 30);
    let constraints: Vec<Constraint> = acts.iter().map(|_| Constraint::Length(2)).collect();
    let chunks = Layout::vertical(constraints).split(area);
    for (i, a) in acts.iter().enumerate() {
        let vals = store::series(events, &a.name, &dates);
        let data: Vec<u64> = vals.iter().map(|v| v.max(0.0) as u64).collect();
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(chunks[i]);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{} ", a.name), theme::pane_header()),
                Span::styled("last 30d", theme::dim()),
            ])),
            rows[0],
        );
        f.render_widget(
            Sparkline::default().data(&data).style(theme::now()),
            rows[1],
        );
    }
}

pub fn render_alltime(f: &mut Frame, area: Rect, cfg: &HealthConfig, events: &[Event]) {
    let acts = &cfg.activities;
    let mut lines = vec![
        Line::from(Span::styled(" all-time ", theme::pane_header_focused())),
        Line::from(""),
    ];
    for a in acts {
        let at = store::all_time(events, &a.name);
        lines.push(Line::from(vec![
            Span::styled(format!("{:<10}", a.name), theme::active_row()),
            Span::styled(format!("total {:<10}", fmt_count(at.total)), theme::historical()),
            Span::styled(format!("best {:<8}", fmt_count(at.best_day)), theme::dim()),
            Span::styled(format!("avg {}/active-day", fmt_count(at.avg)), theme::dim()),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::config::starter;
    use ratatui::buffer::Buffer;

    fn scan(buf: &Buffer) -> String {
        (0..buf.area().height)
            .flat_map(|y| (0..buf.area().width).map(move |x| (x, y)))
            .map(|(x, y)| buf[(x, y)].symbol().to_string())
            .collect()
    }

    #[test]
    fn view_cycle() {
        let v = HealthView::Today;
        assert_eq!(v.next(), HealthView::Weekly);
        assert_eq!(v.next().next().next().next(), HealthView::Today);
    }

    #[test]
    fn today_renders_activity_names() {
        let cfg = starter();
        let events = vec![Event { ts: 0, date: "2026-05-23".into(), activity: "pushups".into(), count: 5.0 }];
        let area = Rect::new(0, 0, 60, 24);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 24)).unwrap();
        term.draw(|f| render_today(f, area, &cfg, &events, "2026-05-23", 0)).unwrap();
        let s = scan(term.backend().buffer());
        assert!(s.contains("pushups"));
        assert!(s.contains("water"));
        assert!(s.contains("streak"));
    }

    #[test]
    fn all_views_render_without_panic() {
        let cfg = starter();
        let events: Vec<Event> = vec![];
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 24)).unwrap();
        let area = Rect::new(0, 0, 60, 24);
        term.draw(|f| render_weekly(f, area, &cfg, &events, "2026-05-23")).unwrap();
        term.draw(|f| render_grid(f, area, &cfg, &events, "2026-05-23")).unwrap();
        term.draw(|f| render_alltime(f, area, &cfg, &events)).unwrap();
    }
}
