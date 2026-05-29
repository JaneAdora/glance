//! `standup` -- today-scoreboard glance panel.
//!
//! Synthesizes today's git commits, Claude Code sessions, and calendar meetings
//! into a single compact tile. Spec: docs/superpowers/specs/2026-05-28-standup-design.md
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub struct StandupPanel {}

impl StandupPanel {
    pub fn new() -> Self { Self {} }
}

impl Panel for StandupPanel {
    fn name(&self) -> &str { "standup" }
    fn refresh_ms(&self) -> u64 { 60_000 }
    fn tick(&mut self) {}
    fn render(&self, f: &mut Frame, area: Rect) {
        let title = Line::from(Span::styled(" standup ", theme::pane_header()));
        let body = Line::from(Span::styled("loading...", theme::dim()));
        f.render_widget(Paragraph::new(vec![title, body]), area);
    }
}
