use crate::brightness;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const SEP: &str = "  │  ";

pub fn render(f: &mut Frame, area: Rect, transient: Option<&str>) {
    let bright = brightness::level();
    let bright_marker = if bright >= 130 {
        "●●●"
    } else if bright >= 110 {
        "●●·"
    } else if bright >= 90 {
        "●··"
    } else if bright >= 60 {
        "·●·"
    } else {
        "··●"
    };

    let line = Line::from(vec![
        Span::styled("1-9·0", theme::pane_header_focused()),
        Span::raw(" jump"),
        Span::styled(SEP, theme::dim()),
        Span::styled("n/p", theme::pane_header_focused()),
        Span::raw(" cycle"),
        Span::styled(SEP, theme::dim()),
        Span::styled("r", theme::pane_header_focused()),
        Span::raw(" refresh"),
        Span::styled(SEP, theme::dim()),
        Span::styled("[ ]", theme::pane_header_focused()),
        Span::raw(" "),
        Span::styled(bright_marker, theme::historical()),
        Span::styled(SEP, theme::dim()),
        Span::styled("?", theme::pane_header_focused()),
        Span::raw(" help"),
        Span::styled(SEP, theme::dim()),
        Span::styled("q", theme::pane_header_focused()),
        Span::raw(" quit"),
    ]);

    let mut lines = vec![line];
    if let Some(msg) = transient {
        lines.push(Line::from(Span::styled(msg.to_string(), theme::status())));
    }
    f.render_widget(Paragraph::new(lines), area);
}
