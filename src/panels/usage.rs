//! Claude usage panel: live limit gauges (session / weekly / per-model) from
//! the local OAuth token. Background curl fetch on a 60s cadence, mirroring
//! the weather panel. Read-only: never logs or writes the token.
use crate::panels::Panel;
use crate::theme;
use crate::usage::{
    bar_string, fetch, fmt_reset, header_label, now_ms, read_credentials, util_color, CredsError,
    UsageSnapshot, WindowKind,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

enum Msg {
    Ok {
        header: String,
        snapshot: UsageSnapshot,
    },
    NoCreds,
    Err {
        header: Option<String>,
        reason: String,
    },
}

enum Status {
    Loading,
    Ok,
    Stale(String),
    NoCreds,
}

pub struct UsagePanel {
    snapshot: Option<UsageSnapshot>,
    status: Status,
    header: String,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<Msg>,
    tx: mpsc::Sender<Msg>,
    inflight: Arc<Mutex<bool>>,
}

impl UsagePanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            snapshot: None,
            status: Status::Loading,
            header: "claude".to_string(),
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
            let msg = match read_credentials(now_ms()) {
                Ok(c) => {
                    let header = header_label(&c.subscription, &c.tier);
                    match fetch(&c.access_token) {
                        Ok(snapshot) => Msg::Ok { header, snapshot },
                        Err(reason) => Msg::Err {
                            header: Some(header),
                            reason,
                        },
                    }
                }
                Err(CredsError::Missing) => Msg::NoCreds,
                Err(CredsError::Expired) => Msg::Err {
                    header: None,
                    reason: "token expired, open claude to refresh".to_string(),
                },
                Err(CredsError::Malformed(e)) => Msg::Err {
                    header: None,
                    reason: format!("creds malformed: {e}"),
                },
            };
            let _ = tx.send(msg);
            if let Ok(mut g) = inflight.lock() {
                *g = false;
            }
        });
        self.last_kick = Some(Instant::now());
    }
}

impl Panel for UsagePanel {
    fn name(&self) -> &str {
        "usage"
    }

    fn refresh_ms(&self) -> u64 {
        // Drain the inbox often; the network fetch itself is gated to 60s below.
        500
    }

    fn tick(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Ok { header, snapshot } => {
                    self.header = header;
                    self.snapshot = Some(snapshot);
                    self.status = Status::Ok;
                }
                Msg::NoCreds => self.status = Status::NoCreds,
                Msg::Err { header, reason } => {
                    if let Some(h) = header {
                        self.header = h;
                    }
                    self.status = Status::Stale(reason);
                }
            }
        }
        let due = match self.last_kick {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_secs(60),
        };
        if due {
            self.kick();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(1), // gap
                Constraint::Min(0),    // gauges / message
            ])
            .split(area);

        let title = Line::from(vec![
            Span::styled(" usage ", theme::pane_header()),
            Span::styled(self.header.clone(), theme::pane_header_focused()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        let snap = match &self.snapshot {
            Some(s) => s,
            None => {
                let msg = match &self.status {
                    Status::Loading => "loading…".to_string(),
                    Status::NoCreds => "no claude credentials found".to_string(),
                    Status::Stale(r) => r.clone(),
                    Status::Ok => "no usage data".to_string(),
                };
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(msg, theme::dim()))),
                    chunks[2],
                );
                return;
            }
        };

        let stale = matches!(self.status, Status::Stale(_));
        let bar_w = (area.width as usize).saturating_sub(34).clamp(6, 40);
        let now_s = now_ms() / 1000;

        let mut lines: Vec<Line> = Vec::new();
        if let Status::Stale(r) = &self.status {
            lines.push(Line::from(Span::styled(
                format!("stale · {r}"),
                theme::dim(),
            )));
        }
        for w in &snap.windows {
            let label = match w.kind {
                WindowKind::SevenDayOpus | WindowKind::SevenDaySonnet => {
                    format!("  {}", w.kind.label())
                }
                _ => w.kind.label().to_string(),
            };
            let bar = bar_string(w.utilization, bar_w);
            let bar_style = if stale {
                theme::dim()
            } else {
                Style::default().fg(util_color(w.utilization))
            };
            let pct_style = if stale { theme::dim() } else { theme::historical() };
            let resets = w
                .resets_at
                .map(|r| format!("   resets {}", fmt_reset(r - now_s)))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(format!("{label:<9}"), theme::dim()),
                Span::styled(bar, bar_style),
                Span::styled(format!(" {:>3.0}%", w.utilization), pct_style),
                Span::styled(resets, theme::dim()),
            ]));
        }
        f.render_widget(Paragraph::new(lines), chunks[2]);
    }
}
