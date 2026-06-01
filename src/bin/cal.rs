//! Standalone `cal` agenda tile. Pure tile: no PrintAndExit. `space` copies
//! the focused Meet URL; switch to your browser and paste.
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use glance::cal::CalCore;
use glance::theme;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SEP: &str = "  │  ";
const HELP: &str = "\
cal :: Google Calendar agenda (today + week)

USAGE:
  cal           Open the agenda (interactive TTY required).
  cal --help    Print this message.
  cal --version Print version.

KEYS: see ? help inside the TUI.
";

fn main() -> Result<()> {
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--help" | "-h" => { print!("{HELP}"); return Ok(()); }
            "--version" | "-V" => { println!("cal {VERSION}"); return Ok(()); }
            other => {
                eprintln!("cal: unknown arg: {other}\n\nTry: cal --help");
                std::process::exit(2);
            }
        }
    }

    let mut core = CalCore::new();

    // Restore the terminal on panic (before entering raw mode + alt screen).
    suite_term::panic::install_panic_hook();
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle("cal"),
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;
    let outcome = run(&mut terminal, &mut core);
    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;
    outcome
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    core: &mut CalCore,
) -> Result<()> {
    let mut last_tick = Instant::now();
    let mut show_help = false;
    loop {
        if last_tick.elapsed() >= Duration::from_millis(1000) {
            core.tick();
            last_tick = Instant::now();
        }
        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
            core.render(f, chunks[0]);
            render_footer(f, chunks[1], core);
            if show_help {
                render_help(f, area);
            }
        })?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release { continue; }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(());
                }
                if show_help { show_help = false; continue; }
                if core.show_detail {
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')
                        | KeyCode::Char('h') | KeyCode::Left => {
                            core.close_detail();
                            continue;
                        }
                        KeyCode::Char('c') => { let _ = core.copy_detail(); continue; }
                        _ => continue,
                    }
                }
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('?') => show_help = true,
                    KeyCode::Char('j') | KeyCode::Down => core.move_down(),
                    KeyCode::Char('k') | KeyCode::Up => core.move_up(),
                    KeyCode::Char('l') | KeyCode::Right => core.drill_in(),
                    KeyCode::Char('h') | KeyCode::Left => core.drill_out(),
                    KeyCode::Char('o') | KeyCode::Tab | KeyCode::Char(' ') => core.toggle_expand(),
                    KeyCode::Char('y') => { let _ = core.copy_url(); }
                    KeyCode::Char('c') => { let _ = core.copy_detail(); }
                    KeyCode::Char('p') => core.toggle_past(),
                    KeyCode::Char('r') => core.refresh(),
                    KeyCode::Enter => core.toggle_detail(),
                    _ => {}
                }
            }
        }
    }
}

fn render_footer(f: &mut ratatui::Frame, area: ratatui::layout::Rect, core: &CalCore) {
    let mut foot = vec![
        Span::styled("space", theme::pane_header_focused()), Span::raw(" expand"),
        Span::styled(SEP, theme::dim()),
        Span::styled("y", theme::pane_header_focused()), Span::raw(" copy URL"),
        Span::styled(SEP, theme::dim()),
        Span::styled("c", theme::pane_header_focused()), Span::raw(" copy detail"),
        Span::styled(SEP, theme::dim()),
        Span::styled("p", theme::pane_header_focused()), Span::raw(" past"),
        Span::styled(SEP, theme::dim()),
        Span::styled("r", theme::pane_header_focused()), Span::raw(" refresh"),
        Span::styled(SEP, theme::dim()),
        Span::styled("?", theme::pane_header_focused()), Span::raw(" help"),
        Span::styled(SEP, theme::dim()),
        Span::styled("q", theme::pane_header_focused()), Span::raw(" quit"),
    ];
    if let Some(t) = core.current_toast() {
        foot.push(Span::styled(format!("   {t}"), theme::status()));
    }
    f.render_widget(Paragraph::new(Line::from(foot)), area);
}

fn render_help(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    use ratatui::layout::Margin;
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};
    let inner = area.inner(Margin { vertical: 3, horizontal: 6 });
    f.render_widget(Clear, inner);
    let lines = vec![
        Line::from(Span::styled("cal — keybindings", theme::pane_header_focused())),
        Line::from(""),
        Line::from(Span::raw("NAVIGATION")),
        Line::from(Span::raw("  j / ↓     next row")),
        Line::from(Span::raw("  k / ↑     prev row")),
        Line::from(Span::raw("  space / Tab / o   toggle expand on focused day")),
        Line::from(Span::raw("  l / →     drill in (header → event → detail)")),
        Line::from(Span::raw("  h / ←     drill out (detail → event → header)")),
        Line::from(""),
        Line::from(Span::raw("ACTIONS")),
        Line::from(Span::raw("  y         copy focused event's Meet URL")),
        Line::from(Span::raw("  c         copy event detail (paste into a Claude prompt)")),
        Line::from(Span::raw("  Enter     detail modal (alias for l on event)")),
        Line::from(""),
        Line::from(Span::raw("VIEW")),
        Line::from(Span::raw("  p         toggle show past (default: shown ✓ dimmed)")),
        Line::from(Span::raw("  r         force refresh (bypasses 5-min cache)")),
        Line::from(""),
        Line::from(Span::raw("EXIT")),
        Line::from(Span::raw("  q         quit")),
        Line::from(Span::raw("  Esc       cancel mode / close modal")),
        Line::from(""),
        Line::from(Span::styled("Cancelled events hidden. Declined events strikethrough.", theme::dim())),
        Line::from(Span::styled("any key closes this", theme::dim())),
    ];
    let block = Block::default().borders(Borders::ALL).title("help");
    f.render_widget(Paragraph::new(lines).block(block), inner);
}
