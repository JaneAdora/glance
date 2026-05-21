//! Quick-reference palette of the launcher family + live cards (Wave 0: gst).
//! Vertical, single-column, mobile-first. Card data is fetched by shelling out
//! to `<bin> --summary --json` on a background thread (weather/commits pattern).
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::process::Command;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// (name, description, shortcut). The full family; cards exist only for some.
const PALETTE: &[(&str, &str, char)] = &[
    ("gst", "git status/log", 'g'),
    ("clip", "clipboard", 'c'),
    ("op", "1password", 'o'),
    ("proc", "processes", 'p'),
    ("docker", "containers", 'd'),
    ("svc", "services", 's'),
    ("ssh", "hosts", 'h'),
    ("note", "journal", 'n'),
    ("gh", "PR triage", 'G'),
    ("port", "listeners", 't'),
    ("agent", "AI sessions", 'a'),
    ("hub", "hubspot portals", 'b'),
];

pub struct LaunchersPanel {
    gst_card: Option<String>,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<Option<String>>,
    tx: mpsc::Sender<Option<String>>,
    inflight: Arc<Mutex<bool>>,
}

impl LaunchersPanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { gst_card: None, last_kick: None, rx, tx, inflight: Arc::new(Mutex::new(false)) }
    }

    fn kick(&mut self) {
        let mut g = match self.inflight.lock() { Ok(g) => g, Err(_) => return };
        if *g { return; }
        *g = true;
        drop(g);
        let tx = self.tx.clone();
        let inflight = Arc::clone(&self.inflight);
        thread::spawn(move || {
            let out = Command::new("gst").args(["--summary", "--json"]).output();
            let headline = out.ok().filter(|o| o.status.success()).and_then(|o| {
                let v: serde_json::Value = serde_json::from_slice(&o.stdout).ok()?;
                Some(v.get("headline")?.as_str()?.to_string())
            });
            let _ = tx.send(headline);
            if let Ok(mut g) = inflight.lock() { *g = false; }
        });
        self.last_kick = Some(Instant::now());
    }
}

impl Panel for LaunchersPanel {
    fn name(&self) -> &str { "launchers" }
    fn refresh_ms(&self) -> u64 { 5_000 }

    fn tick(&mut self) {
        while let Ok(card) = self.rx.try_recv() { self.gst_card = card; }
        let stale = match self.last_kick {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_secs(60),
        };
        if stale { self.kick(); }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),                              // title
                Constraint::Length(PALETTE.len() as u16),          // palette
                Constraint::Length(1),                             // divider
                Constraint::Min(0),                                // cards
            ])
            .split(area);

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" launchers", theme::pane_header()))),
            chunks[0],
        );

        let rows: Vec<Line> = PALETTE
            .iter()
            .map(|(name, desc, key)| {
                Line::from(vec![
                    Span::styled(format!("  {name:<7}"), theme::pane_header_focused()),
                    Span::styled(format!("{desc:<16}"), theme::dim()),
                    Span::styled(format!("[{key}]"), theme::historical()),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(rows), chunks[1]);

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" ──────────────", theme::dim()))),
            chunks[2],
        );

        let card = match &self.gst_card {
            Some(h) => format!(" gst · {h}"),
            None => " gst · …".to_string(),
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(card, theme::now()))),
            chunks[3],
        );
    }
}
