//! Shared color palette for glance. All RGB accessors go through
//! `brightness::scale` so the global `[` / `]` shortcut applies uniformly.
use crate::brightness;
use ratatui::style::{Color, Modifier, Style};

// Raw defining values (Rep Cap palette).
const PINK_RAW: Color = Color::Rgb(0xe8, 0x8b, 0x9f);
const LAVENDER_RAW: Color = Color::Rgb(0xc5, 0xa3, 0xff);
const MAGENTA_RAW: Color = Color::Rgb(0xff, 0x6e, 0xc7);
const FACE_RAW: Color = Color::Rgb(0x40, 0x28, 0x60);
const MAP_BORDER_RAW: Color = Color::Rgb(0x60, 0x4f, 0x80);
const SHADOW_RAW: Color = Color::Rgb(0x2a, 0x20, 0x40);
const SAGE_RAW: Color = Color::Rgb(0x9b, 0xe1, 0x95);
const AMBER_RAW: Color = Color::Rgb(0xff, 0xd9, 0x6e);
const DIM_PURPLE_RAW: Color = Color::Rgb(0x80, 0x60, 0xb0);

// Public color accessors — always brightness-scaled.
pub fn pink() -> Color {
    brightness::scale(PINK_RAW)
}
pub fn lavender() -> Color {
    brightness::scale(LAVENDER_RAW)
}
pub fn magenta() -> Color {
    brightness::scale(MAGENTA_RAW)
}
pub fn face() -> Color {
    brightness::scale(FACE_RAW)
}
pub fn map_border() -> Color {
    brightness::scale(MAP_BORDER_RAW)
}
pub fn shadow() -> Color {
    brightness::scale(SHADOW_RAW)
}
pub fn sage() -> Color {
    brightness::scale(SAGE_RAW)
}
pub fn amber() -> Color {
    brightness::scale(AMBER_RAW)
}
pub fn dim_purple() -> Color {
    brightness::scale(DIM_PURPLE_RAW)
}

pub fn pane_header() -> Style {
    Style::default().fg(lavender()).add_modifier(Modifier::BOLD)
}

pub fn pane_header_focused() -> Style {
    Style::default().fg(magenta()).add_modifier(Modifier::BOLD)
}

pub fn active_row() -> Style {
    Style::default().fg(pink()).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(lavender()).add_modifier(Modifier::DIM)
}

pub fn status() -> Style {
    Style::default().fg(magenta())
}

pub fn now() -> Style {
    Style::default().fg(pink())
}

pub fn historical() -> Style {
    Style::default().fg(lavender())
}

pub fn alert() -> Style {
    Style::default().fg(magenta()).add_modifier(Modifier::BOLD)
}

pub fn tab_inactive() -> Style {
    Style::default().fg(lavender()).add_modifier(Modifier::DIM)
}

pub fn tab_active() -> Style {
    Style::default().fg(magenta()).add_modifier(Modifier::BOLD)
}

#[allow(dead_code)]
pub const FOCUS_MARKER: &str = "▸ ";
