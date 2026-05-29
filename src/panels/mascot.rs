//! Mascot panel. Hand-designed pixel-art creature rotating through several
//! poses. Pure decoration tile — no data source, just rotating glance vibes.
use crate::layout::braille_aspect_bounds;
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::time::Instant;

/// Each pose: bitmap rows top-to-bottom of (body, accent, face) layers. Each
/// row is the same width. ` ` = transparent, `B` = body, `A` = accent
/// (lighter highlight), `F` = face/eyes/mouth (dark contrast).
struct Pose {
    name: &'static str,
    caption: &'static str,
    grid: &'static [&'static str],
}

const POSES: &[Pose] = &[
    Pose {
        name: "hello",
        caption: "glance is glad you're here.",
        grid: &[
            "    BBBBBBBB    ",
            "   BBBBBBBBBB   ",
            "  BBABBBBBBABB  ",
            "  BBAFFAAFFABB  ",
            "  BBBBBBBBBBBB  ",
            "  BBBBBBFFBBBB  ",
            "  BBBBFFFFFFBB  ",
            "   BBBBBBBBBB   ",
            "    BB    BB    ",
            "                ",
        ],
    },
    Pose {
        name: "wink",
        caption: "winking at you.",
        grid: &[
            "    BBBBBBBB    ",
            "   BBBBBBBBBB   ",
            "  BBABBBBBBABB  ",
            "  BBAFFAAFFFBB  ",
            "  BBBBBBBBBBBB  ",
            "  BBBBBFFFBBBB  ",
            "  BBBBBBBBBBBB  ",
            "   BBBBBBBBBB   ",
            "    BB    BB    ",
            "                ",
        ],
    },
    Pose {
        name: "smug",
        caption: "everything is fine.",
        grid: &[
            "    BBBBBBBB    ",
            "   BBBBBBBBBB   ",
            "  BBBBBBBBBBBB  ",
            "  BBFFBBBBFFBB  ",
            "  BBBBBBBBBBBB  ",
            "  BBBFFFFFFBBB  ",
            "  BBFFBBBBFFBB  ",
            "   BBBBBBBBBB   ",
            "    BB    BB    ",
            "                ",
        ],
    },
    Pose {
        name: "love",
        caption: "feelin' it.",
        grid: &[
            "  AA      AA    ",
            " AAAA    AAAA   ",
            "AAAAAA  AAAAAA  ",
            " AAAAAAAAAAAA   ",
            "  AAFFAAAAFFAA  ",
            "  AAAAAAAAAAAA  ",
            "  AAAFFFFFFAAA  ",
            "   AAAAAAAAAA   ",
            "    AA    AA    ",
            "                ",
        ],
    },
    Pose {
        name: "snooze",
        caption: "off the clock.",
        grid: &[
            "             zz ",
            "           zz   ",
            "    BBBBBBBB    ",
            "   BBBBBBBBBB   ",
            "  BBFFBBBBFFBB  ",
            "  BBBBBBBBBBBB  ",
            "  BBBBBFFBBBBB  ",
            "   BBBBBBBBBB   ",
            "    BB    BB    ",
            "                ",
        ],
    },
    Pose {
        name: "wave",
        caption: "waving hi.",
        grid: &[
            "          BB    ",
            "    BBBBBBBB    ",
            "   BBBBBBBBBB   ",
            "  BBABBBBBBABB  ",
            "  BBAFFAAFFABB  ",
            "  BBBBBBBBBBBB  ",
            "  BBBBFFFFFFBB  ",
            "   BBBBBBBBBB   ",
            "    BB    BB    ",
            "                ",
        ],
    },
];

pub struct MascotPanel {
    started: Instant,
    cycle_secs: u64,
}

impl MascotPanel {
    pub fn new() -> Self {
        let cycle_secs = std::env::var("GLANCE_MASCOT_CYCLE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);
        Self {
            started: Instant::now(),
            cycle_secs,
        }
    }

    fn current_pose(&self) -> &'static Pose {
        let elapsed = self.started.elapsed().as_secs();
        let idx = (elapsed / self.cycle_secs.max(1)) as usize % POSES.len();
        &POSES[idx]
    }
}

/// Convert a Pose's grid into 3 lists of canvas points (one per layer color).
fn pose_layers(pose: &Pose) -> (Vec<(f64, f64)>, Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let rows = pose.grid.len() as f64;
    let cols = pose
        .grid
        .iter()
        .map(|r| r.chars().count())
        .max()
        .unwrap_or(1) as f64;

    let mut body = Vec::new();
    let mut accent = Vec::new();
    let mut face = Vec::new();
    let mut zzz = Vec::new();

    // Canvas coordinates: center the grid in [-1, 1] × [-1, 1].
    // We invert y so row 0 (top) corresponds to high y.
    for (row_i, row) in pose.grid.iter().enumerate() {
        for (col_i, ch) in row.chars().enumerate() {
            let x = (col_i as f64 - cols / 2.0) / (cols / 2.0);
            let y = -(row_i as f64 - rows / 2.0) / (rows / 2.0);
            match ch {
                'B' => body.push((x, y)),
                'A' => accent.push((x, y)),
                'F' => face.push((x, y)),
                'z' => zzz.push((x, y)),
                _ => {}
            }
        }
    }
    // Z's pile back into the accent layer (lavender-ish)
    accent.extend(zzz);
    (body, accent, face)
}

impl Panel for MascotPanel {
    fn name(&self) -> &str {
        "mascot"
    }

    fn refresh_ms(&self) -> u64 {
        500 // not strictly needed but keeps cycle smooth
    }

    fn tick(&mut self) {
        // No data fetching; cycle index derived from elapsed time on render.
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let pose = self.current_pose();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // 0: title chip (TOP)
                Constraint::Length(1), // 1: gap
                Constraint::Min(8),    // 2: canvas (mascot) fills
                Constraint::Length(1), // 3: caption
                Constraint::Length(1), // 4: gap
                Constraint::Length(1), // 5: hint (BOTTOM)
            ])
            .split(area);

        let title = Line::from(vec![
            Span::styled(" mascot ", theme::pane_header()),
            Span::styled(format!("[{}]", pose.name), theme::pane_header_focused()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        let canvas_block = Block::default().borders(Borders::NONE);
        let inner = canvas_block.inner(chunks[2]);
        f.render_widget(canvas_block, chunks[2]);

        let (body, accent, face) = pose_layers(pose);

        // Use aspect-preserving bounds so the creature stays proportional.
        let (xb, yb) = braille_aspect_bounds(inner, 1.0, 1.0);
        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds(xb)
            .y_bounds(yb)
            .paint(move |ctx| {
                if !body.is_empty() {
                    ctx.draw(&Points {
                        coords: &body,
                        color: theme::pink(),
                    });
                }
                ctx.layer();
                if !accent.is_empty() {
                    ctx.draw(&Points {
                        coords: &accent,
                        color: theme::magenta(),
                    });
                }
                ctx.layer();
                if !face.is_empty() {
                    ctx.draw(&Points {
                        coords: &face,
                        color: theme::face(),
                    });
                }
            });
        f.render_widget(canvas, inner);

        let caption = Line::from(Span::styled(pose.caption, theme::now()));
        f.render_widget(
            Paragraph::new(caption).alignment(Alignment::Center),
            chunks[3],
        );

        let hint = Line::from(Span::styled(
            format!(
                "rotates every {}s · set $GLANCE_MASCOT_CYCLE to change",
                self.cycle_secs
            ),
            theme::dim(),
        ));
        f.render_widget(Paragraph::new(hint).alignment(Alignment::Center), chunks[5]);
    }
}
