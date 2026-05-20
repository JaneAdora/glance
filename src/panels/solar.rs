//! Sun position arc: where the sun is in today's sky right now, with horizon
//! line, sunrise/sunset markers, and golden hour highlights.
//!
//! Astronomical math via the NOAA simplified solar position algorithm. Doesn't
//! depend on the weather panel — independent computation from lat/lon.
use crate::layout::braille_aspect_bounds;
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::f64::consts::PI;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct SolarPanel {
    lat: f64,
    lon: f64,
    location: String,
    // Cached values, refreshed on tick
    sunrise_secs: i64,
    solar_noon_secs: i64,
    sunset_secs: i64,
    now_secs: i64,
}

impl SolarPanel {
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
        Self {
            lat,
            lon,
            location,
            sunrise_secs: 0,
            solar_noon_secs: 0,
            sunset_secs: 0,
            now_secs: 0,
        }
    }
}

/// Compute today's sunrise / solar noon / sunset for (lat, lon).
/// Returns (sunrise, solar_noon, sunset) as unix seconds. Uses NOAA's general
/// solar position algorithm; accurate to about a minute. Solar refraction
/// constant is baked into the standard zenith of 90.833°.
fn compute_sun_times(lat_deg: f64, lon_deg: f64) -> (i64, i64, i64) {
    // Wikipedia sunrise equation (NOAA general form). Accuracy ~1 minute,
    // good enough for a glance tile. Uses `round` for the day-count so we
    // get the sunrise/sunset of the current local-day cycle.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let jd_unix_epoch = 2440587.5;
    let j2000 = 2451545.0;
    let jd_now = jd_unix_epoch + now as f64 / 86400.0;

    // Day cycle number: round to the nearest integer JD so we land on the
    // current local day at the configured longitude (not tomorrow or yesterday).
    let n = (jd_now - j2000 + 0.0008).round();
    let j_star = n + 0.0009 - lon_deg / 360.0;

    let m_deg = (357.5291 + 0.98560028 * j_star).rem_euclid(360.0);
    let m_rad = m_deg.to_radians();
    let c = 1.9148 * m_rad.sin() + 0.02 * (2.0 * m_rad).sin() + 0.0003 * (3.0 * m_rad).sin();
    let lambda_deg = (m_deg + c + 180.0 + 102.9372).rem_euclid(360.0);
    let lambda_rad = lambda_deg.to_radians();

    let j_transit = j2000 + j_star + 0.0053 * m_rad.sin() - 0.0069 * (2.0 * lambda_rad).sin();

    let decl = (lambda_rad.sin() * 23.44_f64.to_radians().sin()).asin();
    let zenith = 90.833_f64.to_radians();
    let lat_rad = lat_deg.to_radians();
    let cos_omega = (zenith.cos() - lat_rad.sin() * decl.sin()) / (lat_rad.cos() * decl.cos());

    let solar_noon_unix = ((j_transit - jd_unix_epoch) * 86400.0) as i64;

    if !cos_omega.is_finite() || cos_omega.abs() > 1.0 {
        // Polar day or polar night
        return (solar_noon_unix, solar_noon_unix, solar_noon_unix);
    }
    let omega_deg = cos_omega.acos().to_degrees();
    let j_rise = j_transit - omega_deg / 360.0;
    let j_set = j_transit + omega_deg / 360.0;
    let sunrise = ((j_rise - jd_unix_epoch) * 86400.0) as i64;
    let sunset = ((j_set - jd_unix_epoch) * 86400.0) as i64;
    (sunrise, solar_noon_unix, sunset)
}

/// Format unix seconds as local HH:MM using jiff.
fn local_hhmm(secs: i64) -> String {
    let ts = jiff::Timestamp::from_second(secs).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
    let t = zoned.time();
    format!("{:02}:{:02}", t.hour() as u8, t.minute() as u8)
}

impl Panel for SolarPanel {
    fn name(&self) -> &str {
        "solar"
    }

    fn refresh_ms(&self) -> u64 {
        30_000 // re-evaluate every 30s; sun position changes slowly
    }

    fn tick(&mut self) {
        let (rise, noon, set) = compute_sun_times(self.lat, self.lon);
        self.sunrise_secs = rise;
        self.solar_noon_secs = noon;
        self.sunset_secs = set;
        self.now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // 0: title (TOP)
                Constraint::Fill(1),   // 1: spacer (top third)
                Constraint::Fill(2),   // 2: canvas arc (bottom two-thirds)
                Constraint::Length(2), // 3: legend / times (BOTTOM)
            ])
            .split(area);

        let title = Line::from(vec![
            Span::styled(" solar ", theme::pane_header()),
            Span::styled(self.location.clone(), theme::pane_header_focused()),
            Span::styled(format!("   {:.4}, {:.4}", self.lat, self.lon), theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        let canvas_block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(theme::dim());
        let inner = canvas_block.inner(chunks[2]);
        f.render_widget(canvas_block, chunks[2]);

        // Project the sun's path: a semicircle from sunrise (west: x=-1, y=0) to
        // sunset (east: x=+1, y=0), peaking at solar noon (y=+1). Use cos
        // parameterization: t in [0, 1] from sunrise to sunset.
        let day_len = (self.sunset_secs - self.sunrise_secs).max(1);
        let now_frac = if self.now_secs <= self.sunrise_secs {
            -0.05_f64
        } else if self.now_secs >= self.sunset_secs {
            1.05_f64
        } else {
            (self.now_secs - self.sunrise_secs) as f64 / day_len as f64
        };

        let mut arc_pts: Vec<(f64, f64)> = Vec::with_capacity(120);
        let mut golden_pts: Vec<(f64, f64)> = Vec::with_capacity(40);
        let n = 100;
        for i in 0..=n {
            let t = i as f64 / n as f64;
            let theta = (1.0 - t) * PI; // PI at sunrise, 0 at sunset
            let x = -theta.cos();       // -1 at sunrise to +1 at sunset
            let y = theta.sin();
            // Mark golden hour as the first/last 12% of the daylight arc
            if t < 0.12 || t > 0.88 {
                golden_pts.push((x, y));
            } else {
                arc_pts.push((x, y));
            }
        }

        // Horizon line: y=0 from x=-1.1 to x=+1.1, plus tick marks at sunrise/sunset
        let mut horizon: Vec<(f64, f64)> = Vec::with_capacity(60);
        let mut x = -1.1f64;
        while x <= 1.1 {
            horizon.push((x, 0.0));
            x += 0.04;
        }

        // Sun position
        let sun_pt = if (0.0..=1.0).contains(&now_frac) {
            let theta = (1.0 - now_frac) * PI;
            Some((-theta.cos(), theta.sin()))
        } else {
            None
        };
        let is_day = sun_pt.is_some();

        // Build a small filled disc around the sun position for visibility
        let mut sun_disc: Vec<(f64, f64)> = Vec::new();
        if let Some((sx, sy)) = sun_pt {
            let r = 0.07;
            let mut yy = -r;
            while yy <= r {
                let mut xx = -r;
                while xx <= r {
                    if xx * xx + yy * yy <= r * r {
                        sun_disc.push((sx + xx, sy + yy));
                    }
                    xx += 0.015;
                }
                yy += 0.015;
            }
        }

        // Sunrise/sunset endpoint markers (tick crosses)
        let endpoint_marks: Vec<(f64, f64)> = vec![
            (-1.0, 0.0),
            (-1.0, 0.05),
            (-1.0, -0.05),
            (1.0, 0.0),
            (1.0, 0.05),
            (1.0, -0.05),
        ];

        let (xb, yb) = braille_aspect_bounds(inner, 1.15, 0.65);
        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds(xb)
            .y_bounds(yb)
            .paint(move |ctx| {
                // Horizon line first (background)
                ctx.draw(&Points {
                    coords: &horizon,
                    color: theme::map_border(),
                });
                ctx.draw(&Points {
                    coords: &endpoint_marks,
                    color: theme::lavender(),
                });
                ctx.layer();
                // Arc path
                ctx.draw(&Points {
                    coords: &arc_pts,
                    color: theme::pink(),
                });
                ctx.draw(&Points {
                    coords: &golden_pts,
                    color: theme::magenta(),
                });
                ctx.layer();
                // Sun position (only if it's daytime)
                if !sun_disc.is_empty() {
                    ctx.draw(&Points {
                        coords: &sun_disc,
                        color: theme::magenta(),
                    });
                }
            });
        f.render_widget(canvas, inner);

        // Legend / times row
        let progress_pct = if is_day {
            (now_frac * 100.0).round() as i32
        } else if self.now_secs < self.sunrise_secs {
            -((self.sunrise_secs - self.now_secs) / 60) as i32
        } else {
            -((self.now_secs - self.sunset_secs) / 60) as i32
        };

        let progress_line = if is_day {
            format!("{}% through daylight", progress_pct)
        } else if self.now_secs < self.sunrise_secs {
            format!("{} min until sunrise", -progress_pct)
        } else {
            format!("{} min since sunset", -progress_pct)
        };

        let day_secs = self.sunset_secs - self.sunrise_secs;
        let day_h = day_secs / 3600;
        let day_m = (day_secs.rem_euclid(3600)) / 60;

        let legend = vec![
            Line::from(vec![
                Span::styled("🌅 sunrise ", theme::dim()),
                Span::styled(local_hhmm(self.sunrise_secs), theme::historical()),
                Span::styled("    ☀ noon ", theme::dim()),
                Span::styled(local_hhmm(self.solar_noon_secs), theme::historical()),
                Span::styled("    🌇 sunset ", theme::dim()),
                Span::styled(local_hhmm(self.sunset_secs), theme::historical()),
                Span::styled(format!("    daylight {}h{:02}m", day_h, day_m), theme::dim()),
            ]),
            Line::from(vec![Span::styled(progress_line, theme::now())]),
        ];
        f.render_widget(
            Paragraph::new(legend).alignment(Alignment::Center),
            chunks[3],
        );
    }
}
