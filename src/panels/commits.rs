use crate::panels::Panel;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

const DAYS: usize = 91;

pub struct CommitsPanel {
    counts: HashMap<String, u32>,  // ISO date string -> count
    last_refresh: Option<Instant>,
    rx: mpsc::Receiver<HashMap<String, u32>>,
    tx: mpsc::Sender<HashMap<String, u32>>,
    loading: bool,
}

impl CommitsPanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            counts: HashMap::new(),
            last_refresh: None,
            rx,
            tx,
            loading: false,
        }
    }

    fn kick_scan(&mut self) {
        let tx = self.tx.clone();
        self.loading = true;
        thread::spawn(move || {
            let result = scan_all_repos();
            let _ = tx.send(result);
        });
    }
}

pub(crate) fn project_roots() -> Vec<PathBuf> {
    if let Ok(s) = std::env::var("WT_ROOTS") {
        return s
            .split(':')
            .filter(|x| !x.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    vec![home.join("projects"), home.join("Projects")]
}

pub(crate) fn find_repos(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        if p.join(".git").exists() {
            out.push(p);
        }
    }
    out
}

fn scan_repo(repo: &Path) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    let res = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "log",
            "--since=91 days ago",
            "--format=%cs",
            "--all",
            "--no-merges",
        ])
        .output();
    let stdout = match res {
        Ok(o) if o.status.success() => o.stdout,
        _ => return out,
    };
    for line in String::from_utf8_lossy(&stdout).lines() {
        let date = line.trim();
        if date.len() == 10 {
            *out.entry(date.to_string()).or_insert(0) += 1;
        }
    }
    out
}

fn scan_all_repos() -> HashMap<String, u32> {
    let mut total = HashMap::new();
    for root in project_roots() {
        for repo in find_repos(&root) {
            for (date, n) in scan_repo(&repo) {
                *total.entry(date).or_insert(0) += n;
            }
        }
    }
    total
}

fn ymd(date: chrono_lite::Date) -> String {
    format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)
}

/// Interpolate the contribution color from lavender (low) to magenta (high).
fn grad(ratio: f64) -> Color {
    let ratio = ratio.clamp(0.0, 1.0);
    let r = (0xc5 as f64 * (1.0 - ratio) + 0xff as f64 * ratio) as u8;
    let g = (0xa3 as f64 * (1.0 - ratio) + 0x6e as f64 * ratio) as u8;
    let b = (0xff as f64 * (1.0 - ratio) + 0xc7 as f64 * ratio) as u8;
    Color::Rgb(r, g, b)
}

/// Cell color for a day's commit count; empty days use the dim shadow swatch.
fn heat_color(n: u32, max: u32) -> Color {
    if n == 0 {
        theme::shadow()
    } else {
        let ratio = (n as f64 / max.max(1) as f64).clamp(0.15, 1.0);
        grad(ratio)
    }
}

// Tiny date math without pulling in chrono
mod chrono_lite {
    #[derive(Clone, Copy)]
    pub struct Date {
        pub year: i32,
        pub month: u32,
        pub day: u32,
    }

    pub fn today() -> Date {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;
        from_days(now / 86400)
    }

    pub fn dow(date: Date) -> u32 {
        // Days since 1970-01-01 was Thursday (dow=4); modulo 7
        let d = days_since_epoch(date);
        ((d + 4).rem_euclid(7)) as u32
    }

    pub fn sub_days(date: Date, n: i64) -> Date {
        let d = days_since_epoch(date);
        from_days(d - n)
    }

    fn is_leap(y: i32) -> bool {
        (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
    }

    fn days_in_month(y: i32, m: u32) -> u32 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => if is_leap(y) { 29 } else { 28 },
            _ => 0,
        }
    }

    fn days_since_epoch(d: Date) -> i64 {
        let mut total: i64 = 0;
        for y in 1970..d.year {
            total += if is_leap(y) { 366 } else { 365 };
        }
        for m in 1..d.month {
            total += days_in_month(d.year, m) as i64;
        }
        total + (d.day as i64) - 1
    }

    fn from_days(mut d: i64) -> Date {
        let mut year = 1970;
        loop {
            let yd: i64 = if is_leap(year) { 366 } else { 365 };
            if d < yd {
                break;
            }
            d -= yd;
            year += 1;
        }
        let mut month = 1;
        loop {
            let md = days_in_month(year, month) as i64;
            if d < md {
                break;
            }
            d -= md;
            month += 1;
        }
        Date {
            year,
            month,
            day: (d as u32) + 1,
        }
    }
}

impl Panel for CommitsPanel {
    fn name(&self) -> &str {
        "commits"
    }

    fn refresh_ms(&self) -> u64 {
        // Slow scan; trigger via tick gating below
        20_000
    }

    fn tick(&mut self) {
        while let Ok(map) = self.rx.try_recv() {
            self.counts = map;
            self.loading = false;
        }
        let stale = match self.last_refresh {
            None => true,
            Some(t) => t.elapsed() > std::time::Duration::from_secs(300),
        };
        if stale && !self.loading {
            self.last_refresh = Some(Instant::now());
            self.kick_scan();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let today = chrono_lite::today();
        let dow_today = chrono_lite::dow(today); // 0=Sun

        // Build (week_idx, day_of_week, count) cells: we draw 13 weeks of 7 days.
        let weeks = 13usize;
        let mut cells: Vec<(usize, u32, u32)> = Vec::with_capacity(weeks * 7);
        let start_offset = dow_today as i64 + ((weeks - 1) * 7) as i64;
        let mut max_count = 0u32;
        for i in 0..(weeks * 7) {
            let offset = start_offset - i as i64;
            if offset < 0 {
                break;
            }
            let d = chrono_lite::sub_days(today, offset);
            let dow = chrono_lite::dow(d);
            let week_idx = i / 7;
            let n = self
                .counts
                .get(&ymd(d))
                .copied()
                .unwrap_or(0);
            if n > max_count {
                max_count = n;
            }
            cells.push((week_idx, dow, n));
        }

        let title = if self.loading && self.counts.is_empty() {
            " commits (last 90 days) -- scanning… ".to_string()
        } else {
            let total: u32 = self.counts.values().sum();
            format!(" commits (last 90 days) — {} total ", total)
        };
        let mut lines: Vec<Line> =
            vec![Line::from(Span::styled(title, theme::pane_header()))];

        if max_count == 0 && self.counts.is_empty() {
            lines.push(Line::from(Span::styled(
                if self.loading {
                    "(scanning ~/projects + ~/Projects…)"
                } else {
                    "(no commits found; set $WT_ROOTS to point at your repos)"
                },
                theme::dim(),
            )));
            f.render_widget(Paragraph::new(lines), area);
            return;
        }

        // Reshape the cells into a 7 (Sun..Sat) x weeks grid of counts.
        let mut grid = vec![vec![0u32; weeks]; 7];
        for &(w, dow, n) in &cells {
            if (dow as usize) < 7 && w < weeks {
                grid[dow as usize][w] = n;
            }
        }

        // GitHub-style heatmap of filled cells, with Mon/Wed/Fri row labels.
        let row_labels = ["   ", "Mon", "   ", "Wed", "   ", "Fri", "   "];
        lines.push(Line::from(""));
        for (dow, label) in row_labels.iter().enumerate() {
            let mut spans: Vec<Span> = vec![Span::styled(format!("{label} "), theme::dim())];
            for w in 0..weeks {
                let color = heat_color(grid[dow][w], max_count);
                spans.push(Span::styled("██", Style::default().fg(color)));
                spans.push(Span::raw(" "));
            }
            lines.push(Line::from(spans));
        }

        // Legend: less ██ ██ ██ ██ ██ more
        let mut legend: Vec<Span> = vec![Span::raw("    "), Span::styled("less ", theme::dim())];
        for &lvl in &[0.0f64, 0.25, 0.5, 0.75, 1.0] {
            let c = if lvl == 0.0 { theme::shadow() } else { grad(lvl) };
            legend.push(Span::styled("██", Style::default().fg(c)));
            legend.push(Span::raw(" "));
        }
        legend.push(Span::styled("more", theme::dim()));
        lines.push(Line::from(""));
        lines.push(Line::from(legend));

        f.render_widget(Paragraph::new(lines), area);
    }
}
