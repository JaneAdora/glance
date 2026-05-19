use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use crate::layout::braille_aspect_bounds;
use ratatui::widgets::canvas::{Canvas, Map, MapResolution, Points};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::process::Command;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct TsState {
    pub self_relay: String,
    pub self_host: String,
    pub peers: Vec<TsPeer>,
}

#[derive(Clone, Debug)]
pub struct TsPeer {
    pub relay: String,
    pub host: String,
    pub online: bool,
    pub infra: bool, // funnel-ingress-node etc. (no OS string)
}

pub struct TsMapPanel {
    state: Option<TsState>,
    error: Option<String>,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<Result<TsState, String>>,
    tx: mpsc::Sender<Result<TsState, String>>,
    inflight: Arc<Mutex<bool>>,
}

impl TsMapPanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            state: None,
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
            let result = fetch_status();
            let _ = tx.send(result);
            if let Ok(mut g) = inflight.lock() {
                *g = false;
            }
        });
        self.last_kick = Some(Instant::now());
    }
}

fn fetch_status() -> Result<TsState, String> {
    let out = Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .map_err(|e| format!("tailscale not runnable: {e}"))?;
    if !out.status.success() {
        return Err(format!("tailscale status exited {}", out.status));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("bad json: {e}"))?;

    let self_obj = v.get("Self").ok_or_else(|| "no Self in status".to_string())?;
    let self_relay = self_obj
        .get("Relay")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let self_host = self_obj
        .get("HostName")
        .and_then(|x| x.as_str())
        .unwrap_or("self")
        .to_string();

    let mut peers = Vec::new();
    if let Some(peer_obj) = v.get("Peer").and_then(|p| p.as_object()) {
        for (_, p) in peer_obj.iter() {
            let relay = p
                .get("Relay")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if relay.is_empty() {
                continue;
            }
            let host = p
                .get("HostName")
                .and_then(|x| x.as_str())
                .unwrap_or("?")
                .to_string();
            let online = p.get("Online").and_then(|x| x.as_bool()).unwrap_or(false);
            let os = p.get("OS").and_then(|x| x.as_str()).unwrap_or("");
            peers.push(TsPeer {
                relay,
                host,
                online,
                infra: os.is_empty(),
            });
        }
    }

    Ok(TsState {
        self_relay,
        self_host,
        peers,
    })
}

/// Lat/lon for common Tailscale DERP relay airport codes.
/// Returned coords are (latitude, longitude) in degrees.
fn relay_coords(code: &str) -> Option<(f64, f64)> {
    let lc = code.to_lowercase();
    let map: &[(&str, f64, f64)] = &[
        ("ams", 52.31, 4.76),
        ("atl", 33.64, -84.43),
        ("blr", 12.95, 77.69),
        ("bom", 19.09, 72.87),
        ("den", 39.74, -104.99),
        ("dfw", 32.78, -96.81),
        ("dxb", 25.25, 55.36),
        ("fra", 50.04, 8.55),
        ("gru", -23.43, -46.48),
        ("hel", 60.32, 24.97),
        ("hkg", 22.31, 113.92),
        ("hnl", 21.32, -157.92),
        ("iad", 38.94, -77.45),
        ("icn", 37.46, 126.44),
        ("jnb", -26.13, 28.24),
        ("lax", 33.94, -118.41),
        ("lhr", 51.51, -0.13),
        ("maa", 12.99, 80.17),
        ("mad", 40.42, -3.70),
        ("mia", 25.79, -80.29),
        ("nrt", 35.55, 139.78),
        ("nyc", 40.71, -74.01),
        ("ord", 41.88, -87.63),
        ("otp", 44.57, 26.10),
        ("par", 48.86, 2.35),
        ("sao", -23.43, -46.48),
        ("sea", 47.45, -122.31),
        ("sfo", 37.62, -122.38),
        ("sin", 1.36, 103.99),
        ("syd", -33.87, 151.21),
        ("tor", 43.68, -79.63),
        ("tpe", 25.08, 121.23),
        ("waw", 52.17, 20.97),
        ("yyz", 43.68, -79.63),
    ];
    for (k, lat, lon) in map {
        if *k == lc {
            return Some((*lat, *lon));
        }
    }
    None
}

const SELF_COLOR: Color = Color::Rgb(0xff, 0x6e, 0xc7); // magenta
const PEER_ONLINE: Color = Color::Rgb(0xe8, 0x8b, 0x9f); // pink
const PEER_OFFLINE: Color = Color::Rgb(0x80, 0x60, 0xb0); // dim purple
const INFRA: Color = Color::Rgb(0xc5, 0xa3, 0xff); // lavender
const MAP_BORDER: Color = Color::Rgb(0x60, 0x4f, 0x80); // dimmer lavender for map borders

impl Panel for TsMapPanel {
    fn name(&self) -> &str {
        "tsmap"
    }

    fn refresh_ms(&self) -> u64 {
        30_000
    }

    fn tick(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            match result {
                Ok(s) => {
                    self.state = Some(s);
                    self.error = None;
                }
                Err(e) => {
                    self.error = Some(e);
                }
            }
        }
        let stale = match self.last_kick {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_millis(self.refresh_ms() - 1000),
        };
        if stale {
            self.kick();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        if let Some(err) = &self.error {
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("(tailscale unavailable) ", theme::dim()),
                    Span::styled(err.clone(), theme::historical()),
                ])),
                area,
            );
            return;
        }
        let state = match &self.state {
            Some(s) => s,
            None => {
                f.render_widget(
                    Paragraph::new("loading tailscale status…").style(theme::dim()),
                    area,
                );
                return;
            }
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area);

        // Build point sets, deduped to one dot per (relay, classification)
        let mut infra_pts: Vec<(f64, f64)> = Vec::new();
        let mut online_pts: Vec<(f64, f64)> = Vec::new();
        let mut offline_pts: Vec<(f64, f64)> = Vec::new();
        for p in &state.peers {
            let (lat, lon) = match relay_coords(&p.relay) {
                Some(c) => c,
                None => continue,
            };
            let pt = (lon, lat); // x=lon, y=lat
            if p.infra {
                infra_pts.push(pt);
            } else if p.online {
                online_pts.push(pt);
            } else {
                offline_pts.push(pt);
            }
        }
        let self_pt = relay_coords(&state.self_relay);

        let block = Block::default()
            .borders(Borders::NONE)
            .title(Line::from(Span::styled(
                " tailscale net ",
                theme::pane_header(),
            )));
        let map_area = block.inner(chunks[0]);
        f.render_widget(block, chunks[0]);

        let (xb, yb) = braille_aspect_bounds(map_area, 180.0, 90.0);
        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds(xb)
            .y_bounds(yb)
            .paint(move |ctx| {
                ctx.draw(&Map {
                    resolution: MapResolution::High,
                    color: MAP_BORDER,
                });
                ctx.layer();
                if !infra_pts.is_empty() {
                    ctx.draw(&Points {
                        coords: &infra_pts,
                        color: INFRA,
                    });
                }
                if !offline_pts.is_empty() {
                    ctx.draw(&Points {
                        coords: &offline_pts,
                        color: PEER_OFFLINE,
                    });
                }
                if !online_pts.is_empty() {
                    ctx.draw(&Points {
                        coords: &online_pts,
                        color: PEER_ONLINE,
                    });
                }
                if let Some((lat, lon)) = self_pt {
                    // Draw a small cross to make self obvious.
                    let s = 3.0;
                    ctx.draw(&Points {
                        coords: &[
                            (lon, lat),
                            (lon + s, lat),
                            (lon - s, lat),
                            (lon, lat + s),
                            (lon, lat - s),
                        ],
                        color: SELF_COLOR,
                    });
                }
            });
        f.render_widget(canvas, map_area);

        // Legend / counts below map
        let online_count = state.peers.iter().filter(|p| p.online && !p.infra).count();
        let offline_count = state.peers.iter().filter(|p| !p.online && !p.infra).count();
        let infra_count = state.peers.iter().filter(|p| p.infra).count();
        let total = state.peers.len();

        let lines = vec![
            Line::from(vec![
                Span::styled("  ✚ ", Style::default().fg(SELF_COLOR)),
                Span::styled(format!("self ({} @ {}) ", state.self_host, state.self_relay), theme::pane_header()),
            ]),
            Line::from(vec![
                Span::styled("  ● ", Style::default().fg(PEER_ONLINE)),
                Span::styled(format!("{} online   ", online_count), theme::pane_header()),
                Span::styled("● ", Style::default().fg(PEER_OFFLINE)),
                Span::styled(format!("{} offline   ", offline_count), theme::dim()),
                Span::styled("● ", Style::default().fg(INFRA)),
                Span::styled(format!("{} infra   ", infra_count), theme::dim()),
                Span::styled(format!("({} total)", total), theme::dim()),
            ]),
        ];
        f.render_widget(Paragraph::new(lines), chunks[1]);
    }
}
