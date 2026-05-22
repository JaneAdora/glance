//! GitHub issues assigned to you, as a BarChart of open-issue counts per repo.
//! Uses `gh search issues --assignee=@me` on a background thread. Graceful if
//! gh is missing/unauthenticated. Mirrors the `prs` panel's fetch scaffold.
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, Borders, Paragraph};
use ratatui::Frame;
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
struct ApiIssue {
    repository: ApiRepo,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiRepo {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

pub struct IssuesPanel {
    /// (repo, count) sorted by count desc; None until the first fetch lands.
    repos: Option<Vec<(String, u64)>>,
    total: u64,
    error: Option<String>,
    last_kick: Option<Instant>,
    rx: mpsc::Receiver<Result<Vec<String>, String>>,
    tx: mpsc::Sender<Result<Vec<String>, String>>,
    inflight: Arc<Mutex<bool>>,
}

impl IssuesPanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            repos: None,
            total: 0,
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

/// One repo name per open assigned issue (so callers can tally counts).
fn fetch() -> Result<Vec<String>, String> {
    let out = Command::new("gh")
        .args([
            "search", "issues", "--assignee=@me", "--state=open",
            "--limit", "100", "--json", "repository",
        ])
        .output()
        .map_err(|e| format!("gh: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "gh: {}",
            String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("error")
        ));
    }
    let issues: Vec<ApiIssue> =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("json: {e}"))?;
    Ok(issues.into_iter().map(|i| i.repository.name_with_owner).collect())
}

/// Count issues per repo, sorted count-desc then name-asc for stable bars.
fn tally(repos: &[String]) -> Vec<(String, u64)> {
    let mut map: HashMap<String, u64> = HashMap::new();
    for r in repos {
        *map.entry(r.clone()).or_insert(0) += 1;
    }
    let mut v: Vec<(String, u64)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

impl Panel for IssuesPanel {
    fn name(&self) -> &str {
        "issues"
    }

    fn refresh_ms(&self) -> u64 {
        5_000
    }

    fn tick(&mut self) {
        while let Ok(r) = self.rx.try_recv() {
            match r {
                Ok(repos) => {
                    self.total = repos.len() as u64;
                    self.repos = Some(tally(&repos));
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
            Span::styled(" issues ", theme::pane_header()),
            Span::styled("GitHub assigned", theme::pane_header_focused()),
            Span::styled("  gh search issues", theme::dim()),
        ]);
        f.render_widget(Paragraph::new(title), chunks[0]);

        let body = chunks[1];
        let repos = match (&self.repos, &self.error) {
            (Some(r), _) => r,
            (None, Some(e)) => {
                f.render_widget(crate::widgets::error(e), body);
                return;
            }
            (None, None) => {
                f.render_widget(crate::widgets::loading("querying GitHub"), body);
                return;
            }
        };

        if repos.is_empty() {
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("✓ ", theme::now()),
                    Span::styled("no issues assigned to you", theme::pane_header_focused()),
                ])),
                body,
            );
            return;
        }

        let palette = [theme::pink(), theme::magenta(), theme::lavender()];
        let max = repos.iter().map(|(_, c)| *c).max().unwrap_or(1);
        let bars: Vec<Bar> = repos
            .iter()
            .take(8)
            .enumerate()
            .map(|(i, (repo, count))| {
                let color = palette[i % palette.len()];
                Bar::default()
                    .value(*count)
                    .text_value(format!("{count}"))
                    .label(Line::from(short_repo(repo)))
                    .style(Style::default().fg(color))
                    .value_style(Style::default().fg(color))
            })
            .collect();

        let heading = format!(" {} open · {} repos ", self.total, repos.len());
        let chart = BarChart::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(theme::dim())
                    .title(Line::from(Span::styled(heading, theme::pane_header()))),
            )
            .bar_width(10)
            .bar_gap(1)
            .max(max)
            .data(BarGroup::default().bars(&bars))
            .label_style(theme::dim());
        f.render_widget(chart, body);
    }
}

/// Short repo label: the part after `owner/`, truncated to fit a bar.
fn short_repo(full: &str) -> String {
    truncate(full.rsplit('/').next().unwrap_or(full), 9)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tally_counts_and_sorts() {
        let repos = vec![
            "a/one".to_string(),
            "b/two".to_string(),
            "a/one".to_string(),
            "a/one".to_string(),
            "b/two".to_string(),
        ];
        let t = tally(&repos);
        assert_eq!(t[0], ("a/one".to_string(), 3));
        assert_eq!(t[1], ("b/two".to_string(), 2));
    }

    #[test]
    fn short_repo_takes_basename() {
        assert_eq!(short_repo("owner/repo"), "repo");
        assert_eq!(short_repo("owner/averylongreponame"), "averylon…");
    }
}
