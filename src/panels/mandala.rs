//! Pure decoration: a slowly rotating parametric mandala (superimposed rose
//! curves) drawn on a Canvas. No data source. Sibling to `starfield`.
use crate::layout::braille_aspect_bounds;
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::Block;
use ratatui::Frame;
use std::f64::consts::TAU;
use std::time::Instant;

pub struct MandalaPanel {
    started: Instant,
}

impl MandalaPanel {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

/// A rose curve r = cos(k·θ), rotated by `phase`, sampled into points.
fn rose(k: f64, phase: f64, scale: f64, samples: usize) -> Vec<(f64, f64)> {
    let mut pts = Vec::with_capacity(samples);
    for i in 0..samples {
        let theta = (i as f64 / samples as f64) * TAU;
        let r = (k * theta).cos() * scale;
        let a = theta + phase;
        pts.push((r * a.cos(), r * a.sin()));
    }
    pts
}

impl Panel for MandalaPanel {
    fn name(&self) -> &str {
        "mandala"
    }

    fn refresh_ms(&self) -> u64 {
        100 // smooth rotation
    }

    fn tick(&mut self) {}

    fn render(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Line::from(Span::styled(" mandala ", theme::pane_header())));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let t = self.started.elapsed().as_secs_f64();

        // Three layered rose curves rotating at different rates and petal counts.
        let layer_a = rose(3.0, t * 0.20, 0.95, 360);
        let layer_b = rose(5.0, -t * 0.13, 0.70, 360);
        let layer_c = rose(7.0, t * 0.08, 0.45, 360);

        let (xb, yb) = braille_aspect_bounds(inner, 1.0, 1.0);
        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds(xb)
            .y_bounds(yb)
            .paint(move |ctx| {
                ctx.draw(&Points {
                    coords: &layer_a,
                    color: theme::lavender(),
                });
                ctx.layer();
                ctx.draw(&Points {
                    coords: &layer_b,
                    color: theme::pink(),
                });
                ctx.layer();
                ctx.draw(&Points {
                    coords: &layer_c,
                    color: theme::magenta(),
                });
            });
        f.render_widget(canvas, inner);
    }
}
