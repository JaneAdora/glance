//! Background Claude Code session view (crew). Shared core for the standalone
//! binary and the glance panel.
pub mod job;

use crate::theme;
use crossterm::event::{KeyCode, KeyEvent};
use job::Job;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub enum CrewAction {
    None,
    /// `d` — drop in: full shell command, plus parts for a tmux spawn.
    Drop { command: String, cwd: Option<String>, claude: String },
    /// `c` — copy the resume command to the clipboard.
    Copy { command: String },
}

pub struct CrewCore {
    pub jobs: Vec<Job>,
    pub focus: usize,
    pub filter_live: bool,
    pub show_detail: bool,
    toast: Option<(String, Instant)>,
}

impl Default for CrewCore {
    fn default() -> Self {
        Self::new()
    }
}

impl CrewCore {
    pub fn new() -> Self {
        Self {
            jobs: job::load_jobs(),
            focus: 0,
            filter_live: false,
            show_detail: false,
            toast: None,
        }
    }

    pub fn tick(&mut self) {
        let focused = self
            .visible_indices()
            .get(self.focus)
            .and_then(|&i| self.jobs.get(i))
            .map(|j| j.short.clone());
        self.jobs = job::load_jobs();
        if let Some(short) = focused {
            if let Some(pos) = self.visible_indices().iter().position(|&i| self.jobs[i].short == short) {
                self.focus = pos;
            }
        }
        self.clamp_focus();
    }

    fn visible_indices(&self) -> Vec<usize> {
        self.jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| !self.filter_live || j.is_live())
            .map(|(i, _)| i)
            .collect()
    }

    fn clamp_focus(&mut self) {
        let n = self.visible_indices().len();
        if n == 0 {
            self.focus = 0;
        } else if self.focus >= n {
            self.focus = n - 1;
        }
    }

    fn focused_job(&self) -> Option<&Job> {
        let vis = self.visible_indices();
        vis.get(self.focus).and_then(|&i| self.jobs.get(i))
    }

    fn set_toast(&mut self, m: impl Into<String>) {
        self.toast = Some((m.into(), Instant::now()));
    }

    pub fn current_toast(&self) -> Option<&str> {
        self.toast.as_ref().and_then(|(m, t)| {
            if t.elapsed().as_secs() < 3 {
                Some(m.as_str())
            } else {
                None
            }
        })
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> CrewAction {
        if self.show_detail {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                self.show_detail = false;
            }
            return CrewAction::None;
        }
        let n = self.visible_indices().len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    self.focus = (self.focus + 1) % n;
                }
                CrewAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if n > 0 {
                    self.focus = if self.focus == 0 { n - 1 } else { self.focus - 1 };
                }
                CrewAction::None
            }
            KeyCode::Char('f') => {
                self.filter_live = !self.filter_live;
                self.clamp_focus();
                self.set_toast(if self.filter_live { "filter: live" } else { "filter: all" });
                CrewAction::None
            }
            KeyCode::Enter => {
                if self.focused_job().is_some() {
                    self.show_detail = true;
                }
                CrewAction::None
            }
            KeyCode::Char('d') => match self.focused_job() {
                Some(j) => {
                    let (cwd, claude) = j.resume_parts();
                    CrewAction::Drop { command: j.resume_command(), cwd, claude }
                }
                None => CrewAction::None,
            },
            KeyCode::Char('c') => match self.focused_job() {
                Some(j) => CrewAction::Copy { command: j.resume_command() },
                None => CrewAction::None,
            },
            _ => CrewAction::None,
        }
    }

    fn state_glyph(state: &str) -> (&'static str, Style) {
        match state {
            "working" => ("●", theme::now()),
            "done" => ("✓", theme::historical()),
            "stopped" => ("⏹", theme::dim()),
            "blocked" => ("◍", Style::default().fg(theme::amber())),
            "failed" => ("✗", theme::alert()),
            _ => ("·", theme::dim()),
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let vis = self.visible_indices();
        let total = self.jobs.len();
        let live = self.jobs.iter().filter(|j| j.is_live()).count();

        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);

        let mut head = vec![
            Span::styled(" claude bg sessions ", theme::pane_header_focused()),
            Span::styled(format!("  {live} live · {total} total"), theme::dim()),
        ];
        if self.filter_live {
            head.push(Span::styled("  [live]", theme::now()));
        }
        f.render_widget(Paragraph::new(Line::from(head)), chunks[0]);

        if vis.is_empty() {
            f.render_widget(crate::widgets::empty("no background sessions"), chunks[1]);
            return;
        }

        let now = jiff::Timestamp::now();
        let mut lines: Vec<Line> = Vec::new();
        for (row, &ji) in vis.iter().enumerate() {
            let j = &self.jobs[ji];
            let (glyph, gstyle) = Self::state_glyph(&j.state);
            let marker = if row == self.focus { "▸ " } else { "  " };
            let name = j.display_name();
            let inflight = if j.in_flight > 0 { format!("{}▸ ", j.in_flight) } else { String::new() };
            let name_style = if row == self.focus { theme::active_row() } else { theme::pane_header() };
            lines.push(Line::from(vec![
                Span::styled(marker, theme::dim()),
                Span::styled(format!("{glyph} "), gstyle),
                Span::styled(format!("{:<22}", truncate(&name, 22)), name_style),
                Span::styled(format!("{:<8}", truncate(&j.state, 8)), theme::dim()),
                Span::styled(format!("{:<7}", truncate(&j.tempo, 7)), theme::dim()),
                Span::styled(inflight, theme::now()),
                Span::styled(format!("{:>4}  ", j.age(now)), theme::historical()),
                Span::styled(truncate(&j.detail, 40), theme::dim()),
            ]));
        }
        f.render_widget(Paragraph::new(lines), chunks[1]);

        if self.show_detail {
            self.render_detail(f, area);
        }
    }

    fn render_detail(&self, f: &mut Frame, area: Rect) {
        let Some(j) = self.focused_job() else { return };
        let rect = crate::app::centered_rect(area, 70, 70);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::pane_header_focused())
            .title(Line::from(Span::styled(
                format!(" {} ", j.display_name()),
                theme::pane_header_focused(),
            )));
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        let lines = vec![
            Line::from(vec![
                Span::styled("state ", theme::dim()),
                Span::styled(format!("{}  ", j.state), theme::active_row()),
                Span::styled("tempo ", theme::dim()),
                Span::styled(format!("{}  ", j.tempo), theme::historical()),
                Span::styled(format!("inflight {}", j.in_flight), theme::dim()),
            ]),
            Line::from(vec![Span::styled("cwd  ", theme::dim()), Span::raw(j.cwd.clone())]),
            Line::from(vec![Span::styled("id   ", theme::dim()), Span::raw(j.resume_session_id.clone())]),
            Line::from(vec![Span::styled("updated ", theme::dim()), Span::raw(j.updated_at.clone())]),
            Line::from(""),
            Line::from(Span::styled("intent", theme::pane_header())),
            Line::from(j.intent.clone()),
            Line::from(""),
            Line::from(Span::styled("detail", theme::pane_header())),
            Line::from(j.detail.clone()),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn core_with(jobs: Vec<Job>) -> CrewCore {
        CrewCore { jobs, focus: 0, filter_live: false, show_detail: false, toast: None }
    }
    fn job(short: &str, state: &str) -> Job {
        Job { short: short.into(), state: state.into(), resume_session_id: format!("{short}-id"), cwd: "/w".into(), ..Default::default() }
    }

    #[test]
    fn d_returns_drop_with_dangerous_command() {
        let mut c = core_with(vec![job("a", "working")]);
        match c.handle_key(key('d')) {
            CrewAction::Drop { command, claude, cwd } => {
                assert!(command.contains("--dangerously-skip-permissions"));
                assert!(command.starts_with("cd '/w' && "));
                assert_eq!(claude, "claude --resume a-id --dangerously-skip-permissions");
                assert_eq!(cwd, Some("/w".to_string()));
            }
            other => panic!("expected Drop, got {other:?}"),
        }
    }

    #[test]
    fn c_returns_copy() {
        let mut c = core_with(vec![job("a", "done")]);
        assert_eq!(
            c.handle_key(key('c')),
            CrewAction::Copy { command: "cd '/w' && claude --resume a-id --dangerously-skip-permissions".into() }
        );
    }

    #[test]
    fn f_toggles_live_filter_and_shrinks_visible() {
        let mut c = core_with(vec![job("a", "working"), job("b", "done")]);
        assert_eq!(c.visible_indices().len(), 2);
        c.handle_key(key('f'));
        assert!(c.filter_live);
        assert_eq!(c.visible_indices().len(), 1);
    }

    #[test]
    fn enter_toggles_detail() {
        let mut c = core_with(vec![job("a", "done")]);
        c.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(c.show_detail);
        c.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!c.show_detail);
    }

    #[test]
    fn renders_name_and_glyph() {
        let c = core_with(vec![Job { name: "R-Suite".into(), state: "working".into(), ..Default::default() }]);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 12)).unwrap();
        let area = Rect::new(0, 0, 100, 12);
        term.draw(|f| c.render(f, area)).unwrap();
        let buf = term.backend().buffer();
        let s: String = (0..buf.area().height)
            .flat_map(|y| (0..buf.area().width).map(move |x| (x, y)))
            .map(|(x, y)| buf[(x, y)].symbol().to_string())
            .collect();
        assert!(s.contains("R-Suite"));
        assert!(s.contains("bg sessions"));
    }
}
