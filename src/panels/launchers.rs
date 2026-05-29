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

/// (name, description, shortcut, built). `built` gates the spawn action; the
/// full family is listed, but only genuine suite launchers are spawnable.
const PALETTE: &[(&str, &str, char, bool)] = &[
    ("gst", "git status/log", 'g', true),
    ("clip", "clipboard", 'c', true),
    ("1p", "1password", 'o', true),
    ("proc", "processes", 'P', true),
    ("roam", "directories", 'R', true),
    ("wt", "git worktrees", 'w', true),
    ("recall", "cc sessions", 'l', true),
    ("docker", "containers", 'd', false),
    ("svc", "services", 's', false),
    ("ssh", "hosts", 'h', false),
    ("note", "journal", 'N', false),
    ("gh", "PR triage", 'G', false),
    ("port", "listeners", 't', false),
    ("agent", "AI sessions", 'a', false),
    ("hub", "hubspot portals", 'b', false),
    ("mm", "miss minutes", 'm', true),
];

/// Launchers that expose a live glance card, and the binary to call for it.
/// (panel name, binary). Wave 1-3 append here as each launcher ships its
/// `--summary --json` envelope; render()/kick_all() pick them up automatically.
const CARDS: &[(&str, &str)] = &[("gst", "gst"), ("clip", "clip"), ("proc", "proc")];

pub struct LaunchersPanel {
    /// launcher name -> latest headline. Absent until the first fetch lands.
    cards: HashMap<String, String>,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<(String, Option<String>)>,
    tx: mpsc::Sender<(String, Option<String>)>,
    /// launcher names with an in-flight worker, so we never double-spawn.
    inflight: Arc<Mutex<HashSet<String>>>,
    /// Transient status message (copied / opened / not-built / no-tmux), shown
    /// in the title for 3s. Generalized from the old copy-only toast.
    status: Option<(String, Instant)>,
    /// Cursor index into PALETTE for the spawn action.
    selected: usize,
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
            status: None,
            selected: 0,
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

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn move_down(&mut self) {
        self.selected = (self.selected + 1).min(PALETTE.len() - 1);
    }

    /// Spawn the focused launcher in a new tmux window (bare command), or toast
    /// if it is not built / not in tmux. We spawn the command name (e.g. "1p"),
    /// never "op" -- the fork-bomb path is never constructed here.
    fn spawn_selected(&mut self) {
        let (name, _, _, built) = PALETTE[self.selected];
        if !built {
            self.status = Some((format!("{name}: not built yet"), Instant::now()));
            return;
        }
        if crate::spawn::in_tmux() {
            let ok = crate::spawn::tmux_new_window(None, &[name]);
            let msg = if ok {
                format!("opened {name} in new tmux window")
            } else {
                "tmux failed".to_string()
            };
            self.status = Some((msg, Instant::now()));
        } else {
            crate::clip::copy(name);
            self.status = Some((format!("no tmux: copied {name}"), Instant::now()));
        }
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

        let mut title = vec![Span::styled(" launchers", theme::pane_header())];
        if let Some((msg, ts)) = &self.status {
            if ts.elapsed() < Duration::from_secs(3) {
                title.push(Span::styled(format!("   {msg}"), theme::now()));
            }
        }
        f.render_widget(Paragraph::new(Line::from(title)), chunks[0]);

        let rows: Vec<Line> = PALETTE
            .iter()
            .enumerate()
            .map(|(i, (name, desc, key, built))| {
                let focused = i == self.selected;
                let gutter = if focused { "▸ " } else { "  " };
                let name_style = if !*built {
                    theme::dim()
                } else if focused {
                    theme::active_row()
                } else {
                    theme::pane_header()
                };
                let key_style = if *built { theme::historical() } else { theme::dim() };
                Line::from(vec![
                    Span::styled(format!("{gutter}{name:<7}"), name_style),
                    Span::styled(format!("{desc:<16}"), theme::dim()),
                    Span::styled(format!("[{key}]"), key_style),
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

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_up();
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_down();
                true
            }
            KeyCode::Enter => {
                self.spawn_selected();
                true
            }
            KeyCode::Char(c) => {
                if let Some(i) = PALETTE.iter().position(|(_, _, k, _)| *k == c) {
                    let name = PALETTE[i].0;
                    crate::clip::copy(name);
                    self.selected = i;
                    self.status = Some((format!("copied: {name}"), Instant::now()));
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn built_set_is_exact() {
        let built: HashSet<&str> = PALETTE
            .iter()
            .filter(|(_, _, _, b)| *b)
            .map(|(n, _, _, _)| *n)
            .collect();
        let expected: HashSet<&str> =
            ["gst", "clip", "1p", "proc", "roam", "wt", "recall", "mm"].into_iter().collect();
        assert_eq!(built, expected);
    }

    #[test]
    fn cursor_clamps_at_top() {
        let mut p = LaunchersPanel::new();
        p.selected = 0;
        p.move_up();
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn cursor_clamps_at_bottom() {
        let mut p = LaunchersPanel::new();
        p.selected = PALETTE.len() - 1;
        p.move_down();
        assert_eq!(p.selected, PALETTE.len() - 1);
    }

    #[test]
    fn cursor_moves_within_bounds() {
        let mut p = LaunchersPanel::new();
        p.selected = 2;
        p.move_up();
        assert_eq!(p.selected, 1);
        p.move_down();
        assert_eq!(p.selected, 2);
    }
}
