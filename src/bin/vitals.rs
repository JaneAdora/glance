//! Standalone `vitals` cockpit: a single-screen hardware dashboard for a quick
//! "is my hardware OK right now?" check, designed for a phone/SSH terminal. A
//! color-coded vitals row (CPU / RAM / GPU / hottest temp) stays pinned at the
//! top; below it the detail panels stack in one full-width, scrollable column.
//! q quits.
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
use glance::vitals::{combine_temp, status_line, Status, Vitals};
use glance::{brightness, theme};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
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

A color-coded vitals row (CPU / RAM / GPU / hottest temp) stays pinned at the
top; the detail panels (cpu, mem, gpu, thermals, disk, net, io) stack below in
one scrollable column. Alarms turn magenta. Built for a quick check from a
phone / SSH terminal.

KEYS: j/k or up/down scroll · space/b page · g/G top/bottom · [ ] brightness · q quit.
";

/// Owns one concrete instance of each hardware panel. Ticked together; read for
/// the vitals row and rendered into the scrollable column.
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

    /// Per-panel row heights for the scrollable column, in render order. CPU is
    /// sized to show every core; the rest are fixed generous heights (panels
    /// clip to their row if a machine has more items than fit).
    fn panel_heights(&self) -> Vec<u16> {
        let cpu_h = (self.cpu.core_count() as u16).saturating_add(2 + 9);
        // cpu, mem, gpu, temp, fans, disk, net, io. mem=17 / gpu=18 leave
        // room for their Top-processes tables (RAM / VRAM) in the column.
        vec![cpu_h, 17, 18, 9, 5, 16, 12, 4]
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

/// Render the full detail column into an off-screen buffer of size
/// `width x content_h`, one panel per row using `heights`. The caller blits the
/// visible slice into the real frame, which gives smooth line-by-line scrolling
/// regardless of how tall the column is.
fn render_column_offscreen(c: &Cockpit, width: u16, heights: &[u16], content_h: u16) -> Buffer {
    let backend = TestBackend::new(width.max(1), content_h.max(1));
    let mut term = ratatui::Terminal::new(backend).expect("offscreen terminal");
    term.draw(|tf| {
        let area = tf.area();
        let constraints: Vec<Constraint> =
            heights.iter().map(|h| Constraint::Length(*h)).collect();
        let slots = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);
        c.cpu.render(tf, slots[0]);
        c.mem.render(tf, slots[1]);
        c.gpu.render(tf, slots[2]);
        c.temp.render(tf, slots[3]);
        c.fans.render(tf, slots[4]);
        c.disk.render(tf, slots[5]);
        c.net.render(tf, slots[6]);
        c.io.render(tf, slots[7]);
    })
    .expect("offscreen draw");
    term.backend().buffer().clone()
}

/// Copy the rows `[scroll, scroll + area.height)` of `src` into `area` of the
/// frame. Out-of-range source rows are skipped (leaves blanks), so this never
/// panics on a short column or oversized viewport.
fn blit(f: &mut Frame, src: &Buffer, area: Rect, scroll: u16) {
    let dst = f.buffer_mut();
    for row in 0..area.height {
        let sy = scroll.saturating_add(row);
        for col in 0..area.width {
            if let Some(cell) = src.cell((col, sy)) {
                if let Some(d) = dst.cell_mut((area.x + col, area.y + row)) {
                    *d = cell.clone();
                }
            }
        }
    }
}

fn draw(f: &mut Frame, c: &Cockpit, offbuf: &Buffer, scroll: u16, content_h: u16) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // pinned vitals row
            Constraint::Length(1), // separator
            Constraint::Min(0),    // scrollable column
            Constraint::Length(1), // footer
        ])
        .split(area);

    let v = c.read_vitals();
    render_vitals_row(f, chunks[0], &v, c.gpu.vram());

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            theme::dim(),
        ))),
        chunks[1],
    );

    blit(f, offbuf, chunks[2], scroll);

    let view_h = chunks[2].height;
    let max_scroll = content_h.saturating_sub(view_h);
    let pct = if max_scroll == 0 {
        100
    } else {
        (scroll as u32 * 100 / max_scroll as u32) as u16
    };
    let footer = Line::from(vec![
        Span::styled(" j/k", theme::pane_header_focused()),
        Span::styled(" scroll", theme::dim()),
        Span::styled("  space/b", theme::pane_header_focused()),
        Span::styled(" page", theme::dim()),
        Span::styled("  g/G", theme::pane_header_focused()),
        Span::styled(" top/bot", theme::dim()),
        Span::styled("  [ ]", theme::pane_header_focused()),
        Span::styled(" bright", theme::dim()),
        Span::styled("  q", theme::pane_header_focused()),
        Span::styled(" quit", theme::dim()),
        Span::styled(format!("   {pct}%"), theme::status()),
    ]);
    f.render_widget(Paragraph::new(footer), chunks[3]);
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
    let mut scroll: u16 = 0;
    let mut goto_bottom = false;
    cockpit.tick_all(); // prime so the first frame has data
    loop {
        if last.elapsed() >= Duration::from_millis(1000) {
            cockpit.tick_all();
            last = Instant::now();
        }

        let size = terminal.size()?;
        let width = size.width.max(1);
        // Column viewport height = total minus row(2) + separator(1) + footer(1).
        let view_h = size.height.saturating_sub(4);
        let heights = cockpit.panel_heights();
        let content_h: u16 = heights.iter().copied().sum::<u16>().max(1);
        let offbuf = render_column_offscreen(cockpit, width, &heights, content_h);
        let max_scroll = content_h.saturating_sub(view_h);
        if goto_bottom {
            scroll = max_scroll;
            goto_bottom = false;
        }
        if scroll > max_scroll {
            scroll = max_scroll;
        }

        terminal.draw(|f| draw(f, cockpit, &offbuf, scroll, content_h))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(());
                }
                let page = view_h.saturating_sub(1).max(1);
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('j') | KeyCode::Down => {
                        scroll = scroll.saturating_add(2).min(max_scroll);
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        scroll = scroll.saturating_sub(2);
                    }
                    KeyCode::Char(' ') | KeyCode::PageDown => {
                        scroll = scroll.saturating_add(page).min(max_scroll);
                    }
                    KeyCode::Char('b') | KeyCode::PageUp => {
                        scroll = scroll.saturating_sub(page);
                    }
                    KeyCode::Char('g') | KeyCode::Home => {
                        scroll = 0;
                    }
                    KeyCode::Char('G') | KeyCode::End => {
                        goto_bottom = true;
                    }
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
