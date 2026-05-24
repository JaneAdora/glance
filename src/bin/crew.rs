//! Standalone `crew` launcher: browse background Claude Code sessions; d drops
//! back in (prints `cd … && claude --resume … --dangerously-skip-permissions`
//! for `eval "$(crew)"` and copies it); c copies to the clipboard.
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use glance::clip;
use glance::crew::{CrewAction, CrewCore};
use glance::theme;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SEP: &str = "  │  ";
const HELP: &str = "\
crew :: background Claude Code session view

USAGE:
  crew             Browse background sessions (interactive TTY required).
  crew --help      Print this message.
  crew --version   Print version.

KEYS: d drop-in (resume --dangerously-skip-permissions) · c copy · enter detail · f live · q quit.
Use as: eval \"$(crew)\"   (d prints the resume command on exit)
";

enum RunOutcome {
    Quit,
    PrintAndExit(String),
}

fn main() -> Result<()> {
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("crew {VERSION}");
                return Ok(());
            }
            other => {
                eprintln!("crew: unknown arg: {other}\n\nTry: crew --help");
                std::process::exit(2);
            }
        }
    }

    let mut core = CrewCore::new();

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle("crew"),
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let outcome = run(&mut terminal, &mut core);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;

    match outcome? {
        RunOutcome::Quit => Ok(()),
        RunOutcome::PrintAndExit(cmd) => {
            println!("{cmd}");
            Ok(())
        }
    }
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    core: &mut CrewCore,
) -> Result<RunOutcome> {
    let mut last = Instant::now();
    loop {
        if last.elapsed() >= Duration::from_millis(2000) {
            core.tick();
            last = Instant::now();
        }

        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
            core.render(f, chunks[0]);
            let mut foot = vec![
                Span::styled("d", theme::pane_header_focused()),
                Span::raw(" drop-in"),
                Span::styled(SEP, theme::dim()),
                Span::styled("c", theme::pane_header_focused()),
                Span::raw(" copy"),
                Span::styled(SEP, theme::dim()),
                Span::styled("enter", theme::pane_header_focused()),
                Span::raw(" detail"),
                Span::styled(SEP, theme::dim()),
                Span::styled("f", theme::pane_header_focused()),
                Span::raw(" live"),
                Span::styled(SEP, theme::dim()),
                Span::styled("q", theme::pane_header_focused()),
                Span::raw(" quit"),
            ];
            if let Some(t) = core.current_toast() {
                foot.push(Span::styled(format!("   {t}"), theme::status()));
            }
            f.render_widget(Paragraph::new(Line::from(foot)), chunks[1]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(RunOutcome::Quit);
                }
                if key.code == KeyCode::Char('q') && !core.show_detail {
                    return Ok(RunOutcome::Quit);
                }
                match core.handle_key(key) {
                    CrewAction::None => {}
                    CrewAction::Copy { command } => {
                        clip::copy(&command);
                    }
                    CrewAction::Drop { command, .. } => {
                        clip::copy(&command);
                        return Ok(RunOutcome::PrintAndExit(command));
                    }
                }
            }
        }
    }
}
