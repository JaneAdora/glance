use crate::theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, panel_name: &str, idx: usize, total: usize, transient: Option<&str>) {
    let title = Line::from(vec![
        Span::styled("glance ", theme::pane_header_focused()),
        Span::styled(format!("[{}/{}] ", idx + 1, total), theme::dim()),
        Span::styled(panel_name.to_string(), theme::pane_header()),
    ]);
    let mut lines = vec![title];
    if let Some(msg) = transient {
        lines.push(Line::from(Span::styled(msg.to_string(), theme::status())));
    }
    f.render_widget(Paragraph::new(lines), area);
}
