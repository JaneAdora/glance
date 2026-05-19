use ratatui::style::{Color, Modifier, Style};

pub const PINK: Color = Color::Rgb(0xe8, 0x8b, 0x9f);
pub const LAVENDER: Color = Color::Rgb(0xc5, 0xa3, 0xff);
pub const MAGENTA: Color = Color::Rgb(0xff, 0x6e, 0xc7);

pub fn pane_header() -> Style {
    Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD)
}

pub fn pane_header_focused() -> Style {
    Style::default().fg(MAGENTA).add_modifier(Modifier::BOLD)
}

pub fn active_row() -> Style {
    Style::default().fg(PINK).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(LAVENDER).add_modifier(Modifier::DIM)
}

pub fn status() -> Style {
    Style::default().fg(MAGENTA)
}

pub fn now() -> Style {
    Style::default().fg(PINK)
}

pub fn historical() -> Style {
    Style::default().fg(LAVENDER)
}

pub fn alert() -> Style {
    Style::default().fg(MAGENTA).add_modifier(Modifier::BOLD)
}

pub const FOCUS_MARKER: &str = "▸ ";
