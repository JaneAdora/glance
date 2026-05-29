//! Network path to a destination via `mtr --report`. Renders each hop as a row
//! with a latency bar; background thread so the multi-second probe never blocks.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::process::Command;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Hop {
    n: u32,
    host: String,
    loss: f64,
    avg_ms: f64,
}

pub struct TraceroutePanel {
    target: String,
    hops: Option<Vec<Hop>>,
    error: Option<String>,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<Result<Vec<Hop>, String>>,
    tx: mpsc::Sender<Result<Vec<Hop>, String>>,
    inflight: Arc<Mutex<bool>>,
}

impl TraceroutePanel {
    pub fn new() -> Self {
        let target = std::env::var("GLANCE_TRACE_HOST").unwrap_or_else(|_| "1.1.1.1".to_string());
        let (tx, rx) = mpsc::channel();
        Self {
            target,
            hops: None,
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
        let target = self.target.clone();
        thread::spawn(move || {
            let r = run_mtr(&target);
            let _ = tx.send(r);
            if let Ok(mut g) = inflight.lock() {
                *g = false;
            }
        });
        self.last_kick = Some(Instant::now());
    }
}

fn run_mtr(target: &str) -> Result<Vec<Hop>, String> {
    let out = Command::new("mtr")
        .args(["--report", "--report-cycles", "1", "--no-dns", target])
        .output()
        .map_err(|e| format!("mtr: {e}"))?;
    if !out.status.success() {
        return Err(format!("mtr exited {}", out.status));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut hops = Vec::new();
    for line in text.lines() {
        // Lines look like: "  1.|-- 192.168.4.1   0.0%   1   4.9  4.9  4.9  4.9  0.0"
        let l = line.trim();
        if !l.contains("|--") {
            continue;
        }
        let parts: Vec<&str> = l.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }
        // parts[0] = "1.|--", parts[1] = host, then Loss% Snt Last Avg ...
        let n: u32 = parts[0].trim_end_matches(".|--").trim_end_matches('.').parse().unwrap_or(0);
        let host = parts[1].to_string();
        // After host: Loss%, Snt, Last, Avg, Best, Wrst, StDev
        let loss = parts[2].trim_end_matches('%').parse().unwrap_or(0.0);
        let avg = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        hops.push(Hop { n, host, loss, avg_ms: avg });
    }
    if hops.is_empty() {
        return Err("no hops parsed".to_string());
    }
    Ok(hops)
}

impl Panel for TraceroutePanel {
    fn name(&self) -> &str {
        "traceroute"
    }

    fn refresh_ms(&self) -> u64 {
        5_000
    }

    fn tick(&mut self) {
        while let Ok(r) = self.rx.try_recv() {
            match r {
                Ok(h) => {
                    self.hops = Some(h);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
        let stale = match self.last_kick {
            None => true,
            Some(t) => t.elapsed() >= Duration::from_secs(30),
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
            Span::styled(" traceroute ", theme::pane_header()),
            Span::styled(format!("→ {}", self.target), theme::pane_header_focused()),
            Span::styled("   mtr  ($GLANCE_TRACE_HOST)", theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        let body = chunks[1];
        let hops = match (&self.hops, &self.error) {
            (Some(h), _) => h,
            (None, Some(e)) => {
                f.render_widget(crate::widgets::error(e), body);
                return;
            }
            (None, None) => {
                f.render_widget(crate::widgets::loading("tracing route"), body);
                return;
            }
        };

        let max_avg = hops.iter().map(|h| h.avg_ms).fold(1.0_f64, f64::max);
        let mut lines: Vec<Line> = Vec::with_capacity(hops.len());
        let bar_width = (body.width as usize).saturating_sub(40).clamp(4, 60);
        for h in hops {
            let filled = ((h.avg_ms / max_avg) * bar_width as f64).round() as usize;
            let bar: String = "█".repeat(filled.min(bar_width));
            let bar_style = if h.loss > 0.0 {
                theme::alert()
            } else if h.avg_ms > max_avg * 0.6 {
                Style::default().fg(theme::pink())
            } else {
                Style::default().fg(theme::lavender())
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{:>2} ", h.n), theme::dim()),
                Span::styled(format!("{:<16}", truncate(&h.host, 16)), theme::historical()),
                Span::styled(format!("{:>6.1}ms ", h.avg_ms), Style::default().fg(theme::magenta())),
                Span::styled(bar, bar_style),
                if h.loss > 0.0 {
                    Span::styled(format!(" {:.0}% loss", h.loss), theme::alert())
                } else {
                    Span::raw("")
                },
            ]));
        }
        f.render_widget(Paragraph::new(lines), body);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}
