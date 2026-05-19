use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::Frame;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

pub struct PeonPanel {
    available: bool,
    date: String,
    reps: BTreeMap<String, u64>,
    goals: BTreeMap<String, u64>,
}

impl PeonPanel {
    pub fn new() -> Self {
        Self {
            available: false,
            date: String::new(),
            reps: BTreeMap::new(),
            goals: BTreeMap::new(),
        }
    }
}

fn state_path() -> Option<PathBuf> {
    let p = PathBuf::from("/home/jane/.claude/hooks/peon-ping/.state.json");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

fn config_path() -> Option<PathBuf> {
    let p = PathBuf::from("/home/jane/.claude/hooks/peon-ping/config.json");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

fn read_goals() -> BTreeMap<String, u64> {
    let mut goals = BTreeMap::new();
    let Some(p) = config_path() else { return goals };
    let Ok(s) = fs::read_to_string(&p) else { return goals };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else { return goals };
    if let Some(ex) = v.get("trainer").and_then(|t| t.get("exercises")).and_then(|e| e.as_object()) {
        for (k, val) in ex {
            if let Some(n) = val.as_u64() {
                goals.insert(k.clone(), n);
            }
        }
    }
    if goals.is_empty() {
        goals.insert("pushups".into(), 300);
        goals.insert("squats".into(), 300);
    }
    goals
}

impl Panel for PeonPanel {
    fn name(&self) -> &str {
        "peon"
    }

    fn refresh_ms(&self) -> u64 {
        5_000
    }

    fn tick(&mut self) {
        self.goals = read_goals();
        let Some(p) = state_path() else {
            self.available = false;
            return;
        };
        let Ok(s) = fs::read_to_string(&p) else {
            self.available = false;
            return;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
            self.available = false;
            return;
        };
        self.available = true;
        let trainer = match v.get("trainer") {
            Some(t) => t,
            None => {
                self.reps.clear();
                self.date = String::new();
                return;
            }
        };
        self.date = trainer
            .get("date")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();
        self.reps.clear();
        if let Some(reps) = trainer.get("reps").and_then(|r| r.as_object()) {
            for (k, val) in reps {
                if let Some(n) = val.as_u64() {
                    self.reps.insert(k.clone(), n);
                }
            }
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        if !self.available {
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("(no peon-ping state) ", theme::dim()),
                    Span::styled("first run /peon-ping-log 25 pushups to start", theme::historical()),
                ])),
                area,
            );
            return;
        }

        let exercises: Vec<&str> = self.goals.keys().map(|s| s.as_str()).collect();
        if exercises.is_empty() {
            f.render_widget(crate::widgets::empty("no exercises configured"), area);
            return;
        }

        let mut constraints = vec![Constraint::Length(1)];
        for _ in &exercises {
            constraints.push(Constraint::Length(3));
        }
        constraints.push(Constraint::Min(1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" peon trainer ", theme::pane_header()),
                Span::styled(self.date.clone(), theme::dim()),
            ])),
            chunks[0],
        );

        let mut total_done = 0u64;
        let mut total_goal = 0u64;
        for (i, ex) in exercises.iter().enumerate() {
            let done = self.reps.get(*ex).copied().unwrap_or(0);
            let goal = self.goals.get(*ex).copied().unwrap_or(1).max(1);
            total_done += done;
            total_goal += goal;
            let pct = ((done.saturating_mul(100)) / goal).min(100) as u16;
            let style = if pct >= 100 {
                theme::historical()
            } else if pct >= 50 {
                theme::now()
            } else {
                theme::alert()
            };
            let g = Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::NONE)
                        .title(Line::from(vec![
                            Span::styled(format!(" {} ", ex), theme::pane_header()),
                            Span::styled(format!("{}/{}", done, goal), theme::dim()),
                        ])),
                )
                .gauge_style(style)
                .percent(pct);
            f.render_widget(g, chunks[i + 1]);
        }

        let pct_total = if total_goal > 0 {
            ((total_done.saturating_mul(100)) / total_goal).min(100)
        } else {
            0
        };
        let summary_idx = chunks.len() - 1;
        let summary_style = if pct_total >= 100 {
            theme::now()
        } else {
            theme::dim()
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" total ", theme::pane_header()),
                Span::styled(format!("{}/{}  ({}%)", total_done, total_goal, pct_total), summary_style),
            ])),
            chunks[summary_idx],
        );
    }
}
