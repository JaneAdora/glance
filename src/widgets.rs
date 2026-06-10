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

/// Make an externally-sourced string safe to render in a terminal cell.
/// Process names (`comm` / `argv[0]`) are attacker-controllable and can embed
/// ANSI escape / control bytes; replacing control chars with `.` neutralizes
/// terminal escape-sequence injection while preserving visible width.
pub fn sanitize_label(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { '.' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use super::sanitize_label;

    #[test]
    fn sanitize_label_neutralizes_control_chars() {
        assert_eq!(sanitize_label("chrome"), "chrome");
        // ESC (0x1b) -> '.', so a CSI clear-screen can't reach the terminal.
        assert_eq!(sanitize_label("ev\u{1b}[2Jil"), "ev.[2Jil");
        assert_eq!(sanitize_label("a\tb\nc"), "a.b.c");
        // DEL (0x7f) and a C1 control (0x9b) are stripped too.
        assert_eq!(sanitize_label("\u{7f}\u{9b}"), "..");
    }
}
