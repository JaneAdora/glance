//! Now-playing track from playerctl (MPRIS). Marquee-scrolls long titles.
//! Graceful when no player is running.
use crate::panels::Panel;
use crate::theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::Frame;
use std::process::Command;
use std::time::{Duration, Instant};

const TOAST_TTL: Duration = Duration::from_secs(2);

pub struct MusicPanel {
    status: String,   // Playing / Paused / Stopped / (none)
    artist: String,
    title: String,
    album: String,
    position: f64,    // seconds
    length: f64,      // seconds
    players: Vec<String>,
    selected: Option<String>,
    shuffle: bool,
    loop_status: String,
    toast: Option<(String, Instant)>,
    started: Instant, // for marquee animation
    auto_target: Option<String>,   // active player picked when none is selected
    last_refresh: Option<Instant>, // throttle for player discovery + selection
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
            players: Vec::new(),
            selected: None,
            shuffle: false,
            loop_status: "None".to_string(),
            toast: None,
            started: Instant::now(),
            auto_target: None,
            last_refresh: None,
        }
    }

    /// The player to act on: the user's explicit selection, else the auto-picked
    /// active player (a real Playing/Paused player, not an idle background tab).
    fn target(&self) -> Option<String> {
        self.selected.clone().or_else(|| self.auto_target.clone())
    }

    fn refresh_players(&mut self) {
        self.players = playerctl(&None, &["-l"])
            .map(|s| {
                s.lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();

        if let Some(selected) = &self.selected {
            if !self.players.iter().any(|p| p == selected) {
                self.selected = None;
            }
        }
    }

    fn set_toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }

    fn run_control(&mut self, args: &[&str], message: &str) {
        if playerctl(&self.target(), args).is_some() {
            self.set_toast(message);
        } else {
            self.set_toast("no player");
        }
    }
}

impl Default for MusicPanel {
    fn default() -> Self {
        Self::new()
    }
}

fn playerctl(target: &Option<String>, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("playerctl");
    if let Some(name) = target {
        cmd.args(["-p", name.as_str()]);
    }
    let out = cmd.args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A full now-playing snapshot fetched in ONE playerctl call.
#[derive(Debug, Default, PartialEq)]
struct Meta {
    status: String,
    artist: String,
    title: String,
    album: String,
    length: f64,   // seconds
    position: f64, // seconds
    shuffle: bool,
}

/// Parse the US-separated output of the batched `metadata --format` call.
/// `{{mpris:length}}` and `{{position}}` are microseconds; `{{shuffle}}` is
/// "true"/"false". Returns None when the player is gone (no/short output).
fn parse_meta(out: &str) -> Option<Meta> {
    let f: Vec<&str> = out.split('\u{1f}').collect();
    if f.len() < 7 {
        return None;
    }
    let us_to_s = |s: &str| s.parse::<f64>().ok().map(|us| us / 1_000_000.0).unwrap_or(0.0);
    Some(Meta {
        status: f[0].to_string(),
        artist: f[1].to_string(),
        title: f[2].to_string(),
        album: f[3].to_string(),
        length: us_to_s(f[4]),
        position: us_to_s(f[5]),
        shuffle: f[6] == "true",
    })
}

/// One batched playerctl call for the whole now-playing snapshot.
fn playerctl_metadata(target: &Option<String>) -> Option<Meta> {
    const FMT: &str = "{{status}}\u{1f}{{xesam:artist}}\u{1f}{{xesam:title}}\u{1f}{{xesam:album}}\u{1f}{{mpris:length}}\u{1f}{{position}}\u{1f}{{shuffle}}";
    parse_meta(&playerctl(target, &["metadata", "--format", FMT])?)
}

/// Choose which player to follow from (name, status) pairs: prefer a Playing
/// player, then a Paused one, else the first. Avoids latching onto an idle
/// background player (e.g. a stopped browser tab) while a real player is active.
fn choose_target(players: &[(String, String)]) -> Option<String> {
    players
        .iter()
        .find(|(_, s)| s == "Playing")
        .or_else(|| players.iter().find(|(_, s)| s == "Paused"))
        .or_else(|| players.first())
        .map(|(name, _)| name.clone())
}

/// Pick the active player by querying each player's status once.
fn pick_active_player(players: &[String]) -> Option<String> {
    if players.is_empty() {
        return None;
    }
    let with_status: Vec<(String, String)> = players
        .iter()
        .map(|name| {
            let st = playerctl(&Some(name.clone()), &["status"]).unwrap_or_default();
            (name.clone(), st)
        })
        .collect();
    choose_target(&with_status)
}

fn marquee(text: &str, width: usize, t: f64) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    // Scroll with a gap, wrapping around. `width` is a cell count, and CJK or
    // emoji glyphs are two cells wide, so fill the window by display width (not
    // char count) and pad to exactly `width` so it never overflows the panel.
    let gap = "   •   ";
    let full: Vec<char> = format!("{text}{gap}").chars().collect();
    let offset = (t * 3.0) as usize % full.len();
    let mut out = String::new();
    let mut cells = 0usize;
    let mut i = 0usize;
    while cells < width && i < full.len() {
        let c = full[(offset + i) % full.len()];
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if cells + cw > width {
            break;
        }
        out.push(c);
        cells += cw;
        i += 1;
    }
    while cells < width {
        out.push(' ');
        cells += 1;
    }
    out
}

fn next_player(players: &[String], current: &Option<String>) -> Option<String> {
    match current {
        None => players.first().cloned(),
        Some(name) => match players.iter().position(|p| p == name) {
            Some(i) if i + 1 < players.len() => Some(players[i + 1].clone()),
            _ => None,
        },
    }
}

fn next_loop_status(current: &str) -> &'static str {
    match current {
        "None" => "Track",
        "Track" => "Playlist",
        "Playlist" => "None",
        _ => "None",
    }
}

fn friendly_player_label(name: &str) -> String {
    if name.starts_with("kdeconnect.mpris_") {
        "phone".to_string()
    } else if name.starts_with("chromium") {
        "chromium".to_string()
    } else if name == "spotify" {
        "spotify".to_string()
    } else {
        name.to_string()
    }
}

fn key_playerctl_args(code: KeyCode) -> Option<&'static [&'static str]> {
    match code {
        KeyCode::Char(' ') => Some(&["play-pause"]),
        KeyCode::Char('>') => Some(&["next"]),
        KeyCode::Char('<') => Some(&["previous"]),
        KeyCode::Left => Some(&["previous"]),
        KeyCode::Right => Some(&["next"]),
        KeyCode::Up | KeyCode::Char('+') | KeyCode::Char('=') => Some(&["volume", "0.05+"]),
        KeyCode::Down | KeyCode::Char('-') | KeyCode::Char('_') => Some(&["volume", "0.05-"]),
        KeyCode::Char('.') => Some(&["position", "5+"]),
        KeyCode::Char(',') => Some(&["position", "5-"]),
        KeyCode::Char('s') => Some(&["shuffle", "toggle"]),
        _ => None,
    }
}

impl Panel for MusicPanel {
    fn name(&self) -> &str {
        "music"
    }

    fn refresh_ms(&self) -> u64 {
        1000
    }

    fn tick(&mut self) {
        if self.toast.as_ref().is_some_and(|(_, shown)| shown.elapsed() >= TOAST_TTL) {
            self.toast = None;
        }

        // Player discovery + active-player selection + loop status rarely change
        // but cost several playerctl spawns (= D-Bus connections). Throttle them
        // to every few seconds. The old code ran ~9 playerctl processes PER TICK
        // at 2 Hz, flooding the session bus until it wedged after a few hours;
        // this is the bulk of that churn.
        let due = self
            .last_refresh
            .map_or(true, |t| t.elapsed() >= Duration::from_secs(5));
        if due {
            self.refresh_players();
            self.auto_target = pick_active_player(&self.players);
            if let Some(l) = playerctl(&self.target(), &["loop"]) {
                self.loop_status = l;
            }
            self.last_refresh = Some(Instant::now());
        }

        // One batched call gets status + all metadata + position + shuffle.
        match playerctl_metadata(&self.target()) {
            Some(m) => {
                self.status = m.status;
                self.artist = m.artist;
                self.title = m.title;
                self.album = m.album;
                self.length = m.length;
                self.position = m.position;
                self.shuffle = m.shuffle;
            }
            None => {
                self.status = "(none)".to_string();
                self.artist.clear();
                self.title.clear();
                self.album.clear();
                self.position = 0.0;
                self.length = 0.0;
                self.shuffle = false;
                self.loop_status = "None".to_string();
            }
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title chip (TOP)
                Constraint::Min(0),    // spacer pushes content to bottom
                Constraint::Length(1), // gap
                Constraint::Length(1), // track title
                Constraint::Length(1), // artist
                Constraint::Length(1), // album
                Constraint::Length(1), // gap
                Constraint::Length(2), // progress
                Constraint::Length(1), // key hint
            ])
            .split(area);

        let glyph = match self.status.as_str() {
            "Playing" => "▶",
            "Paused" => "⏸",
            "Stopped" => "⏹",
            _ => "·",
        };
        let device = self.selected
            .as_deref()
            .map(friendly_player_label)
            .unwrap_or_else(|| "auto".to_string());
        let shuffle = if self.shuffle { " ⇄" } else { "" };
        let loop_status = if self.loop_status != "None" {
            format!(" ↻ {}", self.loop_status)
        } else {
            String::new()
        };
        let title = Line::from(vec![
            Span::styled(" music ", theme::pane_header()),
            Span::styled(format!("{glyph} {}", self.status), theme::pane_header_focused()),
            Span::styled(format!(" @{device}{shuffle}{loop_status}"), theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        let hint = self.toast
            .as_ref()
            .filter(|(_, shown)| shown.elapsed() < TOAST_TTL)
            .map(|(message, _)| message.as_str())
            .unwrap_or("space play  <> track  +/- vol  ., seek  s shuffle  L loop  d device");
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(hint, theme::dim())))
                .alignment(Alignment::Center),
            chunks[8],
        );

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

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
            return false;
        }

        if key.code == KeyCode::Char('d') {
            self.refresh_players();
            self.selected = next_player(&self.players, &self.selected);
            let label = self.selected
                .as_deref()
                .map(friendly_player_label)
                .unwrap_or_else(|| "auto".to_string());
            self.set_toast(format!("target {label}"));
            return true;
        }

        if key.code == KeyCode::Char('L') {
            let next = next_loop_status(&self.loop_status).to_string();
            if playerctl(&self.target(), &["loop", &next]).is_some() {
                self.loop_status = next.clone();
                self.set_toast(format!("loop {next}"));
            } else {
                self.set_toast("no player");
            }
            return true;
        }

        if let Some(args) = key_playerctl_args(key.code) {
            let message = match key.code {
                KeyCode::Char(' ') => "play/pause",
                KeyCode::Char('>') => "next track",
                KeyCode::Char('<') => "previous track",
                KeyCode::Left => "previous track",
                KeyCode::Right => "next track",
                KeyCode::Up | KeyCode::Char('+') | KeyCode::Char('=') => "volume +5%",
                KeyCode::Down | KeyCode::Char('-') | KeyCode::Char('_') => "volume -5%",
                KeyCode::Char('.') => "seek +5s",
                KeyCode::Char(',') => "seek -5s",
                KeyCode::Char('s') => "shuffle toggle",
                _ => "music control",
            };
            if key.code == KeyCode::Char('s') {
                if playerctl(&self.target(), args).is_some() {
                    self.shuffle = !self.shuffle;
                    self.set_toast(message);
                } else {
                    self.set_toast("no player");
                }
            } else {
                self.run_control(args, message);
            }
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marquee_returns_short_text_unchanged() {
        assert_eq!(marquee("hi", 10, 0.0), "hi");
    }

    #[test]
    fn marquee_caps_ascii_to_width() {
        use unicode_width::UnicodeWidthStr;
        assert_eq!(UnicodeWidthStr::width(marquee("abcdefghijklmnop", 8, 0.0).as_str()), 8);
    }

    #[test]
    fn marquee_never_overflows_with_wide_glyphs() {
        use unicode_width::UnicodeWidthStr;
        let cjk = "스트레이키즈 인스 청색 라이브 방송";
        for step in 0..20 {
            let out = marquee(cjk, 9, step as f64);
            assert!(UnicodeWidthStr::width(out.as_str()) <= 9, "overflow at {step}: {out:?}");
        }
    }

    fn players() -> Vec<String> {
        vec!["spotify".to_string(), "chromium.instance1".to_string()]
    }

    #[test]
    fn next_player_walks_ring_and_wraps() {
        let p = players();
        assert_eq!(next_player(&p, &None), Some("spotify".to_string()));
        assert_eq!(next_player(&p, &Some("spotify".to_string())), Some("chromium.instance1".to_string()));
        assert_eq!(next_player(&p, &Some("chromium.instance1".to_string())), None);
    }

    #[test]
    fn next_player_returns_auto_for_missing_current() {
        assert_eq!(next_player(&players(), &Some("gone".to_string())), None);
    }

    #[test]
    fn next_loop_status_cycles() {
        assert_eq!(next_loop_status("None"), "Track");
        assert_eq!(next_loop_status("Track"), "Playlist");
        assert_eq!(next_loop_status("Playlist"), "None");
    }

    #[test]
    fn friendly_player_label_collapses_common_players() {
        assert_eq!(friendly_player_label("kdeconnect.mpris_123"), "phone");
        assert_eq!(friendly_player_label("chromium.instance1"), "chromium");
        assert_eq!(friendly_player_label("spotify"), "spotify");
        assert_eq!(friendly_player_label("vlc"), "vlc");
    }

    #[test]
    fn key_playerctl_args_maps_transport_volume_and_seek() {
        assert_eq!(key_playerctl_args(KeyCode::Char(' ')), Some(&["play-pause"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Char('>')), Some(&["next"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Char('<')), Some(&["previous"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Left), Some(&["previous"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Right), Some(&["next"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Up), Some(&["volume", "0.05+"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Down), Some(&["volume", "0.05-"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Char('+')), Some(&["volume", "0.05+"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Char('=')), Some(&["volume", "0.05+"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Char('-')), Some(&["volume", "0.05-"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Char('_')), Some(&["volume", "0.05-"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Char('.')), Some(&["position", "5+"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Char(',')), Some(&["position", "5-"][..]));
        assert_eq!(key_playerctl_args(KeyCode::Char('s')), Some(&["shuffle", "toggle"][..]));
    }

    #[test]
    fn parse_meta_splits_fields_and_converts_microseconds() {
        let out = "Playing\u{1f}Zero 7\u{1f}This World\u{1f}Simple Things\u{1f}335960000\u{1f}69226016\u{1f}false";
        let m = parse_meta(out).unwrap();
        assert_eq!(m.status, "Playing");
        assert_eq!(m.artist, "Zero 7");
        assert_eq!(m.title, "This World");
        assert_eq!(m.album, "Simple Things");
        assert!((m.length - 335.96).abs() < 0.01, "length {}", m.length);
        assert!((m.position - 69.226).abs() < 0.01, "position {}", m.position);
        assert!(!m.shuffle);
    }

    #[test]
    fn parse_meta_none_on_short_output() {
        assert!(parse_meta("").is_none());
        assert!(parse_meta("Playing\u{1f}only\u{1f}three").is_none());
    }

    #[test]
    fn choose_target_prefers_playing_then_paused_then_first() {
        let mixed = vec![
            ("chromium".to_string(), "Stopped".to_string()),
            ("spotify".to_string(), "Playing".to_string()),
        ];
        assert_eq!(choose_target(&mixed), Some("spotify".to_string()));
        let paused = vec![
            ("a".to_string(), "Stopped".to_string()),
            ("b".to_string(), "Paused".to_string()),
        ];
        assert_eq!(choose_target(&paused), Some("b".to_string()));
        let stopped = vec![
            ("a".to_string(), "Stopped".to_string()),
            ("b".to_string(), "Stopped".to_string()),
        ];
        assert_eq!(choose_target(&stopped), Some("a".to_string()));
        assert_eq!(choose_target(&[]), None);
    }
}
