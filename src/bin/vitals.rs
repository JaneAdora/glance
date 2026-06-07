//! Standalone `vitals` cockpit: a single-screen hardware dashboard showing the
//! key vitals (CPU / RAM / GPU / hottest temp) all at once, with a big
//! color-coded readout. Detail grid is added in a later step. q quits.
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use glance::panels::cpu::CpuPanel;
use glance::panels::disk::DiskPanel;
use glance::panels::fans::FansPanel;
use glance::panels::gpu::GpuPanel;
use glance::panels::io::IoPanel;
use glance::panels::mem::MemPanel;
use glance::panels::net::NetPanel;
use glance::panels::temp::TempPanel;
use glance::panels::Panel;
use glance::vitals::{choose_mode, combine_temp, status_line, Mode, Status, Vitals};
use glance::{brightness, theme};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HELP: &str = "\
vitals :: single-screen hardware cockpit

USAGE:
  vitals             Launch the dashboard (interactive TTY required).
  vitals --help      Print this message.
  vitals --version   Print version.

SHOWS: CPU / RAM / GPU / hottest-temp at a glance, plus detail panels
(cpu, mem, gpu, thermals, disk, net, io) when the terminal is large enough.
Alarms turn magenta.

KEYS: [ ] brightness · q quit.
";

/// Owns one concrete instance of each hardware panel. Ticked together; read for
/// the vitals row and rendered into the grid.
struct Cockpit {
    cpu: CpuPanel,
    mem: MemPanel,
    gpu: GpuPanel,
    temp: TempPanel,
    fans: FansPanel,
    disk: DiskPanel,
    net: NetPanel,
    io: IoPanel,
}

impl Cockpit {
    fn new() -> Self {
        Self {
            cpu: CpuPanel::new(),
            mem: MemPanel::new(),
            gpu: GpuPanel::new(),
            temp: TempPanel::new(),
            fans: FansPanel::new(),
            disk: DiskPanel::new(),
            net: NetPanel::new(),
            io: IoPanel::new(),
        }
    }

    fn tick_all(&mut self) {
        self.cpu.tick();
        self.mem.tick();
        self.gpu.tick();
        self.temp.tick();
        self.fans.tick();
        self.disk.tick();
        self.net.tick();
        self.io.tick();
    }

    /// Snapshot the current readings for the vitals row.
    fn read_vitals(&self) -> Vitals {
        Vitals {
            cpu: Some(self.cpu.overall_pct()),
            ram: Some(self.mem.used_pct()),
            gpu: self.gpu.util(),
            temp: combine_temp(self.temp.hottest(), self.gpu.temp()),
        }
    }
}

fn metric_style(status: Status) -> Style {
    match status {
        Status::Alarm => Style::default().fg(theme::magenta()).add_modifier(Modifier::BOLD),
        Status::Normal => Style::default().fg(theme::pink()).add_modifier(Modifier::BOLD),
        Status::Unknown => theme::dim(),
    }
}

fn fmt_pct(v: Option<u16>) -> String {
    match v {
        Some(p) => format!("{p}%"),
        None => "--".to_string(),
    }
}

fn fmt_temp(v: Option<f64>) -> String {
    match v {
        Some(t) => format!("{:.0}°C", t),
        None => "--".to_string(),
    }
}

/// Render the four big readouts (line 1) and the status line (line 2) into the
/// top two rows of `area`.
fn render_vitals_row(f: &mut Frame, area: Rect, v: &Vitals, gpu_vram: Option<(u64, u64)>) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    let vram = match gpu_vram {
        Some((used, total)) => {
            format!(" {:.1}/{:.1}G", used as f64 / 1024.0, total as f64 / 1024.0)
        }
        None => String::new(),
    };

    let sep = "    ";
    let line = Line::from(vec![
        Span::styled(" CPU ", theme::dim()),
        Span::styled(fmt_pct(v.cpu), metric_style(v.cpu_status())),
        Span::raw(sep),
        Span::styled("RAM ", theme::dim()),
        Span::styled(fmt_pct(v.ram), metric_style(v.ram_status())),
        Span::raw(sep),
        Span::styled("GPU ", theme::dim()),
        Span::styled(fmt_pct(v.gpu), metric_style(v.gpu_status())),
        Span::styled(vram, theme::dim()),
        Span::raw(sep),
        Span::styled("TEMP ", theme::dim()),
        Span::styled(fmt_temp(v.temp), metric_style(v.temp_status())),
    ]);
    f.render_widget(Paragraph::new(line), rows[0]);

    let status = status_line(v);
    let status_style = if v.any_alarm() {
        Style::default().fg(theme::magenta())
    } else {
        theme::status()
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(format!(" {status}"), status_style))),
        rows[1],
    );
}

/// Render the 3x2 detail grid: [cpu | mem] / [gpu | thermals] / [disk | net+io].
/// The thermals and net+io cells stack two panels vertically.
fn render_grid(f: &mut Frame, area: Rect, c: &Cockpit) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(area);

    let split_cols = |r: Rect| {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(r)
    };
    let split_stack = |r: Rect| {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(r)
    };

    let r0 = split_cols(rows[0]);
    c.cpu.render(f, r0[0]);
    c.mem.render(f, r0[1]);

    let r1 = split_cols(rows[1]);
    c.gpu.render(f, r1[0]);
    let thermals = split_stack(r1[1]);
    c.temp.render(f, thermals[0]);
    c.fans.render(f, thermals[1]);

    let r2 = split_cols(rows[2]);
    c.disk.render(f, r2[0]);
    let netio = split_stack(r2[1]);
    c.net.render(f, netio[0]);
    c.io.render(f, netio[1]);
}

fn draw(f: &mut Frame, c: &Cockpit) {
    let area = f.area();
    let v = c.read_vitals();
    let gpu_vram = c.gpu.vram();
    match choose_mode(area.width, area.height) {
        Mode::Compact => {
            render_vitals_row(f, area, &v, gpu_vram);
        }
        Mode::Full => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2), // vitals row (2 lines)
                    Constraint::Length(1), // separator
                    Constraint::Min(3),    // detail grid
                ])
                .split(area);
            render_vitals_row(f, chunks[0], &v, gpu_vram);
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "─".repeat(area.width as usize),
                    theme::dim(),
                ))),
                chunks[1],
            );
            render_grid(f, chunks[2], c);
        }
    }
}

fn main() -> Result<()> {
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("vitals {VERSION}");
                return Ok(());
            }
            other => {
                eprintln!("vitals: unknown arg: {other}\n\nTry: vitals --help");
                std::process::exit(2);
            }
        }
    }

    let mut cockpit = Cockpit::new();

    // Restore the terminal on panic (before entering raw mode + alt screen).
    suite_term::panic::install_panic_hook();
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle("vitals"),
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let res = run(&mut terminal, &mut cockpit);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;
    res
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    cockpit: &mut Cockpit,
) -> Result<()> {
    let mut last = Instant::now();
    cockpit.tick_all(); // prime so the first frame has data
    loop {
        if last.elapsed() >= Duration::from_millis(1000) {
            cockpit.tick_all();
            last = Instant::now();
        }

        terminal.draw(|f| draw(f, cockpit))?;

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
                    _ => {}
                }
            }
        }
    }
}
