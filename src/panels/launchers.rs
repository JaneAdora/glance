//! Quick-reference palette of the launcher family + live cards (Wave 0: gst).
//! Vertical, single-column, mobile-first. Card data is fetched by shelling out
//! to `<bin> --summary --json` on background threads (weather/commits pattern).
//! The card machinery is data-driven (see CARDS): Wave 1-3 add launchers by
//! appending a (panel name, binary) pair, with no other changes here.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::collections::{HashMap, HashSet};
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

/// Launchers that expose a live glance card, and the binary to call for it.
/// (panel name, binary). Wave 1-3 append here as each launcher ships its
/// `--summary --json` envelope; render()/kick_all() pick them up automatically.
const CARDS: &[(&str, &str)] = &[("gst", "gst")];

pub struct LaunchersPanel {
    /// launcher name -> latest headline. Absent until the first fetch lands.
    cards: HashMap<String, String>,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<(String, Option<String>)>,
    tx: mpsc::Sender<(String, Option<String>)>,
    /// launcher names with an in-flight worker, so we never double-spawn.
    inflight: Arc<Mutex<HashSet<String>>>,
}

impl LaunchersPanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            cards: HashMap::new(),
            last_kick: None,
            rx,
            tx,
            inflight: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Spawn one worker per CARDS entry that has no fetch already running.
    fn kick_all(&mut self) {
        for &(name, bin) in CARDS {
            {
                let mut set = match self.inflight.lock() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if set.contains(name) {
                    continue;
                }
                set.insert(name.to_string());
            }
            let tx = self.tx.clone();
            let inflight = Arc::clone(&self.inflight);
            let name = name.to_string();
            let bin = bin.to_string();
            thread::spawn(move || {
                let out = Command::new(&bin).args(["--summary", "--json"]).output();
                let headline = out.ok().filter(|o| o.status.success()).and_then(|o| {
                    let v: serde_json::Value = serde_json::from_slice(&o.stdout).ok()?;
                    Some(v.get("headline")?.as_str()?.to_string())
                });
                let _ = tx.send((name.clone(), headline));
                if let Ok(mut set) = inflight.lock() {
                    set.remove(&name);
                }
            });
        }
        self.last_kick = Some(Instant::now());
    }
}

impl Panel for LaunchersPanel {
    fn name(&self) -> &str {
        "launchers"
    }

    fn refresh_ms(&self) -> u64 {
        // weather.rs-style split: the 5s tick only drains the channel into
        // `cards`; the actual `<bin> --summary --json` fetches are far cheaper to
        // gate, so they run every 60s via `last_kick` inside tick().
        5_000
    }

    fn tick(&mut self) {
        while let Ok((name, headline)) = self.rx.try_recv() {
            match headline {
                Some(h) => {
                    self.cards.insert(name, h);
                }
                None => {
                    self.cards.remove(&name);
                }
            }
        }
        let stale = match self.last_kick {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_secs(60),
        };
        if stale {
            self.kick_all();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        // Layout priority: the palette is the point of this panel, so it gets a
        // fixed slot and the cards region (`Min(0)`) collapses first when the
        // area is short. Constraint splitting clamps to `area`, so a tiny height
        // (e.g. a 3-row area) renders fewer rows rather than panicking.
        // TODO(wave-2): scroll the full palette when even it cannot fit.
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),                     // title
                Constraint::Length(PALETTE.len() as u16),  // palette
                Constraint::Length(1),                     // divider
                Constraint::Min(0),                        // cards
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

        let card_lines: Vec<Line> = CARDS
            .iter()
            .map(|(name, _)| {
                let headline = self.cards.get(*name).map(|s| s.as_str()).unwrap_or("…");
                Line::from(Span::styled(format!(" {name} · {headline}"), theme::now()))
            })
            .collect();
        f.render_widget(Paragraph::new(card_lines), chunks[3]);
    }
}
