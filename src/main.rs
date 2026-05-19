mod app;
mod footer;
mod header;
mod layout;
mod panels;
mod theme;

use anyhow::Result;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
glance :: multi-panel system + life dashboard

USAGE:
  glance              Launch dashboard (interactive TTY required).
  glance --help       Print this message.
  glance --version    Print version.

PANELS:
  cpu, mem (more coming)

KEYS: 1-9 jump, n/p cycle, r refresh, ? help, q quit.
";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for a in &args {
        match a.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("glance {VERSION}");
                return Ok(());
            }
            other => {
                eprintln!("glance: unknown arg: {other}\n\nTry: glance --help");
                std::process::exit(2);
            }
        }
    }

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle("glance"),
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let registry = panels::default_registry();
    let mut state = app::AppState::new(registry);

    let result = app::run(&mut terminal, &mut state);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;

    result
}
