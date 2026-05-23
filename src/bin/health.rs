//! Standalone `health` tracker: single-surface tile with view switching and
//! inline logging. Wraps glance::health::HealthCore.
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use glance::health::HealthCore;
use glance::{brightness, theme};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SEP: &str = "  │  ";
const HELP: &str = "\
health :: configurable goals tracker

USAGE:
  health             Launch the tracker (interactive TTY required).
  health --help      Print this message.
  health --version   Print version.

CONFIG:
  ~/.config/glance/health.toml          activities + daily goals (edit freely)
  ~/.local/share/glance/health.jsonl    append-only history

KEYS: v cycle views · j/k focus · +/- log focused · L bulk-log · [ ] brightness · q quit.
";

fn main() -> Result<()> {
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("health {VERSION}");
                return Ok(());
            }
            other => {
                eprintln!("health: unknown arg: {other}\n\nTry: health --help");
                std::process::exit(2);
            }
        }
    }

    let mut core = HealthCore::new();

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle("health"),
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let res = run(&mut terminal, &mut core);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;
    res
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    core: &mut HealthCore,
) -> Result<()> {
    let mut last = Instant::now();
    loop {
        if last.elapsed() >= Duration::from_millis(1000) {
            core.tick();
            last = Instant::now();
        }

        terminal.draw(|f| {
            let area = f.area();
            let t = if core.current_toast().is_some() { 1 } else { 0 };
            let chunks = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1 + t),
            ])
            .split(area);

            let header = Line::from(vec![
                Span::styled(" health ", theme::pane_header_focused()),
                Span::styled(format!("· {} ", core.view.label()), theme::dim()),
            ]);
            f.render_widget(Paragraph::new(header), chunks[0]);

            let body = chunks[1];
            let padded = if body.width > 2 {
                ratatui::layout::Rect {
                    x: body.x + 1,
                    y: body.y,
                    width: body.width - 2,
                    height: body.height,
                }
            } else {
                body
            };
            core.render(f, padded);

            let mut lines = vec![footer_line()];
            if let Some(msg) = core.current_toast() {
                lines.push(Line::from(Span::styled(msg.to_string(), theme::status())));
            }
            f.render_widget(Paragraph::new(lines), chunks[2]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(());
                }
                if core.is_capturing() {
                    core.handle_key(key);
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('[') => {
                        brightness::dim();
                    }
                    KeyCode::Char(']') => {
                        brightness::brighten();
                    }
                    _ => {
                        core.handle_key(key);
                    }
                }
            }
        }
    }
}

fn footer_line() -> Line<'static> {
    Line::from(vec![
        Span::styled("v", theme::pane_header_focused()),
        Span::raw(" views"),
        Span::styled(SEP, theme::dim()),
        Span::styled("j/k", theme::pane_header_focused()),
        Span::raw(" focus"),
        Span::styled(SEP, theme::dim()),
        Span::styled("+/-", theme::pane_header_focused()),
        Span::raw(" log"),
        Span::styled(SEP, theme::dim()),
        Span::styled("L", theme::pane_header_focused()),
        Span::raw(" bulk"),
        Span::styled(SEP, theme::dim()),
        Span::styled("[ ]", theme::pane_header_focused()),
        Span::raw(" bright"),
        Span::styled(SEP, theme::dim()),
        Span::styled("q", theme::pane_header_focused()),
        Span::raw(" quit"),
    ])
}
