//! Shared paragraph widgets for consistent panel state messaging.
use crate::theme;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn empty(msg: &str) -> Paragraph<'static> {
    Paragraph::new(Line::from(Span::styled(
        format!("({})", msg),
        theme::dim(),
    )))
}

pub fn loading(msg: &str) -> Paragraph<'static> {
    Paragraph::new(Line::from(Span::styled(
        format!("{msg}…"),
        theme::dim(),
    )))
}

pub fn error(msg: &str) -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled("⚠ ", theme::alert()),
        Span::styled(msg.to_string(), theme::dim()),
    ]))
}
