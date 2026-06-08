//! Standalone `music` cockpit: the glance now-playing / MPRIS panel running
//! full-screen as its own binary. Because it owns the whole keymap (glance
//! reserves the arrow keys for panel navigation), the arrow keys here move
//! between tracks. Controls any MPRIS player via playerctl. q quits.
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use glance::panels::music::MusicPanel;
use glance::panels::Panel;
use glance::{brightness, theme};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SEP: &str = "  ·  ";
const HELP: &str = "\
music :: now-playing + MPRIS controls (standalone)

USAGE:
  music              Launch the player (interactive TTY required).
  music --help       Print this message.
  music --version    Print version.

Controls any MPRIS player via playerctl (Spotify, Chromium, a phone over
kdeconnect, ...). Running standalone frees the arrow keys for track nav.

KEYS: ←/→ prev/next · space play/pause · ↑/↓ volume · ,/. seek · s shuffle ·
      L loop · d device · [ ] brightness · q quit.
";

fn main() -> Result<()> {
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("music {VERSION}");
                return Ok(());
            }
            other => {
                eprintln!("music: unknown arg: {other}\n\nTry: music --help");
                std::process::exit(2);
            }
        }
    }

    let mut panel = MusicPanel::new();

    // Restore the terminal on panic (before entering raw mode + alt screen).
    suite_term::panic::install_panic_hook();
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle("music"),
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
    panel: &mut MusicPanel,
) -> Result<()> {
    let mut last = Instant::now();
    panel.tick(); // prime so the first frame has now-playing data
    loop {
        if last.elapsed() >= Duration::from_millis(1000) {
            panel.tick();
            last = Instant::now();
        }

        terminal.draw(|f| {
            let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(f.area());
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
    // The panel already renders its own keymap hint (space / <> / vol / seek /
    // s / L / d). This footer adds only what is standalone-specific: the freed
    // arrow keys for track nav, plus brightness and quit.
    Line::from(vec![
        Span::styled(" ←/→", theme::pane_header_focused()),
        Span::raw(" prev/next"),
        Span::styled(SEP, theme::dim()),
        Span::styled("[ ]", theme::pane_header_focused()),
        Span::raw(" bright"),
        Span::styled(SEP, theme::dim()),
        Span::styled("q", theme::pane_header_focused()),
        Span::raw(" quit"),
    ])
}
