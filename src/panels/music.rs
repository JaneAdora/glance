//! Now-playing track from playerctl (MPRIS). Marquee-scrolls long titles.
//! Graceful when no player is running.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::Frame;
use std::process::Command;
use std::time::Instant;

pub struct MusicPanel {
    status: String,   // Playing / Paused / Stopped / (none)
    artist: String,
    title: String,
    album: String,
    position: f64,    // seconds
    length: f64,      // seconds
    started: Instant, // for marquee animation
}

impl MusicPanel {
    pub fn new() -> Self {
        Self {
            status: "(none)".to_string(),
            artist: String::new(),
            title: String::new(),
            album: String::new(),
            position: 0.0,
            length: 0.0,
            started: Instant::now(),
        }
    }
}

fn playerctl(args: &[&str]) -> Option<String> {
    let out = Command::new("playerctl").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn marquee(text: &str, width: usize, t: f64) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    // Scroll with a gap, wrapping around.
    let gap = "   •   ";
    let full: Vec<char> = format!("{text}{gap}").chars().collect();
    let offset = (t * 3.0) as usize % full.len();
    let mut out = String::new();
    for i in 0..width {
        out.push(full[(offset + i) % full.len()]);
    }
    out
}

impl Panel for MusicPanel {
    fn name(&self) -> &str {
        "music"
    }

    fn refresh_ms(&self) -> u64 {
        500
    }

    fn tick(&mut self) {
        match playerctl(&["status"]) {
            Some(s) => self.status = s,
            None => {
                self.status = "(none)".to_string();
                return;
            }
        }
        self.artist = playerctl(&["metadata", "artist"]).unwrap_or_default();
        self.title = playerctl(&["metadata", "title"]).unwrap_or_default();
        self.album = playerctl(&["metadata", "album"]).unwrap_or_default();
        self.position = playerctl(&["position"]).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        self.length = playerctl(&["metadata", "mpris:length"])
            .and_then(|s| s.parse::<f64>().ok())
            .map(|us| us / 1_000_000.0)
            .unwrap_or(0.0);
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),    // flex top
                Constraint::Length(1), // title chip
                Constraint::Length(1), // gap
                Constraint::Length(1), // track title
                Constraint::Length(1), // artist
                Constraint::Length(1), // album
                Constraint::Length(1), // gap
                Constraint::Length(2), // progress
                Constraint::Min(0),    // flex bottom
            ])
            .split(area);

        let glyph = match self.status.as_str() {
            "Playing" => "▶",
            "Paused" => "⏸",
            "Stopped" => "⏹",
            _ => "·",
        };
        let title = Line::from(vec![
            Span::styled(" music ", theme::pane_header()),
            Span::styled(format!("{glyph} {}", self.status), theme::pane_header_focused()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[1]);

        if self.status == "(none)" || self.title.is_empty() {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "nothing playing",
                    theme::dim(),
                )))
                .alignment(Alignment::Center),
                chunks[3],
            );
            return;
        }

        let t = self.started.elapsed().as_secs_f64();
        let w = (area.width as usize).saturating_sub(4).max(8);

        let title_line = Line::from(Span::styled(
            marquee(&self.title, w, t),
            Style::default()
                .fg(theme::magenta())
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
        f.render_widget(
            Paragraph::new(title_line).alignment(Alignment::Center),
            chunks[3],
        );
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(self.artist.clone(), theme::now())))
                .alignment(Alignment::Center),
            chunks[4],
        );
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(self.album.clone(), theme::dim())))
                .alignment(Alignment::Center),
            chunks[5],
        );

        if self.length > 0.0 {
            let pct = ((self.position / self.length) * 100.0).clamp(0.0, 100.0) as u16;
            let pos_m = (self.position as u64) / 60;
            let pos_s = (self.position as u64) % 60;
            let len_m = (self.length as u64) / 60;
            let len_s = (self.length as u64) % 60;
            let gauge = Gauge::default()
                .block(Block::default().borders(Borders::NONE).title(Line::from(
                    Span::styled(
                        format!(" {pos_m}:{pos_s:02} / {len_m}:{len_s:02} ", ),
                        theme::dim(),
                    ),
                )))
                .gauge_style(Style::default().fg(theme::pink()))
                .percent(pct);
            f.render_widget(gauge, chunks[7]);
        }
    }
}
