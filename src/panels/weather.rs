//! Weather panel. Open-Meteo for the data source (free, no auth). Fetches via
//! `curl` subprocess on a background thread so no HTTP crate is needed.
//! Defaults to Baton Rouge, LA; override via $GLANCE_LAT + $GLANCE_LON.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
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

// 5-row × 3-col block-character digit font. Same shape as clock.rs.
const DIGITS: [[&str; 5]; 10] = [
    ["███", "█ █", "█ █", "█ █", "███"],
    ["  █", "  █", "  █", "  █", "  █"],
    ["███", "  █", "███", "█  ", "███"],
    ["███", "  █", "███", "  █", "███"],
    ["█ █", "█ █", "███", "  █", "  █"],
    ["███", "█  ", "███", "  █", "███"],
    ["███", "█  ", "███", "█ █", "███"],
    ["███", "  █", "  █", "  █", "  █"],
    ["███", "█ █", "███", "█ █", "███"],
    ["███", "█ █", "███", "  █", "███"],
];

const MINUS_ROWS: [&str; 5] = ["   ", "   ", "███", "   ", "   "];
const DEGREE_ROWS: [&str; 5] = [" █ ", "█ █", " █ ", "   ", "   "];

#[derive(Debug, Clone, Deserialize)]
struct ApiResponse {
    timezone: Option<String>,
    current: Option<ApiCurrent>,
    daily: Option<ApiDaily>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiCurrent {
    time: Option<String>,
    temperature_2m: Option<f64>,
    relative_humidity_2m: Option<u8>,
    apparent_temperature: Option<f64>,
    is_day: Option<u8>,
    precipitation: Option<f64>,
    weather_code: Option<u32>,
    wind_speed_10m: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiDaily {
    time: Vec<String>,
    weather_code: Vec<u32>,
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
    sunrise: Vec<String>,
    sunset: Vec<String>,
    precipitation_probability_max: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct WeatherData {
    pub tz: String,
    pub current: Current,
    pub days: Vec<Day>,
}

#[derive(Debug, Clone)]
pub struct Current {
    pub temp_f: f64,
    pub feels_f: f64,
    pub humidity: u8,
    pub precip_in: f64,
    pub wind_mph: f64,
    pub code: u32,
    pub is_day: bool,
    pub observed_at: String,
}

#[derive(Debug, Clone)]
pub struct Day {
    pub date: String,         // ISO YYYY-MM-DD
    pub code: u32,
    pub high_f: f64,
    pub low_f: f64,
    pub precip_prob: u8,
    pub sunrise: String,      // ISO
    pub sunset: String,       // ISO
}

pub struct WeatherPanel {
    lat: f64,
    lon: f64,
    location: String,
    data: Option<WeatherData>,
    error: Option<String>,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<Result<WeatherData, String>>,
    tx: mpsc::Sender<Result<WeatherData, String>>,
    inflight: Arc<Mutex<bool>>,
}

impl WeatherPanel {
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
            data: None,
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
            let result = fetch(lat, lon);
            let _ = tx.send(result);
            if let Ok(mut g) = inflight.lock() {
                *g = false;
            }
        });
        self.last_kick = Some(Instant::now());
    }
}

fn fetch(lat: f64, lon: f64) -> Result<WeatherData, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,relative_humidity_2m,apparent_temperature,is_day,precipitation,weather_code,wind_speed_10m&daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,precipitation_probability_max&timezone=auto&temperature_unit=fahrenheit&wind_speed_unit=mph&precipitation_unit=inch&forecast_days=7"
    );
    let out = Command::new("curl")
        .args(["-s", "--max-time", "10", "-A", "glance/0.1", &url])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl exited {}", out.status));
    }
    let parsed: ApiResponse = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("json: {e}"))?;
    let current_raw = parsed.current.ok_or_else(|| "no current data".to_string())?;
    let daily_raw = parsed.daily.ok_or_else(|| "no daily data".to_string())?;

    let current = Current {
        temp_f: current_raw.temperature_2m.unwrap_or(0.0),
        feels_f: current_raw.apparent_temperature.unwrap_or(0.0),
        humidity: current_raw.relative_humidity_2m.unwrap_or(0),
        precip_in: current_raw.precipitation.unwrap_or(0.0),
        wind_mph: current_raw.wind_speed_10m.unwrap_or(0.0),
        code: current_raw.weather_code.unwrap_or(0),
        is_day: current_raw.is_day.unwrap_or(1) == 1,
        observed_at: current_raw.time.unwrap_or_default(),
    };

    let n = daily_raw.time.len();
    let mut days = Vec::with_capacity(n);
    for i in 0..n {
        days.push(Day {
            date: daily_raw.time[i].clone(),
            code: *daily_raw.weather_code.get(i).unwrap_or(&0),
            high_f: *daily_raw.temperature_2m_max.get(i).unwrap_or(&0.0),
            low_f: *daily_raw.temperature_2m_min.get(i).unwrap_or(&0.0),
            precip_prob: *daily_raw.precipitation_probability_max.get(i).unwrap_or(&0),
            sunrise: daily_raw.sunrise.get(i).cloned().unwrap_or_default(),
            sunset: daily_raw.sunset.get(i).cloned().unwrap_or_default(),
        });
    }

    Ok(WeatherData {
        tz: parsed.timezone.unwrap_or_else(|| "UTC".to_string()),
        current,
        days,
    })
}

fn code_glyph(code: u32, is_day: bool) -> &'static str {
    match code {
        0 => if is_day { "☀" } else { "🌙" },
        1 => if is_day { "🌤" } else { "🌙" },
        2 => "⛅",
        3 => "☁",
        45 | 48 => "🌫",
        51 | 53 | 55 | 56 | 57 => "🌦",
        61 | 63 | 65 | 66 | 67 => "🌧",
        71 | 73 | 75 | 77 => "🌨",
        80 | 81 | 82 => "🌧",
        85 | 86 => "🌨",
        95 => "⛈",
        96 | 99 => "⛈",
        _ => "·",
    }
}

fn code_name(code: u32) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mainly Clear",
        2 => "Partly Cloudy",
        3 => "Overcast",
        45 => "Fog",
        48 => "Rime Fog",
        51 => "Light Drizzle",
        53 => "Drizzle",
        55 => "Heavy Drizzle",
        56 | 57 => "Freezing Drizzle",
        61 => "Light Rain",
        63 => "Rain",
        65 => "Heavy Rain",
        66 | 67 => "Freezing Rain",
        71 => "Light Snow",
        73 => "Snow",
        75 => "Heavy Snow",
        77 => "Snow Grains",
        80 => "Light Showers",
        81 => "Showers",
        82 => "Violent Showers",
        85 | 86 => "Snow Showers",
        95 => "Thunderstorm",
        96 | 99 => "Severe Thunderstorm",
        _ => "—",
    }
}

fn weekday_from_iso(iso: &str) -> &'static str {
    // Zeller-ish: parse YYYY-MM-DD and compute day of week.
    let parts: Vec<&str> = iso.split('-').collect();
    if parts.len() != 3 {
        return "?";
    }
    let mut y: i32 = parts[0].parse().unwrap_or(2000);
    let mut m: i32 = parts[1].parse().unwrap_or(1);
    let d: i32 = parts[2].parse().unwrap_or(1);
    if m < 3 {
        m += 12;
        y -= 1;
    }
    let k = y % 100;
    let j = y / 100;
    let h = (d + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j).rem_euclid(7);
    // Zeller: 0=Sat, 1=Sun, 2=Mon, ..., 6=Fri
    match h {
        0 => "Sat",
        1 => "Sun",
        2 => "Mon",
        3 => "Tue",
        4 => "Wed",
        5 => "Thu",
        6 => "Fri",
        _ => "?",
    }
}

fn hhmm(iso: &str) -> String {
    // "2026-05-19T06:07" → "06:07"
    iso.split('T').nth(1).unwrap_or("").to_string()
}

/// One row of the big-block temperature, e.g. "84°".
fn temp_row(temp_f: f64, row: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let style = Style::default().fg(theme::magenta());
    let rounded = temp_f.round() as i32;
    let neg = rounded < 0;
    let abs = rounded.unsigned_abs();
    let mut digits: Vec<u8> = Vec::new();
    if abs == 0 {
        digits.push(0);
    } else {
        let mut n = abs;
        while n > 0 {
            digits.push((n % 10) as u8);
            n /= 10;
        }
        digits.reverse();
    }
    if neg {
        spans.push(Span::styled(MINUS_ROWS[row].to_string(), style));
        spans.push(Span::raw(" "));
    }
    for d in digits {
        spans.push(Span::styled(DIGITS[d as usize][row].to_string(), style));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(DEGREE_ROWS[row].to_string(), style));
    spans
}

impl Panel for WeatherPanel {
    fn name(&self) -> &str {
        "weather"
    }

    fn refresh_ms(&self) -> u64 {
        // Drain the inbox every 5s; actual upstream fetch every 15 min, gated by last_kick.
        5_000
    }

    fn tick(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            match result {
                Ok(d) => {
                    self.data = Some(d);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
        let stale = match self.last_kick {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_secs(15 * 60),
        };
        if stale {
            self.kick();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // 0: title chip (TOP)
                Constraint::Min(0),    // 1: spacer pushes content to bottom
                Constraint::Length(1), // 2: gap
                Constraint::Length(5), // 3: big temperature block
                Constraint::Length(1), // 4: gap
                Constraint::Length(1), // 5: condition glyph + name
                Constraint::Length(1), // 6: feels-like / humidity / wind / precip
                Constraint::Length(1), // 7: sunrise / sunset
                Constraint::Length(1), // 8: gap
                Constraint::Length(2), // 9: 7-day forecast (2 rows: day/glyph then high/low)
            ])
            .split(area);

        // Title
        let title_line = Line::from(vec![
            Span::styled(" weather ", theme::pane_header()),
            Span::styled(self.location.clone(), theme::pane_header_focused()),
            Span::styled(
                format!(
                    "   {:.4}, {:.4}",
                    self.lat, self.lon
                ),
                theme::dim(),
            ),
        ]);
        f.render_widget(Paragraph::new(title_line), chunks[0]);

        let data = match &self.data {
            Some(d) => d,
            None => {
                let msg = if let Some(err) = &self.error {
                    format!("error: {err}")
                } else {
                    "fetching…".to_string()
                };
                let pad = chunks[3];
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(msg, theme::dim())))
                        .alignment(Alignment::Center),
                    pad,
                );
                return;
            }
        };

        // Big temperature
        let mut lines = Vec::with_capacity(5);
        for row in 0..5 {
            lines.push(Line::from(temp_row(data.current.temp_f, row)));
        }
        f.render_widget(
            Paragraph::new(lines).alignment(Alignment::Center),
            chunks[3],
        );

        // Condition glyph + name
        let cond = Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{} ", code_glyph(data.current.code, data.current.is_day)),
                theme::now(),
            ),
            Span::styled(code_name(data.current.code), theme::now()),
        ]);
        f.render_widget(
            Paragraph::new(cond).alignment(Alignment::Center),
            chunks[5],
        );

        // Feels like / humidity / wind / precip
        let stats = Line::from(vec![
            Span::styled("feels ", theme::dim()),
            Span::styled(format!("{:.0}°", data.current.feels_f), theme::historical()),
            Span::styled("   humidity ", theme::dim()),
            Span::styled(format!("{}%", data.current.humidity), theme::historical()),
            Span::styled("   wind ", theme::dim()),
            Span::styled(
                format!("{:.0} mph", data.current.wind_mph),
                theme::historical(),
            ),
            Span::styled("   precip ", theme::dim()),
            Span::styled(
                format!("{:.2}\"", data.current.precip_in),
                theme::historical(),
            ),
        ]);
        f.render_widget(
            Paragraph::new(stats).alignment(Alignment::Center),
            chunks[6],
        );

        // Sunrise / sunset
        let today = data.days.first();
        if let Some(today) = today {
            let sun = Line::from(vec![
                Span::styled("🌅 ", theme::now()),
                Span::styled(hhmm(&today.sunrise), theme::historical()),
                Span::styled("   🌇 ", theme::now()),
                Span::styled(hhmm(&today.sunset), theme::historical()),
                Span::styled(format!("   {}", data.tz), theme::dim()),
            ]);
            f.render_widget(
                Paragraph::new(sun).alignment(Alignment::Center),
                chunks[7],
            );
        }

        // 7-day forecast
        let forecast_area = chunks[9];
        render_forecast(f, forecast_area, data);
    }
}

fn render_forecast(f: &mut Frame, area: Rect, data: &WeatherData) {
    let days = &data.days;
    let n = days.len().min(7);
    if n == 0 || area.width < 20 {
        return;
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, n as u32); n])
        .split(area);
    for (i, d) in days.iter().take(n).enumerate() {
        let wd = weekday_from_iso(&d.date);
        let glyph = code_glyph(d.code, true);
        let line1 = Line::from(vec![
            Span::styled(format!("{wd} "), theme::pane_header()),
            Span::styled(glyph.to_string(), theme::now()),
        ])
        .alignment(Alignment::Center);
        let line2 = Line::from(vec![
            Span::styled(
                format!("{:.0}°", d.high_f),
                Style::default().fg(theme::magenta()),
            ),
            Span::styled(" / ", theme::dim()),
            Span::styled(
                format!("{:.0}°", d.low_f),
                theme::historical(),
            ),
        ])
        .alignment(Alignment::Center);
        let block = Block::default()
            .borders(Borders::NONE)
            .style(Style::default());
        f.render_widget(
            Paragraph::new(vec![line1, line2]).block(block),
            cols[i],
        );
    }
}
