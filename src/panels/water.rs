//! Water intake tracker. Local state file with auto-reset at midnight.
//! Keys: `+` log a glass, `-` undo, `0` reset.
use crate::panels::Panel;
use crate::theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::Frame;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct State {
    date: String, // YYYY-MM-DD
    glasses: u32,
}

pub struct WaterPanel {
    state: State,
    goal: u32,
    path: PathBuf,
    last_toast: Option<String>,
}

fn today_iso() -> String {
    let z = jiff::Zoned::now();
    let d = z.date();
    format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day() as u8)
}

fn state_path() -> PathBuf {
    let dir = dirs::data_local_dir()
        .map(|d| d.join("glance"))
        .unwrap_or_else(|| PathBuf::from("/tmp/glance"));
    let _ = fs::create_dir_all(&dir);
    dir.join("water.json")
}

fn load_state(path: &PathBuf) -> State {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_state(path: &PathBuf, state: &State) {
    if let Ok(s) = serde_json::to_string_pretty(state) {
        let _ = fs::write(path, s);
    }
}

impl WaterPanel {
    pub fn new() -> Self {
        let goal = std::env::var("GLANCE_WATER_GOAL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8);
        let path = state_path();
        let mut state = load_state(&path);
        let today = today_iso();
        if state.date != today {
            state = State {
                date: today,
                glasses: 0,
            };
            save_state(&path, &state);
        }
        Self {
            state,
            goal,
            path,
            last_toast: None,
        }
    }

    fn log_glass(&mut self) {
        self.maybe_rollover();
        self.state.glasses = (self.state.glasses + 1).min(99);
        save_state(&self.path, &self.state);
        self.last_toast = Some(format!("logged glass {} of {}", self.state.glasses, self.goal));
    }

    fn undo(&mut self) {
        self.maybe_rollover();
        self.state.glasses = self.state.glasses.saturating_sub(1);
        save_state(&self.path, &self.state);
        self.last_toast = Some("undid last glass".to_string());
    }

    fn reset(&mut self) {
        self.maybe_rollover();
        self.state.glasses = 0;
        save_state(&self.path, &self.state);
        self.last_toast = Some("reset to 0".to_string());
    }

    fn maybe_rollover(&mut self) {
        let today = today_iso();
        if self.state.date != today {
            self.state.date = today;
            self.state.glasses = 0;
            save_state(&self.path, &self.state);
        }
    }
}

impl Panel for WaterPanel {
    fn name(&self) -> &str {
        "water"
    }

    fn refresh_ms(&self) -> u64 {
        60_000 // mostly static, only updates on keypress; tick handles midnight rollover.
    }

    fn tick(&mut self) {
        self.maybe_rollover();
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.log_glass();
                true
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.undo();
                true
            }
            KeyCode::Char('R') => {
                self.reset();
                true
            }
            _ => false,
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // 0: title (TOP)
                Constraint::Min(0),    // 1: flex spacer (centers the block below)
                Constraint::Length(2), // 2: gauge bar (above the text)
                Constraint::Length(1), // 3: gap
                Constraint::Length(2), // 4: big count line
                Constraint::Length(1), // 5: gap
                Constraint::Length(2), // 6: glass row (cups visualized)
                Constraint::Length(1), // 7: gap
                Constraint::Length(1), // 8: hint
                Constraint::Length(1), // 9: toast
                Constraint::Min(0),    // 10: flex spacer (bottom)
            ])
            .split(area);

        let title = Line::from(vec![
            Span::styled(" water ", theme::pane_header()),
            Span::styled(self.state.date.clone(), theme::pane_header_focused()),
            Span::styled(format!("   goal {}/day", self.goal), theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        // Big "N / GOAL" line — bright pink number, lavender denominator
        let big_line = Line::from(vec![
            Span::styled(
                format!("{}", self.state.glasses),
                Style::default()
                    .fg(theme::magenta())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(format!(" / {}", self.goal), theme::historical()),
            Span::styled(" glasses today", theme::dim()),
        ]);
        f.render_widget(
            Paragraph::new(big_line).alignment(Alignment::Center),
            chunks[4],
        );

        // Glass row: render each glass as a cup glyph
        let mut glass_spans: Vec<Span> = vec![Span::raw("  ")];
        for i in 0..self.goal.max(self.state.glasses) {
            let filled = i < self.state.glasses;
            let glyph = if filled { "▮" } else { "▯" };
            let style = if filled {
                Style::default().fg(theme::magenta())
            } else {
                theme::dim()
            };
            glass_spans.push(Span::styled(glyph.to_string(), style));
            glass_spans.push(Span::raw(" "));
        }
        f.render_widget(
            Paragraph::new(Line::from(glass_spans)).alignment(Alignment::Center),
            chunks[6],
        );

        // Gauge
        let pct = ((self.state.glasses as u64 * 100) / self.goal.max(1) as u64).min(100) as u16;
        let style = if pct >= 100 {
            Style::default().fg(theme::magenta())
        } else if pct >= 50 {
            Style::default().fg(theme::pink())
        } else {
            Style::default().fg(theme::lavender())
        };
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::NONE))
            .gauge_style(style)
            .percent(pct);
        f.render_widget(gauge, chunks[2]);

        // Hint
        let hint = Line::from(vec![
            Span::styled("press ", theme::dim()),
            Span::styled("+", theme::pane_header_focused()),
            Span::styled(" log  ", theme::dim()),
            Span::styled("-", theme::pane_header_focused()),
            Span::styled(" undo  ", theme::dim()),
            Span::styled("R", theme::pane_header_focused()),
            Span::styled(" reset", theme::dim()),
        ]);
        f.render_widget(Paragraph::new(hint).alignment(Alignment::Center), chunks[8]);

        // Toast
        if let Some(t) = &self.last_toast {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(t.clone(), theme::status())))
                    .alignment(Alignment::Center),
                chunks[9],
            );
        }
    }
}
