//! Pure decoration: a slowly twinkling starfield. No data source. Stars sit at
//! fixed pseudo-random positions and pulse brightness on independent phases.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;
use std::time::Instant;

const STAR_COUNT: usize = 90;

struct Star {
    x: f64,
    y: f64,
    phase: f64, // 0..1 starting phase
    speed: f64, // twinkle speed multiplier
    tier: u8,   // brightness tier → which color
}

pub struct StarfieldPanel {
    started: Instant,
    stars: Vec<Star>,
}

// Tiny deterministic LCG so we don't need the `rand` crate.
struct Lcg(u64);
impl Lcg {
    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

impl StarfieldPanel {
    pub fn new() -> Self {
        let mut rng = Lcg(0x5EED_C0DE_1234_5678);
        let stars = (0..STAR_COUNT)
            .map(|_| Star {
                x: rng.next_f64() * 2.0 - 1.0,
                y: rng.next_f64() * 2.0 - 1.0,
                phase: rng.next_f64(),
                speed: 0.3 + rng.next_f64() * 1.4,
                tier: (rng.next_f64() * 3.0) as u8,
            })
            .collect();
        Self {
            started: Instant::now(),
            stars,
        }
    }
}

impl Panel for StarfieldPanel {
    fn name(&self) -> &str {
        "starfield"
    }

    fn refresh_ms(&self) -> u64 {
        120 // smooth twinkle
    }

    fn tick(&mut self) {}

    fn render(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::NONE)
            .title(Line::from(Span::styled(" starfield ", theme::pane_header())));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let t = self.started.elapsed().as_secs_f64();

        // Partition stars into 3 brightness buckets by their current twinkle value,
        // each drawn in a different palette color so the field shimmers.
        let mut bright: Vec<(f64, f64)> = Vec::new();
        let mut mid: Vec<(f64, f64)> = Vec::new();
        let mut dim: Vec<(f64, f64)> = Vec::new();
        for s in &self.stars {
            let tw = ((t * s.speed + s.phase * std::f64::consts::TAU).sin() + 1.0) / 2.0; // 0..1
            let level = tw * (0.5 + 0.5 * (s.tier as f64 / 2.0));
            if level > 0.66 {
                bright.push((s.x, s.y));
            } else if level > 0.33 {
                mid.push((s.x, s.y));
            } else {
                dim.push((s.x, s.y));
            }
        }

        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([-1.0, 1.0])
            .y_bounds([-1.0, 1.0])
            .paint(move |ctx| {
                ctx.draw(&Points {
                    coords: &dim,
                    color: theme::map_border(),
                });
                ctx.layer();
                ctx.draw(&Points {
                    coords: &mid,
                    color: theme::lavender(),
                });
                ctx.layer();
                ctx.draw(&Points {
                    coords: &bright,
                    color: theme::magenta(),
                });
            });
        f.render_widget(canvas, inner);
    }
}
