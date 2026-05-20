//! GitHub PR triage: your open PRs + review-requested, grouped by repo.
//! Uses `gh search prs` on a background thread. Graceful if gh is missing/unauth.
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
struct ApiPr {
    title: String,
    repository: ApiRepo,
    #[serde(default)]
    url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiRepo {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

#[derive(Debug, Clone)]
pub struct Pr {
    pub title: String,
    pub repo: String,
    pub kind: PrKind,
}

#[derive(Debug, Clone, Copy)]
pub enum PrKind {
    Mine,
    ReviewRequested,
}

pub struct PrsPanel {
    prs: Option<Vec<Pr>>,
    error: Option<String>,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<Result<Vec<Pr>, String>>,
    tx: mpsc::Sender<Result<Vec<Pr>, String>>,
    inflight: Arc<Mutex<bool>>,
}

impl PrsPanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            prs: None,
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
        thread::spawn(move || {
            let r = fetch();
            let _ = tx.send(r);
            if let Ok(mut g) = inflight.lock() {
                *g = false;
            }
        });
        self.last_kick = Some(Instant::now());
    }
}

fn search(args: &[&str]) -> Result<Vec<ApiPr>, String> {
    let out = Command::new("gh")
        .args(args)
        .output()
        .map_err(|e| format!("gh: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "gh: {}",
            String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("error")
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("json: {e}"))
}

fn fetch() -> Result<Vec<Pr>, String> {
    let mut prs = Vec::new();
    let mine = search(&[
        "search", "prs", "--author=@me", "--state=open", "--limit", "20",
        "--json", "title,repository,url",
    ])?;
    for p in mine {
        prs.push(Pr {
            title: p.title,
            repo: p.repository.name_with_owner,
            kind: PrKind::Mine,
        });
    }
    // Review-requested is best-effort; ignore errors so the panel still shows "mine".
    if let Ok(rr) = search(&[
        "search", "prs", "--review-requested=@me", "--state=open", "--limit", "20",
        "--json", "title,repository,url",
    ]) {
        for p in rr {
            prs.push(Pr {
                title: p.title,
                repo: p.repository.name_with_owner,
                kind: PrKind::ReviewRequested,
            });
        }
    }
    Ok(prs)
}

impl Panel for PrsPanel {
    fn name(&self) -> &str {
        "prs"
    }

    fn refresh_ms(&self) -> u64 {
        5_000
    }

    fn tick(&mut self) {
        while let Ok(r) = self.rx.try_recv() {
            match r {
                Ok(p) => {
                    self.prs = Some(p);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
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
            Span::styled(" prs ", theme::pane_header()),
            Span::styled("GitHub triage", theme::pane_header_focused()),
            Span::styled("  gh search prs", theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        let body = chunks[1];
        let prs = match (&self.prs, &self.error) {
            (Some(p), _) => p,
            (None, Some(e)) => {
                f.render_widget(crate::widgets::error(e), body);
                return;
            }
            (None, None) => {
                f.render_widget(crate::widgets::loading("querying GitHub"), body);
                return;
            }
        };

        if prs.is_empty() {
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("✓ ", theme::now()),
                    Span::styled("inbox zero — no open PRs", theme::pane_header_focused()),
                ])),
                body,
            );
            return;
        }

        let mine = prs.iter().filter(|p| matches!(p.kind, PrKind::Mine)).count();
        let rr = prs.iter().filter(|p| matches!(p.kind, PrKind::ReviewRequested)).count();

        let mut lines: Vec<Line> = vec![Line::from(vec![
            Span::styled(format!("◆ {mine} yours"), Style::default().fg(theme::pink())),
            Span::styled("    ", theme::dim()),
            Span::styled(format!("● {rr} to review"), Style::default().fg(theme::magenta())),
        ])];
        lines.push(Line::from(""));

        for p in prs {
            let (glyph, style) = match p.kind {
                PrKind::Mine => ("◆", Style::default().fg(theme::pink())),
                PrKind::ReviewRequested => ("●", Style::default().fg(theme::magenta())),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{glyph} "), style),
                Span::styled(format!("{:<28} ", truncate(&p.repo, 28)), theme::dim()),
                Span::styled(truncate(&p.title, 60), theme::historical()),
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
