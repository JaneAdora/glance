//! glance panel form of crew. d = spawn the resume in a new tmux window (or copy
//! if not in tmux); c = copy the resume command.
use crate::clip;
use crate::crew::{CrewAction, CrewCore};
use crate::panels::Panel;
use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::process::Command;

pub struct CrewPanel {
    core: CrewCore,
    toast: Option<String>,
}

impl CrewPanel {
    pub fn new() -> Self {
        Self { core: CrewCore::new(), toast: None }
    }
}

impl Panel for CrewPanel {
    fn name(&self) -> &str {
        "crew"
    }
    fn refresh_ms(&self) -> u64 {
        2_000
    }
    fn tick(&mut self) {
        self.core.tick();
    }
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match self.core.handle_key(key) {
            CrewAction::None => false,
            CrewAction::Copy { command } => {
                clip::copy(&command);
                self.toast = Some("copied resume command".into());
                true
            }
            CrewAction::Drop { command, cwd, claude } => {
                if std::env::var("TMUX").is_ok() {
                    let mut c = Command::new("tmux");
                    c.arg("new-window");
                    if let Some(dir) = &cwd {
                        c.arg("-c").arg(dir);
                    }
                    for part in claude.split(' ') {
                        c.arg(part);
                    }
                    let ok = c.status().map(|s| s.success()).unwrap_or(false);
                    self.toast = Some(if ok { "opened in new tmux window".into() } else { "tmux failed".into() });
                } else {
                    clip::copy(&command);
                    self.toast = Some("no tmux: copied instead".into());
                }
                true
            }
        }
    }
    fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
        self.core.render(f, chunks[0]);
        let toast = self.core.current_toast().map(|s| s.to_string()).or_else(|| self.toast.clone());
        let mut foot = vec![
            Span::styled("d", crate::theme::pane_header_focused()),
            Span::raw(" drop-in  "),
            Span::styled("c", crate::theme::pane_header_focused()),
            Span::raw(" copy  "),
            Span::styled("enter", crate::theme::pane_header_focused()),
            Span::raw(" detail  "),
            Span::styled("f", crate::theme::pane_header_focused()),
            Span::raw(" live"),
        ];
        if let Some(t) = toast {
            foot.push(Span::styled(format!("   {t}"), crate::theme::status()));
        }
        f.render_widget(Paragraph::new(Line::from(foot)), chunks[1]);
    }
}
