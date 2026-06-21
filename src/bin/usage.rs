//! Standalone `usage` cockpit: the glance usage panel running full-screen as
//! its own binary. Shows live Claude limit gauges (session / weekly / per
//! model). q quits.
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use glance::panels::usage::UsagePanel;
use glance::panels::Panel;
use glance::{brightness, theme};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SEP: &str = "  ·  ";
const HELP: &str = "\
usage :: live Claude limit gauges (standalone)

USAGE:
  usage              Launch the gauges (interactive TTY required).
  usage --help       Print this message.
  usage --version    Print version.

Reads the local Claude OAuth token (~/.claude/.credentials.json, read-only)
and shows the same limit data as the Claude Code /usage view: session (5h),
weekly, and per-model windows.

KEYS: [ ] brightness · q quit.
";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("usage {VERSION}");
        return Ok(());
    }
    if let Some(other) = args.first() {
        eprintln!("usage: unknown arg: {other}\n\nTry: usage --help");
        std::process::exit(2);
    }

    let mut panel = UsagePanel::new();

    suite_term::panic::install_panic_hook();
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle("usage"),
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let res = run(&mut terminal, &mut panel);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;
    res
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    panel: &mut UsagePanel,
) -> Result<()> {
    let mut last = Instant::now();
    panel.tick(); // prime so the first frame kicks a fetch
    loop {
        if last.elapsed() >= Duration::from_millis(500) {
            panel.tick();
            last = Instant::now();
        }

        terminal.draw(|f| {
            let chunks =
                Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(f.area());
            panel.render(f, chunks[0]);
            f.render_widget(Paragraph::new(footer_line()), chunks[1]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(());
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
                        panel.handle_key(key);
                    }
                }
            }
        }
    }
}

fn footer_line() -> Line<'static> {
    Line::from(vec![
        Span::styled(" [ ]", theme::pane_header_focused()),
        Span::raw(" bright"),
        Span::styled(SEP, theme::dim()),
        Span::styled("q", theme::pane_header_focused()),
        Span::raw(" quit"),
    ])
}
