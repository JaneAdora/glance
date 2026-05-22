//! Shared color palette for glance. All RGB accessors go through
//! `brightness::scale` so the global `[` / `]` shortcut applies uniformly.
use crate::brightness;
use ratatui::style::{Color, Modifier, Style};

use std::sync::OnceLock;

struct Palette {
    pink: Color,
    lavender: Color,
    magenta: Color,
}
static PALETTE: OnceLock<Palette> = OnceLock::new();

/// Core palette, overridable via ~/.config/dashboard-suite/theme.toml.
fn palette() -> &'static Palette {
    PALETTE.get_or_init(|| {
        let mut p = Palette {
            pink: Color::Rgb(0xe8, 0x8b, 0x9f),
            lavender: Color::Rgb(0xc5, 0xa3, 0xff),
            magenta: Color::Rgb(0xff, 0x6e, 0xc7),
        };
        if let Some(cfg) = suite_theme_path() {
            if let Ok(s) = std::fs::read_to_string(cfg) {
                for line in s.lines() {
                    let t = line.trim();
                    if t.starts_with('#') {
                        continue;
                    }
                    if let Some((k, v)) = t.split_once('=') {
                        if let Some(c) = parse_hex(v.trim().trim_matches('"')) {
                            match k.trim() {
                                "pink" => p.pink = c,
                                "lavender" => p.lavender = c,
                                "magenta" => p.magenta = c,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        p
    })
}

fn suite_theme_path() -> Option<std::path::PathBuf> {
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        return Some(std::path::PathBuf::from(x).join("dashboard-suite/theme.toml"));
    }
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config/dashboard-suite/theme.toml"))
}

fn parse_hex(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 {
        return None;
    }
    Some(Color::Rgb(
        u8::from_str_radix(&s[0..2], 16).ok()?,
        u8::from_str_radix(&s[2..4], 16).ok()?,
        u8::from_str_radix(&s[4..6], 16).ok()?,
    ))
}

// Raw defining values (Rep Cap palette).
const FACE_RAW: Color = Color::Rgb(0x40, 0x28, 0x60);
const MAP_BORDER_RAW: Color = Color::Rgb(0x60, 0x4f, 0x80);
const SHADOW_RAW: Color = Color::Rgb(0x2a, 0x20, 0x40);
const SAGE_RAW: Color = Color::Rgb(0x9b, 0xe1, 0x95);
const AMBER_RAW: Color = Color::Rgb(0xff, 0xd9, 0x6e);
const DIM_PURPLE_RAW: Color = Color::Rgb(0x80, 0x60, 0xb0);

// Public color accessors — always brightness-scaled.
pub fn pink() -> Color {
    brightness::scale(palette().pink)
}
pub fn lavender() -> Color {
    brightness::scale(palette().lavender)
}
pub fn magenta() -> Color {
    brightness::scale(palette().magenta)
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
