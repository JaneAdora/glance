//! Latency to cloud regions plotted on a world map, colored by RTT. Background
//! ping threads; map dots green→pink→magenta as latency climbs.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Map, MapResolution, Points};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::collections::HashMap;
use std::process::Command;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

// (label, host, lat, lon)
const REGIONS: &[(&str, &str, f64, f64)] = &[
    ("us-east", "1.1.1.1", 39.0, -77.5),
    ("us-west", "8.8.8.8", 37.4, -122.1),
    ("eu-west", "google.de", 50.1, 8.7),
    ("eu-north", "yandex.ru", 60.2, 24.9),
    ("asia-se", "1.1.1.1", 1.35, 103.8),
    ("asia-ne", "ntt.jp", 35.7, 139.7),
    ("sa-east", "registro.br", -23.5, -46.6),
    ("au", "telstra.com", -33.9, 151.2),
];

pub struct WorldPingPanel {
    rtt: HashMap<String, Option<f64>>, // label -> ms (None = unreachable)
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<(String, Option<f64>)>,
    tx: mpsc::Sender<(String, Option<f64>)>,
    inflight: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl WorldPingPanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            rtt: HashMap::new(),
            last_kick: None,
            rx,
            tx,
            inflight: Arc::new(Mutex::new(Default::default())),
        }
    }

    fn kick_all(&mut self) {
        for (label, host, _, _) in REGIONS {
            let label = label.to_string();
            let host = host.to_string();
            {
                let mut g = match self.inflight.lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                if g.contains(&label) {
                    continue;
                }
                g.insert(label.clone());
            }
            let tx = self.tx.clone();
            let inflight = Arc::clone(&self.inflight);
            thread::spawn(move || {
                let ms = ping_once(&host);
                let _ = tx.send((label.clone(), ms));
                if let Ok(mut g) = inflight.lock() {
                    g.remove(&label);
                }
            });
        }
        self.last_kick = Some(Instant::now());
    }
}

fn ping_once(host: &str) -> Option<f64> {
    let out = Command::new("ping")
        .args(["-c", "1", "-W", "2", &host])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let i = s.find("time=")?;
    let rest = &s[i + 5..];
    let end = rest.find(' ')?;
    rest[..end].parse().ok()
}

fn rtt_color(ms: Option<f64>) -> Color {
    match ms {
        None => theme::dim_purple(),
        Some(v) if v < 60.0 => theme::sage(),
        Some(v) if v < 150.0 => theme::pink(),
        Some(_) => theme::magenta(),
    }
}

impl Panel for WorldPingPanel {
    fn name(&self) -> &str {
        "world-ping"
    }

    fn refresh_ms(&self) -> u64 {
        3_000
    }

    fn tick(&mut self) {
        while let Ok((label, ms)) = self.rx.try_recv() {
            self.rtt.insert(label, ms);
        }
        let stale = match self.last_kick {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_secs(10),
        };
        if stale {
            self.kick_all();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(5), Constraint::Length(3)])
            .split(area);

        let title = Line::from(vec![
            Span::styled(" world-ping ", theme::pane_header()),
            Span::styled("RTT to cloud regions", theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        // Map with colored region dots (5-point crosses for visibility).
        let map_area = chunks[1];
        let block = Block::default().borders(Borders::BOTTOM).border_style(theme::dim());
        let inner = block.inner(map_area);
        f.render_widget(block, map_area);

        // Group points by color tier so each draws as one Points layer.
        let mut fast: Vec<(f64, f64)> = Vec::new();
        let mut mid: Vec<(f64, f64)> = Vec::new();
        let mut slow: Vec<(f64, f64)> = Vec::new();
        let mut down: Vec<(f64, f64)> = Vec::new();
        for (label, _host, lat, lon) in REGIONS {
            let ms = self.rtt.get(*label).copied().flatten();
            let cross = [
                (*lon, *lat),
                (*lon + 4.0, *lat),
                (*lon - 4.0, *lat),
                (*lon, *lat + 3.0),
                (*lon, *lat - 3.0),
            ];
            let bucket = match ms {
                None => &mut down,
                Some(v) if v < 60.0 => &mut fast,
                Some(v) if v < 150.0 => &mut mid,
                Some(_) => &mut slow,
            };
            bucket.extend_from_slice(&cross);
        }

        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([-180.0, 180.0])
            .y_bounds([-90.0, 90.0])
            .paint(move |ctx| {
                ctx.draw(&Map {
                    resolution: MapResolution::High,
                    color: theme::map_border(),
                });
                ctx.layer();
                ctx.draw(&Points { coords: &down, color: theme::dim_purple() });
                ctx.draw(&Points { coords: &slow, color: theme::magenta() });
                ctx.draw(&Points { coords: &mid, color: theme::pink() });
                ctx.draw(&Points { coords: &fast, color: theme::sage() });
            });
        f.render_widget(canvas, inner);

        // Legend row: list each region + ms
        let mut spans: Vec<Span> = Vec::new();
        for (i, (label, _host, _, _)) in REGIONS.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            let ms = self.rtt.get(*label).copied().flatten();
            let val = match ms {
                Some(v) => format!("{v:.0}ms"),
                None => "—".to_string(),
            };
            spans.push(Span::styled(format!("{label} "), theme::dim()));
            spans.push(Span::styled(val, Style::default().fg(rtt_color(ms))));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), chunks[2]);
    }
}
