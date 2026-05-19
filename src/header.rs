use crate::panels::Panel;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(
    f: &mut Frame,
    area: Rect,
    panels: &[Box<dyn Panel>],
    current: usize,
    transient: Option<&str>,
) {
    let mut spans: Vec<Span> = vec![
        Span::styled("glance", theme::pane_header_focused()),
        Span::raw("  "),
    ];

    // Slot keys: 1..9 then 0 then "·" for any panels past the digit keyspace.
    let slot_label = |i: usize| -> &'static str {
        match i {
            0 => "1",
            1 => "2",
            2 => "3",
            3 => "4",
            4 => "5",
            5 => "6",
            6 => "7",
            7 => "8",
            8 => "9",
            9 => "0",
            _ => "·",
        }
    };

    for (i, p) in panels.iter().enumerate() {
        let active = i == current;
        let label = slot_label(i);
        let name = p.name();
        if active {
            spans.push(Span::styled(format!("[{label} {name}]"), theme::tab_active()));
        } else {
            spans.push(Span::styled(format!("{label} {name}"), theme::tab_inactive()));
        }
        if i + 1 < panels.len() {
            spans.push(Span::styled("  ", theme::dim()));
        }
    }

    let mut lines = vec![Line::from(spans)];
    if let Some(msg) = transient {
        lines.push(Line::from(Span::styled(msg.to_string(), theme::status())));
    }
    f.render_widget(Paragraph::new(lines), area);
}
