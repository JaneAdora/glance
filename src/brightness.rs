//! Global brightness control. All theme color accessors go through this so
//! `[` and `]` keys can dim or brighten the whole UI uniformly.
//!
//! Level is an integer percentage 10..=150 where 100 means "use the constant
//! as-defined." Below 100 darkens; above 100 lightens (clamped to 255 per
//! channel).
use ratatui::style::Color;
use std::sync::atomic::{AtomicU8, Ordering};

const MIN: u8 = 30;
const MAX: u8 = 150;
const DEFAULT: u8 = 100;
const STEP: u8 = 10;

static LEVEL: AtomicU8 = AtomicU8::new(DEFAULT);

pub fn level() -> u8 {
    LEVEL.load(Ordering::Relaxed)
}

pub fn set(v: u8) {
    LEVEL.store(v.clamp(MIN, MAX), Ordering::Relaxed);
}

pub fn brighten() -> u8 {
    let n = (level() + STEP).min(MAX);
    set(n);
    n
}

pub fn dim() -> u8 {
    let n = level().saturating_sub(STEP).max(MIN);
    set(n);
    n
}

/// Scale an RGB color by the current brightness level. Non-RGB colors pass
/// through untouched.
pub fn scale(c: Color) -> Color {
    match c {
        Color::Rgb(r, g, b) => {
            let l = level() as u32;
            let nr = (r as u32 * l / 100).min(255) as u8;
            let ng = (g as u32 * l / 100).min(255) as u8;
            let nb = (b as u32 * l / 100).min(255) as u8;
            Color::Rgb(nr, ng, nb)
        }
        other => other,
    }
}
