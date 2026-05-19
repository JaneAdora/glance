use crate::footer;
use crate::header;
use crate::panels::{self, Panel};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use std::time::{Duration, Instant};

pub struct AppState {
    pub panels: Vec<Box<dyn Panel>>,
    pub current: usize,
    pub last_tick: Vec<Instant>,
    pub transient: Option<(String, Instant)>,
    pub show_help: bool,
}

impl AppState {
    pub fn new(mut panels: Vec<Box<dyn Panel>>) -> Self {
        if panels.is_empty() {
            panic!("glance: no panels registered");
        }
        for p in panels.iter_mut() {
            p.tick();
        }
        let now = Instant::now();
        let last_tick = vec![now; panels.len()];
        Self {
            panels,
            current: 0,
            last_tick,
            transient: None,
            show_help: false,
        }
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.transient = Some((msg.into(), Instant::now()));
    }

    pub fn current_transient(&self) -> Option<&str> {
        let (m, t) = self.transient.as_ref()?;
        if t.elapsed() < Duration::from_secs(3) {
            Some(m.as_str())
        } else {
            None
        }
    }

    fn current_name(&self) -> String {
        self.panels[self.current].name().to_string()
    }

    fn switch_to(&mut self, idx: usize) {
        if idx >= self.panels.len() {
            return;
        }
        self.current = idx;
        let name = self.current_name();
        self.toast(format!("→ {}", name));
        let now = Instant::now();
        if now.duration_since(self.last_tick[idx]) > Duration::from_millis(50) {
            self.panels[idx].tick();
            self.last_tick[idx] = now;
        }
    }
}

pub fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &mut AppState,
) -> Result<()> {
    loop {
        let interval = state.panels[state.current].refresh_ms();
        let due_at = state.last_tick[state.current] + Duration::from_millis(interval);
        let now = Instant::now();
        if now >= due_at {
            state.panels[state.current].tick();
            state.last_tick[state.current] = now;
        }

        terminal.draw(|f| render(f, state))?;

        let now = Instant::now();
        let due_at = state.last_tick[state.current] + Duration::from_millis(interval);
        let wait = due_at.saturating_duration_since(now).min(Duration::from_millis(100));

        if event::poll(wait)? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if handle_key(state, key) {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
    }
}

fn render(f: &mut ratatui::Frame, state: &AppState) {
    let area = f.area();
    let transient_lines = if state.current_transient().is_some() { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1 + transient_lines),
            Constraint::Min(3),
            Constraint::Length(1 + transient_lines),
        ])
        .split(area);

    header::render(
        f,
        chunks[0],
        &state.panels,
        state.current,
        state.current_transient(),
    );

    // Apply 1-col side padding so panel content never touches the screen edge.
    let body_rect = chunks[1];
    let padded = if body_rect.width > 2 {
        ratatui::layout::Rect {
            x: body_rect.x + 1,
            y: body_rect.y,
            width: body_rect.width - 2,
            height: body_rect.height,
        }
    } else {
        body_rect
    };
    state.panels[state.current].render(f, padded);

    footer::render(f, chunks[2], state.current_transient());

    if state.show_help {
        let rect = centered_rect(area, 70, 60);
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(crate::theme::pane_header())
            .title(ratatui::text::Line::from(ratatui::text::Span::styled(
                " help ",
                crate::theme::pane_header_focused(),
            )));
        f.render_widget(ratatui::widgets::Clear, rect);
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        let help = ratatui::widgets::Paragraph::new(HELP_TEXT.to_string());
        f.render_widget(help, inner);
    }
}

fn centered_rect(parent: ratatui::layout::Rect, percent_x: u16, percent_y: u16) -> ratatui::layout::Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(parent);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1])[1]
}

fn handle_key(state: &mut AppState, key: KeyEvent) -> bool {
    if state.show_help {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')) {
            state.show_help = false;
        }
        return false;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        return true;
    }

    match key.code {
        KeyCode::Char('q') => return true,
        KeyCode::Char('?') => state.show_help = true,
        KeyCode::Char('r') => {
            let idx = state.current;
            state.panels[idx].tick();
            state.last_tick[idx] = Instant::now();
            state.toast("refreshed");
        }
        KeyCode::Char('[') => {
            let n = crate::brightness::dim();
            state.toast(format!("brightness {}", n));
        }
        KeyCode::Char(']') => {
            let n = crate::brightness::brighten();
            state.toast(format!("brightness {}", n));
        }
        KeyCode::Char('n') | KeyCode::Right | KeyCode::Tab => {
            let next = (state.current + 1) % state.panels.len();
            state.switch_to(next);
        }
        KeyCode::Char('p') | KeyCode::Left => {
            let prev = if state.current == 0 {
                state.panels.len() - 1
            } else {
                state.current - 1
            };
            state.switch_to(prev);
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            let target = if c == '0' { 9 } else { (c as u8 - b'1') as usize };
            if target < state.panels.len() {
                state.switch_to(target);
            }
        }
        _ => {
            // Unrecognised global key — give the active panel a chance to consume it.
            let idx = state.current;
            let _ = state.panels[idx].handle_key(key);
        }
    }
    false
}

const HELP_TEXT: &str = "\
PANELS
  1-9, 0   jump to panel by slot (slot 0 + extras reachable via n/p)
  n / Tab  next panel
  p        previous panel

VIEW
  r        force-refresh current panel
  [        dim screen
  ]        brighten screen
  ?        toggle help
  q / Esc  quit (Ctrl-C also works)

About: glance is a tile-mode dashboard widget. Each panel runs at its
own refresh rate; only the focused panel ticks. Sibling of wt/recall/roam.
";
