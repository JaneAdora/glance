//! NWS active weather alerts for a configured point. Color-coded by severity.
//! Free, no auth, just a User-Agent. Same lat/lon defaults as `weather`.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
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
    features: Option<Vec<ApiFeature>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiFeature {
    properties: ApiProps,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiProps {
    event: Option<String>,
    severity: Option<String>,
    urgency: Option<String>,
    headline: Option<String>,
    #[serde(rename = "areaDesc")]
    area_desc: Option<String>,
    effective: Option<String>,
    expires: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Alert {
    pub event: String,
    pub severity: String,
    pub urgency: String,
    pub headline: String,
    pub area: String,
    pub expires: String,
}

pub struct AlertsPanel {
    lat: f64,
    lon: f64,
    location: String,
    alerts: Option<Vec<Alert>>,
    error: Option<String>,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<Result<Vec<Alert>, String>>,
    tx: mpsc::Sender<Result<Vec<Alert>, String>>,
    inflight: Arc<Mutex<bool>>,
}

impl AlertsPanel {
    pub fn new() -> Self {
        let lat = std::env::var("GLANCE_LAT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30.4515);
        let lon = std::env::var("GLANCE_LON")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(-91.1871);
        let location =
            std::env::var("GLANCE_LOCATION").unwrap_or_else(|_| "Baton Rouge, LA".to_string());
        let (tx, rx) = mpsc::channel();
        Self {
            lat,
            lon,
            location,
            alerts: None,
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
        let lat = self.lat;
        let lon = self.lon;
        thread::spawn(move || {
            let r = fetch(lat, lon);
            let _ = tx.send(r);
            if let Ok(mut g) = inflight.lock() {
                *g = false;
            }
        });
        self.last_kick = Some(Instant::now());
    }
}

fn fetch(lat: f64, lon: f64) -> Result<Vec<Alert>, String> {
    let url = format!("https://api.weather.gov/alerts/active?point={lat},{lon}");
    let out = Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "10",
            "-A",
            "glance/0.1 (jane@repcap.com)",
            "-H",
            "Accept: application/geo+json",
            &url,
        ])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl exited {}", out.status));
    }
    let parsed: ApiResponse =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("json: {e}"))?;
    let mut alerts = Vec::new();
    if let Some(features) = parsed.features {
        for f in features {
            let p = f.properties;
            alerts.push(Alert {
                event: p.event.unwrap_or_else(|| "Alert".to_string()),
                severity: p.severity.unwrap_or_else(|| "Unknown".to_string()),
                urgency: p.urgency.unwrap_or_else(|| "Unknown".to_string()),
                headline: p.headline.unwrap_or_default(),
                area: p.area_desc.unwrap_or_default(),
                expires: p.expires.unwrap_or_default(),
            });
        }
    }
    // Sort by severity (Extreme > Severe > Moderate > Minor > Unknown).
    alerts.sort_by_key(|a| match a.severity.as_str() {
        "Extreme" => 0,
        "Severe" => 1,
        "Moderate" => 2,
        "Minor" => 3,
        _ => 4,
    });
    Ok(alerts)
}

fn severity_style(s: &str) -> Style {
    use ratatui::style::Modifier;
    match s {
        "Extreme" => Style::default().fg(theme::magenta()).add_modifier(Modifier::BOLD),
        "Severe" => Style::default().fg(theme::magenta()),
        "Moderate" => Style::default().fg(theme::pink()),
        "Minor" => Style::default().fg(theme::lavender()),
        _ => theme::dim(),
    }
}

fn severity_glyph(s: &str) -> &'static str {
    match s {
        "Extreme" => "⛔",
        "Severe" => "⚠ ",
        "Moderate" => "▲ ",
        "Minor" => "· ",
        _ => "· ",
    }
}

fn hhmm_local(iso: &str) -> String {
    // Pull "HH:MM" from "2026-05-19T17:30:00-05:00"
    iso.split('T')
        .nth(1)
        .map(|t| t.get(..5).unwrap_or("").to_string())
        .unwrap_or_default()
}

impl Panel for AlertsPanel {
    fn name(&self) -> &str {
        "alerts"
    }

    fn refresh_ms(&self) -> u64 {
        5_000
    }

    fn tick(&mut self) {
        while let Ok(r) = self.rx.try_recv() {
            match r {
                Ok(a) => {
                    self.alerts = Some(a);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
        // Upstream fetch every 5 minutes, gated by last_kick.
        let stale = match self.last_kick {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_secs(5 * 60),
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
            Span::styled(" alerts ", theme::pane_header()),
            Span::styled(self.location.clone(), theme::pane_header_focused()),
            Span::styled("  NWS api.weather.gov", theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        let body = chunks[1];

        match (&self.alerts, &self.error) {
            (None, None) => {
                f.render_widget(crate::widgets::loading("checking NWS"), body);
                return;
            }
            (_, Some(err)) if self.alerts.is_none() => {
                f.render_widget(crate::widgets::error(err), body);
                return;
            }
            _ => {}
        }

        let alerts = self.alerts.as_ref().unwrap();
        if alerts.is_empty() {
            let lines = vec![
                Line::from(vec![
                    Span::styled("☼ ", theme::now()),
                    Span::styled("All quiet.", theme::pane_header_focused()),
                ]),
                Line::from(Span::styled(
                    "No active alerts for this location.",
                    theme::dim(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Updated when an NWS office issues warnings, watches, or advisories.",
                    theme::dim(),
                )),
            ];
            f.render_widget(Paragraph::new(lines), body);
            return;
        }

        let n = alerts.len();
        let per_card = (body.height as usize / n).max(3).min(6);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(per_card as u16); n])
            .split(body);
        for (i, a) in alerts.iter().enumerate() {
            if i >= rows.len() {
                break;
            }
            let sev_style = severity_style(&a.severity);
            let mut lines = vec![Line::from(vec![
                Span::styled(severity_glyph(&a.severity).to_string(), sev_style),
                Span::styled(format!(" {} ", a.event), sev_style),
                Span::styled(format!("[{}]", a.severity), theme::dim()),
                Span::styled(format!("  urgency: {}", a.urgency), theme::dim()),
            ])];
            if !a.headline.is_empty() {
                lines.push(Line::from(Span::styled(a.headline.clone(), theme::historical())));
            }
            if !a.area.is_empty() && per_card >= 4 {
                lines.push(Line::from(vec![
                    Span::styled("area: ", theme::dim()),
                    Span::styled(a.area.clone(), theme::historical()),
                ]));
            }
            if !a.expires.is_empty() && per_card >= 5 {
                lines.push(Line::from(vec![
                    Span::styled("expires: ", theme::dim()),
                    Span::styled(hhmm_local(&a.expires), theme::historical()),
                ]));
            }
            let block = Block::default()
                .borders(Borders::LEFT)
                .border_style(sev_style);
            f.render_widget(Paragraph::new(lines).block(block), rows[i]);
        }
    }
}
