//! Health goals tracker shared core.
pub mod config;
pub mod log_entry;
pub mod migrate;
pub mod store;
pub mod view;

use crate::theme;
use config::{fmt_count, HealthConfig};
use crossterm::event::{KeyCode, KeyEvent};
use log_entry::{LogAction, LogInput};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use std::time::Instant;
use store::HealthStore;
use view::HealthView;

pub struct HealthCore {
    pub config: HealthConfig,
    pub store: HealthStore,
    pub view: HealthView,
    pub log: LogInput,
    pub focus: usize,
    pub today: String,
    toast: Option<(String, Instant)>,
}

impl HealthCore {
    pub fn new() -> Self {
        migrate::run_once();
        let config = config::load_or_seed();
        let store = HealthStore::load(store::data_path());
        Self {
            config,
            store,
            view: HealthView::Today,
            log: LogInput::Idle,
            focus: 0,
            today: store::today_iso(),
            toast: None,
        }
    }

    pub fn is_capturing(&self) -> bool {
        self.log.is_capturing()
    }

    pub fn tick(&mut self) {
        self.store.reload_if_changed();
        let t = store::today_iso();
        if t != self.today {
            self.today = t;
        }
    }

    fn set_toast(&mut self, m: impl Into<String>) {
        self.toast = Some((m.into(), Instant::now()));
    }

    pub fn current_toast(&self) -> Option<&str> {
        self.toast.as_ref().and_then(|(m, t)| {
            if t.elapsed().as_secs() < 3 {
                Some(m.as_str())
            } else {
                None
            }
        })
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.log.is_capturing() {
            let action = self.log.handle(key.code, &self.config.activities);
            if let LogAction::Commit { activity, count } = action {
                if let Some(a) = self.config.activities.get(activity) {
                    let name = a.name.clone();
                    let today = self.today.clone();
                    self.store.append(&name, count, &today);
                    self.set_toast(format!("logged {} {name}", fmt_count(count)));
                }
            }
            return true;
        }
        let n = self.config.activities.len();
        let on_today = self.view == HealthView::Today;
        match key.code {
            KeyCode::Char('v') => {
                self.view = self.view.next();
                self.set_toast(format!("view: {}", self.view.label()));
                true
            }
            KeyCode::Char('L') | KeyCode::Char('=') => {
                if n > 0 {
                    self.log.open();
                }
                true
            }
            KeyCode::Char('j') | KeyCode::Down if on_today => {
                if n > 0 {
                    self.focus = (self.focus + 1) % n;
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up if on_today => {
                if n > 0 {
                    self.focus = if self.focus == 0 { n - 1 } else { self.focus - 1 };
                }
                true
            }
            KeyCode::Char('+') if on_today => {
                self.quick(1.0);
                true
            }
            KeyCode::Char('-') if on_today => {
                self.quick(-1.0);
                true
            }
            _ => false,
        }
    }

    fn quick(&mut self, delta: f64) {
        if let Some(a) = self.config.activities.get(self.focus) {
            let name = a.name.clone();
            let today = self.today.clone();
            self.store.append(&name, delta, &today);
            let sign = if delta >= 0.0 { "+" } else { "" };
            self.set_toast(format!("{name} {sign}{}", fmt_count(delta)));
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let events = &self.store.events;
        match self.view {
            HealthView::Today => {
                view::render_today(f, area, &self.config, events, &self.today, self.focus)
            }
            HealthView::Weekly => view::render_weekly(f, area, &self.config, events, &self.today),
            HealthView::Grid => view::render_grid(f, area, &self.config, events, &self.today),
            HealthView::AllTime => view::render_alltime(f, area, &self.config, events),
        }
        if self.log.is_capturing() {
            self.render_log_modal(f, area);
        }
    }

    fn render_log_modal(&self, f: &mut Frame, area: Rect) {
        let rect = crate::app::centered_rect(area, 50, 40);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::pane_header_focused())
            .title(Line::from(Span::styled(" log ", theme::pane_header_focused())));
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        let mut lines = Vec::new();
        match &self.log {
            LogInput::Pick { sel } => {
                lines.push(Line::from(Span::styled(
                    "pick activity (1-9 / j k, Enter):",
                    theme::dim(),
                )));
                for (i, a) in self.config.activities.iter().enumerate() {
                    let mark = if i == *sel { "▸ " } else { "  " };
                    let st = if i == *sel {
                        theme::active_row()
                    } else {
                        theme::historical()
                    };
                    lines.push(Line::from(Span::styled(
                        format!("{mark}{}. {}", i + 1, a.name),
                        st,
                    )));
                }
            }
            LogInput::Type { activity, buf } => {
                let name = self
                    .config
                    .activities
                    .get(*activity)
                    .map(|a| a.name.as_str())
                    .unwrap_or("?");
                lines.push(Line::from(Span::styled(
                    format!("{name}: type a count, Enter to log, Esc cancel"),
                    theme::dim(),
                )));
                lines.push(Line::from(Span::styled(format!("> {buf}"), theme::active_row())));
            }
            LogInput::Idle => {}
        }
        f.render_widget(Paragraph::new(lines), inner);
    }
}

impl Default for HealthCore {
    fn default() -> Self {
        Self::new()
    }
}
