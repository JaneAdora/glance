//! glance panel form of cal. Read-only tile with copy actions; no detail
//! modal (that's in the standalone bin).
use crate::cal::CalCore;
use crate::panels::Panel;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub struct CalPanel {
    core: CalCore,
}

impl CalPanel {
    pub fn new() -> Self {
        Self { core: CalCore::new() }
    }
}

impl Panel for CalPanel {
    fn name(&self) -> &str { "cal" }
    fn refresh_ms(&self) -> u64 { 300_000 }
    fn tick(&mut self) { self.core.tick(); }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => { self.core.move_down(); true }
            KeyCode::Char('k') | KeyCode::Up   => { self.core.move_up(); true }
            KeyCode::Char('o') => { self.core.toggle_expand(); true }
            KeyCode::Char(' ') => { let _ = self.core.copy_url(); true }
            KeyCode::Char('c') => { let _ = self.core.copy_detail(); true }
            _ => false,
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
        self.core.render(f, chunks[0]);
        let mut foot = vec![
            Span::styled("space", crate::theme::pane_header_focused()),
            Span::raw(" copy  "),
            Span::styled("o", crate::theme::pane_header_focused()),
            Span::raw(" expand  "),
            Span::styled("j/k", crate::theme::pane_header_focused()),
            Span::raw(" move"),
        ];
        if let Some(t) = self.core.current_toast() {
            foot.push(Span::styled(format!("   {t}"), crate::theme::status()));
        }
        f.render_widget(Paragraph::new(Line::from(foot)), chunks[1]);
    }
}
