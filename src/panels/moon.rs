use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use crate::layout::braille_aspect_bounds;
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::f64::consts::PI;
use std::time::{SystemTime, UNIX_EPOCH};

const SYNODIC_DAYS: f64 = 29.530588;
// Reference new moon: 2000-01-06 18:14 UTC = Julian Date 2451550.26
const REF_NEW_JD: f64 = 2451550.26;
// Unix epoch (1970-01-01 00:00 UTC) in Julian Date
const UNIX_EPOCH_JD: f64 = 2440587.5;

pub struct MoonPanel {
    phase: f64,        // 0.0 (new) .. 1.0 (back to new)
    illumination: f64, // 0.0 .. 1.0
    age_days: f64,
    name: &'static str,
    glyph: &'static str,
}

impl MoonPanel {
    pub fn new() -> Self {
        let mut p = Self {
            phase: 0.0,
            illumination: 0.0,
            age_days: 0.0,
            name: "New",
            glyph: "🌑",
        };
        p.tick();
        p
    }
}

fn compute_phase() -> f64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let jd_now = UNIX_EPOCH_JD + secs / 86400.0;
    ((jd_now - REF_NEW_JD).rem_euclid(SYNODIC_DAYS)) / SYNODIC_DAYS
}

fn phase_to_name_glyph(phase: f64) -> (&'static str, &'static str) {
    // Eight buckets, each 1/8 of the cycle, centered on the named phase
    // (so e.g. "New" is phase in [-1/16, 1/16) wrapped).
    let bucket = ((phase * 8.0 + 0.5).floor() as i64).rem_euclid(8);
    match bucket {
        0 => ("New", "🌑"),
        1 => ("Waxing Crescent", "🌒"),
        2 => ("First Quarter", "🌓"),
        3 => ("Waxing Gibbous", "🌔"),
        4 => ("Full", "🌕"),
        5 => ("Waning Gibbous", "🌖"),
        6 => ("Last Quarter", "🌗"),
        _ => ("Waning Crescent", "🌘"),
    }
}

impl Panel for MoonPanel {
    fn name(&self) -> &str {
        "moon"
    }

    fn refresh_ms(&self) -> u64 {
        300_000 // 5 minutes
    }

    fn tick(&mut self) {
        self.phase = compute_phase();
        self.illumination = (1.0 - (2.0 * PI * self.phase).cos()) / 2.0;
        self.age_days = self.phase * SYNODIC_DAYS;
        let (n, g) = phase_to_name_glyph(self.phase);
        self.name = n;
        self.glyph = g;
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        // Narrow fallback: ultra-compact glyph view.
        if area.width < 40 || area.height < 8 {
            let lines = vec![
                Line::from(Span::styled(
                    format!("  {}  ", self.glyph),
                    theme::pane_header_focused(),
                )),
                Line::from(Span::styled(self.name.to_string(), theme::pane_header())),
                Line::from(Span::styled(
                    format!("{:.0}% illuminated", self.illumination * 100.0),
                    theme::dim(),
                )),
                Line::from(Span::styled(
                    format!("age {:.1} d", self.age_days),
                    theme::dim(),
                )),
            ];
            f.render_widget(Paragraph::new(lines), area);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(65), Constraint::Min(4)])
            .split(area);

        let phase = self.phase;
        let waxing = phase < 0.5;
        let cos_psi = (2.0 * PI * phase).cos();

        // Build lit/shadow point grids. Disc radius 1.0; bounds slightly larger.
        let steps_per_axis = 80; // dense grid; Braille marker rasterizes finely
        let mut lit: Vec<(f64, f64)> = Vec::with_capacity(steps_per_axis * steps_per_axis / 2);
        let mut shadow: Vec<(f64, f64)> = Vec::with_capacity(steps_per_axis * steps_per_axis / 2);
        for iy in 0..steps_per_axis {
            for ix in 0..steps_per_axis {
                let x = -1.0 + 2.0 * (ix as f64 + 0.5) / steps_per_axis as f64;
                let y = -1.0 + 2.0 * (iy as f64 + 0.5) / steps_per_axis as f64;
                if x * x + y * y > 1.0 {
                    continue;
                }
                let hw = (1.0 - y * y).sqrt();
                let x_t = cos_psi * hw;
                let is_lit = if waxing { x > x_t } else { x < x_t };
                if is_lit {
                    lit.push((x, y));
                } else {
                    shadow.push((x, y));
                }
            }
        }

        let canvas_block = Block::default()
            .borders(Borders::NONE)
            .title(Line::from(Span::styled(
                format!(" {} {} ", self.glyph, self.name),
                theme::pane_header_focused(),
            )));
        let inner = canvas_block.inner(chunks[0]);
        f.render_widget(canvas_block, chunks[0]);

        let (xb, yb) = braille_aspect_bounds(inner, 1.15, 1.15);
        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds(xb)
            .y_bounds(yb)
            .paint(move |ctx| {
                // Shadow first so lit overlays cleanly at the terminator.
                ctx.draw(&Points {
                    coords: &shadow,
                    color: theme::lavender(), // LAVENDER
                });
                ctx.draw(&Points {
                    coords: &lit,
                    color: theme::pink(), // PINK
                });
            });
        f.render_widget(canvas, inner);

        // Stats block below.
        let next_in = next_phase_in_days(self.phase);
        let stats = vec![
            Line::from(vec![
                Span::styled(format!("  {} ", self.glyph), theme::pane_header_focused()),
                Span::styled(format!("{}", self.name), theme::pane_header()),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("  {:.0}% lit", self.illumination * 100.0),
                    Style::default().fg(theme::pink()),
                ),
                Span::styled("    age ", theme::dim()),
                Span::styled(
                    format!("{:.1} d", self.age_days),
                    theme::historical(),
                ),
                Span::styled("    next ", theme::dim()),
                Span::styled(
                    format!("{} in {:.1} d", next_in.0, next_in.1),
                    theme::historical(),
                ),
            ]),
        ];
        f.render_widget(Paragraph::new(stats), chunks[1]);
    }
}

/// Returns (name-of-next-cardinal-phase, days-until-it).
fn next_phase_in_days(phase: f64) -> (&'static str, f64) {
    // Cardinal phases at 0 (New), 0.25 (First Quarter), 0.5 (Full), 0.75 (Last Quarter).
    let cardinals: &[(f64, &str)] = &[
        (0.0, "New"),
        (0.25, "First Qtr"),
        (0.5, "Full"),
        (0.75, "Last Qtr"),
        (1.0, "New"),
    ];
    for (cp, name) in cardinals {
        if *cp > phase {
            let days = (cp - phase) * SYNODIC_DAYS;
            return (name, days);
        }
    }
    ("New", (1.0 - phase) * SYNODIC_DAYS)
}
