use anyhow::Result;
use glance::{config, panels};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
glance :: multi-panel system + life dashboard

USAGE:
  glance                 Launch dashboard (interactive TTY required).
  glance --help          Print this message.
  glance --version       Print version.
  glance --list-panels   Print every available panel name.
  glance --write-config  Write a starter ~/.config/glance/panels.toml.

CONFIG:
  ~/.config/glance/panels.toml controls which panels appear and in what
  order. First 9 get keys 1-9, the 10th gets key 0, the rest use n/p.
  Absent config = all panels in the default order.

KEYS: 1-9/0 jump, n/p cycle, r refresh, [ ] brightness, ? help, q quit.
      (health panel: v views, j/k focus, +/- log, L bulk-log.)
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
            "--list-panels" => {
                for name in panels::ALL_PANELS {
                    println!("{name}");
                }
                return Ok(());
            }
            "--write-config" => {
                match config::write_template(panels::ALL_PANELS, panels::DEFAULT_ORDER) {
                    Ok(path) => println!("wrote {}", path.display()),
                    Err(e) => {
                        eprintln!("glance: could not write config: {e}");
                        std::process::exit(1);
                    }
                }
                return Ok(());
            }
            other => {
                eprintln!("glance: unknown arg: {other}\n\nTry: glance --help");
                std::process::exit(2);
            }
        }
    }

    let registry = match config::load_order() {
        Some(order) => panels::registry_from_names(&order),
        None => panels::default_registry(),
    };

    // Restore the terminal on panic so a crash prints its error to stderr
    // instead of leaving the alternate screen up with the message swallowed
    // (which looks like an opaque freeze/crash). Install BEFORE raw mode.
    suite_term::panic::install_panic_hook();

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle("glance"),
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let mut state = glance::app::AppState::new(registry);
    let result = glance::app::run(&mut terminal, &mut state);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;

    result
}
