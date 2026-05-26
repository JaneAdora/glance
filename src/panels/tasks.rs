//! glance panel form of `tasks`. Read+cycle (status) only; create/delete
//! are reserved for the standalone bin. `wants_keys` is false so digit
//! panel-switching is preserved.
use crate::panels::Panel;
use crate::tasks::TasksCore;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub struct TasksPanel {
    core: TasksCore,
}

impl TasksPanel {
    pub fn new() -> Self {
        Self { core: TasksCore::new() }
    }
}

impl Panel for TasksPanel {
    fn name(&self) -> &str { "tasks" }
    fn refresh_ms(&self) -> u64 { 2_000 }
    fn tick(&mut self) { self.core.tick(); }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => { self.core.move_down(); true }
            KeyCode::Char('k') | KeyCode::Up   => { self.core.move_up(); true }
            KeyCode::Char('l') => { self.core.drill_in(); true }
            KeyCode::Char('h') => { self.core.drill_out(); true }
            KeyCode::Char('o') => { self.core.toggle_expand(); true }
            KeyCode::Char(' ') => { let _ = self.core.cycle_status(); true }
            _ => false,
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
        self.core.render(f, chunks[0]);
        let mut foot = vec![
            Span::styled("space", crate::theme::pane_header_focused()),
            Span::raw(" cycle  "),
            Span::styled("h/l", crate::theme::pane_header_focused()),
            Span::raw(" drill  "),
            Span::styled("j/k", crate::theme::pane_header_focused()),
            Span::raw(" move"),
        ];
        if let Some(t) = self.core.current_toast() {
            foot.push(Span::styled(format!("   {t}"), crate::theme::status()));
        }
        f.render_widget(Paragraph::new(Line::from(foot)), chunks[1]);
    }
}
