//! Active Atlantic-basin tropical cyclones from NHC. Shows storm positions on
//! a regional map; off-season pleasant empty state.
use crate::layout::braille_aspect_bounds;
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Map, MapResolution, Points};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use serde::Deserialize;
use std::process::Command;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
struct ApiResponse {
    #[serde(default)]
    activeStorms: Vec<ApiStorm>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiStorm {
    name: Option<String>,
    classification: Option<String>, // "HU" / "TS" / "TD" / "PT" / "MD" etc.
    intensity: Option<String>,      // wind in mph string
    pressure: Option<String>,       // mb
    latitudeNumeric: Option<f64>,
    longitudeNumeric: Option<f64>,
    movementDir: Option<i64>,
    movementSpeed: Option<i64>,
    #[serde(rename = "binNumber")]
    bin: Option<String>,             // e.g. AL022024
}

#[derive(Debug, Clone)]
pub struct Storm {
    pub name: String,
    pub class: String,
    pub wind_mph: String,
    pub pressure: String,
    pub lat: f64,
    pub lon: f64,
    pub bin: String,
}

pub struct HurricanePanel {
    storms: Option<Vec<Storm>>,
    error: Option<String>,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<Result<Vec<Storm>, String>>,
    tx: mpsc::Sender<Result<Vec<Storm>, String>>,
    inflight: Arc<Mutex<bool>>,
}

impl HurricanePanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            storms: None,
            error: None,
            last_kick: None,
            rx,
            tx,
            inflight: Arc::new(Mutex::new(false)),
        }
    }

    fn kick(&mut self) {
        let mut g = match self.inflight.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if *g {
            return;
        }
        *g = true;
        drop(g);
        let tx = self.tx.clone();
        let inflight = Arc::clone(&self.inflight);
        thread::spawn(move || {
            let r = fetch();
            let _ = tx.send(r);
            if let Ok(mut g) = inflight.lock() {
                *g = false;
            }
        });
        self.last_kick = Some(Instant::now());
    }
}

fn fetch() -> Result<Vec<Storm>, String> {
    let out = Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "10",
            "-A",
            "glance/0.1",
            "https://www.nhc.noaa.gov/CurrentStorms.json",
        ])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl exited {}", out.status));
    }
    let parsed: ApiResponse =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("json: {e}"))?;
    let mut storms = Vec::new();
    for s in parsed.activeStorms {
        let bin = s.bin.clone().unwrap_or_default();
        // Filter to Atlantic basin only (binNumber starts with "AL").
        if !bin.starts_with("AL") {
            continue;
        }
        let (Some(lat), Some(lon)) = (s.latitudeNumeric, s.longitudeNumeric) else {
            continue;
        };
        storms.push(Storm {
            name: s.name.unwrap_or_else(|| "Unnamed".to_string()),
            class: s.classification.unwrap_or_else(|| "?".to_string()),
            wind_mph: s.intensity.unwrap_or_default(),
            pressure: s.pressure.unwrap_or_default(),
            lat,
            lon,
            bin,
        });
    }
    Ok(storms)
}

fn class_glyph(c: &str) -> &'static str {
    match c {
        "HU" => "🌀",  // hurricane
        "TS" => "◉",   // tropical storm
        "TD" => "◎",   // tropical depression
        "PT" | "PTC" => "·", // potential
        _ => "?",
    }
}

fn class_color(c: &str) -> Color {
    match c {
        "HU" => theme::magenta(),
        "TS" => theme::pink(),
        "TD" => theme::lavender(),
        _ => theme::dim_purple(),
    }
}

fn class_label(c: &str) -> &'static str {
    match c {
        "HU" => "Hurricane",
        "TS" => "Tropical Storm",
        "TD" => "Tropical Depression",
        "PT" | "PTC" => "Potential TC",
        "MD" => "Tropical Disturbance",
        _ => "Tropical Cyclone",
    }
}

fn off_season_message() -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("☼ ", theme::now()),
            Span::styled("No active Atlantic storms.", theme::pane_header_focused()),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Atlantic hurricane season: June 1 – November 30.",
            theme::dim(),
        )),
        Line::from(Span::styled(
            "This panel auto-updates when the NHC tracks an active cyclone.",
            theme::dim(),
        )),
    ]
}

impl Panel for HurricanePanel {
    fn name(&self) -> &str {
        "hurricane"
    }

    fn refresh_ms(&self) -> u64 {
        10_000
    }

    fn tick(&mut self) {
        while let Ok(r) = self.rx.try_recv() {
            match r {
                Ok(s) => {
                    self.storms = Some(s);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
        let stale = match self.last_kick {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_secs(10 * 60),
        };
        if stale {
            self.kick();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(3)])
            .split(area);

        let title = Line::from(vec![
            Span::styled(" hurricane ", theme::pane_header()),
            Span::styled("Atlantic basin", theme::pane_header_focused()),
            Span::styled("  NHC nhc.noaa.gov", theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        let body = chunks[1];

        match (&self.storms, &self.error) {
            (None, None) => {
                f.render_widget(crate::widgets::loading("checking NHC"), body);
                return;
            }
            (_, Some(err)) if self.storms.is_none() => {
                f.render_widget(crate::widgets::error(err), body);
                return;
            }
            _ => {}
        }

        let storms = self.storms.as_ref().unwrap();

        // Split body into map (top 60%) + storm list (bottom 40%)
        let body_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Min(4)])
            .split(body);

        // Atlantic basin map: roughly lon -100..0, lat 0..50
        let map_area = body_chunks[0];
        let map_block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(theme::dim())
            .title(Line::from(Span::styled(
                if storms.is_empty() {
                    " quiet seas ".to_string()
                } else {
                    format!(" {} active ", storms.len())
                },
                theme::pane_header(),
            )));
        let inner = map_block.inner(map_area);
        f.render_widget(map_block, map_area);

        // Group points by class for layered drawing
        let mut by_class: Vec<(&str, Color, Vec<(f64, f64)>)> = vec![
            ("HU", theme::magenta(), Vec::new()),
            ("TS", theme::pink(), Vec::new()),
            ("TD", theme::lavender(), Vec::new()),
        ];
        for s in storms {
            let entry = by_class.iter_mut().find(|(c, _, _)| *c == s.class);
            if let Some((_, _, v)) = entry {
                v.push((s.lon, s.lat));
            } else {
                // Unknown class: drop into TD bucket
                by_class[2].2.push((s.lon, s.lat));
            }
        }

        // Atlantic basin bounds: lon -100..20, lat 0..50 -> half-x=60, half-y=25
        // braille_aspect_bounds assumes content centered at 0; the basin is centered roughly at
        // (lon -40, lat 25). We'll just use a fixed projection rather than aspect-preserving
        // to keep the map readable.
        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([-100.0, 20.0])
            .y_bounds([0.0, 50.0])
            .paint(move |ctx| {
                ctx.draw(&Map {
                    resolution: MapResolution::High,
                    color: theme::map_border(),
                });
                ctx.layer();
                for (_, color, pts) in &by_class {
                    if !pts.is_empty() {
                        ctx.draw(&Points {
                            coords: pts,
                            color: *color,
                        });
                    }
                }
            });
        f.render_widget(canvas, inner);

        // Storm list (or off-season message)
        let list_area = body_chunks[1];
        if storms.is_empty() {
            f.render_widget(Paragraph::new(off_season_message()), list_area);
            return;
        }

        let mut lines = Vec::new();
        for s in storms {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", class_glyph(&s.class)),
                    Style::default().fg(class_color(&s.class)),
                ),
                Span::styled(
                    format!("{}", s.name),
                    Style::default()
                        .fg(class_color(&s.class))
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::styled(format!(" {}", class_label(&s.class)), theme::dim()),
                Span::styled(
                    format!("   {} mph", s.wind_mph),
                    theme::historical(),
                ),
                Span::styled(
                    format!("   {} mb", s.pressure),
                    theme::dim(),
                ),
                Span::styled(
                    format!("   {:.1}°N {:.1}°W", s.lat, s.lon.abs()),
                    theme::dim(),
                ),
            ]));
        }
        f.render_widget(Paragraph::new(lines), list_area);
    }
}
