use crate::theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, transient: Option<&str>) {
    let hint = Line::from(vec![
        Span::styled("1-9", theme::pane_header_focused()),
        Span::raw(" panel  "),
        Span::styled("n/p", theme::pane_header_focused()),
        Span::raw(" cycle  "),
        Span::styled("r", theme::pane_header_focused()),
        Span::raw(" refresh  "),
        Span::styled("?", theme::pane_header_focused()),
        Span::raw(" help  "),
        Span::styled("q", theme::pane_header_focused()),
        Span::raw(" quit"),
    ]);
    let mut lines = vec![hint];
    if let Some(msg) = transient {
        lines.push(Line::from(Span::styled(msg.to_string(), theme::status())));
    }
    f.render_widget(Paragraph::new(lines), area);
}
