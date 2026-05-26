//! Standalone `tasks` editor for `~/.claude/tasks/`. Aggregate cross-session
//! view + status cycle (space) + create (n) + delete (xx). Persists
//! `~/.config/glance/tasks.toml` on quit (expanded sessions + show_completed).
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use glance::tasks::{Filter, TasksCore};
use glance::theme;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SEP: &str = "  │  ";
const HELP: &str = "\
tasks :: Claude Code task viewer + editor (cross-session)

USAGE:
  tasks            Open the editor (interactive TTY required).
  tasks --help     Print this message.
  tasks --version  Print version.

KEYS:
  space   cycle status (pending → in_progress → completed → pending)
  Tab/o   collapse/expand focused session
  Enter   detail modal
  n       new task in focused session
  xx      delete focused task (within 2s)
  c       toggle show completed
  s       filter to focused session (toggle)
  /       substring filter on subject (Esc clears)
  r       force reload + label cache refresh
  ?       help modal
  q       quit (persists state)
";

#[derive(Default, Serialize, Deserialize)]
struct PersistedState {
    #[serde(default)]
    expanded: Vec<String>,
    #[serde(default)]
    show_completed: bool,
}

fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"));
    base.join("glance").join("tasks.toml")
}

fn load_state() -> PersistedState {
    let p = config_path();
    let Ok(s) = std::fs::read_to_string(&p) else { return PersistedState::default(); };
    toml::from_str(&s).unwrap_or_default()
}

fn save_state(core: &TasksCore) {
    let state = PersistedState {
        expanded: core.expanded.iter().cloned().collect(),
        show_completed: core.show_completed,
    };
    let Ok(text) = toml::to_string(&state) else { return; };
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, text);
}

fn main() -> Result<()> {
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("tasks {VERSION}");
                return Ok(());
            }
            other => {
                eprintln!("tasks: unknown arg: {other}\n\nTry: tasks --help");
                std::process::exit(2);
            }
        }
    }

    let mut core = TasksCore::new();
    // hydrate persisted state
    let persisted = load_state();
    core.show_completed = persisted.show_completed;
    if !persisted.expanded.is_empty() {
        core.expanded.clear();
        for sid in persisted.expanded {
            core.expanded.insert(sid);
        }
    }

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle("tasks"),
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let outcome = run(&mut terminal, &mut core);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;

    save_state(&core);
    outcome
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    core: &mut TasksCore,
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
            render_footer(f, chunks[1], core, show_help);
            if show_help {
                render_help_modal(f, area);
            }
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release { continue; }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(());
                }
                // help modal absorbs all keys
                if show_help {
                    show_help = false;
                    continue;
                }
                // detail modal absorbs Enter/Esc/q
                if core.show_detail {
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                            core.close_detail();
                            continue;
                        }
                        _ => continue,
                    }
                }
                // create mode captures all printable keys
                if core.create_mode.is_some() {
                    match key.code {
                        KeyCode::Esc => core.cancel_create_mode(),
                        KeyCode::Enter => { let _ = core.submit_create(); },
                        KeyCode::Backspace => core.create_buffer_pop(),
                        KeyCode::Char(c) => core.create_buffer_push(c),
                        _ => {}
                    }
                    continue;
                }
                // filter input mode
                if core.filter_input.is_some() {
                    match key.code {
                        KeyCode::Esc => {
                            core.cancel_filter_input();
                            core.filter = Filter::All;
                        }
                        KeyCode::Enter => core.submit_filter(),
                        KeyCode::Backspace => core.filter_buffer_pop(),
                        KeyCode::Char(c) => core.filter_buffer_push(c),
                        _ => {}
                    }
                    continue;
                }
                // normal mode
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('?') => show_help = true,
                    KeyCode::Char('j') | KeyCode::Down => core.move_down(),
                    KeyCode::Char('k') | KeyCode::Up => core.move_up(),
                    KeyCode::Char('o') | KeyCode::Tab => core.toggle_expand(),
                    KeyCode::Char(' ') => { let _ = core.cycle_status(); },
                    KeyCode::Char('n') => core.enter_create_mode(),
                    KeyCode::Char('x') => { let _ = core.arm_or_delete(); },
                    KeyCode::Char('c') => core.toggle_show_completed(),
                    KeyCode::Char('s') => core.toggle_session_filter(),
                    KeyCode::Char('/') => core.enter_filter_input(),
                    KeyCode::Char('r') => core.refresh(),
                    KeyCode::Enter => {
                        // Header focused: toggle expand. Task focused: open detail.
                        if core.focus.task.is_some() {
                            core.toggle_detail();
                        } else {
                            core.toggle_expand();
                        }
                    }
                    KeyCode::Esc => {
                        core.filter = Filter::All;
                    }
                    _ => {}
                }
            }
        }
    }
}

fn render_footer(f: &mut ratatui::Frame, area: ratatui::layout::Rect, core: &TasksCore, _show_help: bool) {
    let mut foot = vec![
        Span::styled("space", theme::pane_header_focused()),
        Span::raw(" cycle"),
        Span::styled(SEP, theme::dim()),
        Span::styled("Tab/o", theme::pane_header_focused()),
        Span::raw(" collapse"),
        Span::styled(SEP, theme::dim()),
        Span::styled("Enter", theme::pane_header_focused()),
        Span::raw(" detail"),
        Span::styled(SEP, theme::dim()),
        Span::styled("n", theme::pane_header_focused()),
        Span::raw(" new"),
        Span::styled(SEP, theme::dim()),
        Span::styled("xx", theme::pane_header_focused()),
        Span::raw(" delete"),
        Span::styled(SEP, theme::dim()),
        Span::styled("/", theme::pane_header_focused()),
        Span::raw(" filter"),
        Span::styled(SEP, theme::dim()),
        Span::styled("?", theme::pane_header_focused()),
        Span::raw(" help"),
        Span::styled(SEP, theme::dim()),
        Span::styled("q", theme::pane_header_focused()),
        Span::raw(" quit"),
    ];
    if let Some(t) = core.current_toast() {
        foot.push(Span::styled(format!("   {t}"), theme::status()));
    }
    f.render_widget(Paragraph::new(Line::from(foot)), area);
}

fn render_help_modal(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    use ratatui::layout::Margin;
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};
    let inner = area.inner(Margin { vertical: 3, horizontal: 6 });
    f.render_widget(Clear, inner);
    let lines = vec![
        Line::from(Span::styled("tasks — keybindings", theme::pane_header_focused())),
        Line::from(""),
        Line::from(Span::raw("NAVIGATION")),
        Line::from(Span::raw("  j / ↓     next row")),
        Line::from(Span::raw("  k / ↑     prev row")),
        Line::from(Span::raw("  Tab / o   expand/collapse session")),
        Line::from(Span::raw("  s         filter to focused session")),
        Line::from(Span::raw("  /         substring filter (Esc clears)")),
        Line::from(""),
        Line::from(Span::raw("EDIT")),
        Line::from(Span::raw("  space     cycle status")),
        Line::from(Span::raw("  n         new task in focused session")),
        Line::from(Span::raw("  xx        delete (press x twice within 2s)")),
        Line::from(""),
        Line::from(Span::raw("VIEW")),
        Line::from(Span::raw("  c         toggle show completed")),
        Line::from(Span::raw("  r         reload + refresh label cache")),
        Line::from(Span::raw("  Enter     detail modal")),
        Line::from(""),
        Line::from(Span::raw("EXIT")),
        Line::from(Span::raw("  q         quit (persists state)")),
        Line::from(Span::raw("  Esc       cancel mode")),
        Line::from(""),
        Line::from(Span::styled("any key closes this", theme::dim())),
    ];
    let block = Block::default().borders(Borders::ALL).title("help");
    f.render_widget(Paragraph::new(lines).block(block), inner);
}
