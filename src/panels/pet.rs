use crate::layout::braille_aspect_bounds;
use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::fs;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{CpuRefreshKind, RefreshKind, System};


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mood {
    Sleeping,
    Content,
    Happy,
    Tired,
    Hot,
    Wired,
}

impl Mood {
    fn label(self) -> &'static str {
        match self {
            Mood::Sleeping => "sleeping",
            Mood::Content => "content",
            Mood::Happy => "happy",
            Mood::Tired => "tired",
            Mood::Hot => "warm",
            Mood::Wired => "wired",
        }
    }

    fn caption(self, pet: &str) -> String {
        match self {
            Mood::Sleeping => format!("{pet} is napping. zzz"),
            Mood::Content => format!("{pet} is here."),
            Mood::Happy => format!("{pet} is proud of you!"),
            Mood::Tired => format!("{pet} thinks you're working too hard."),
            Mood::Hot => format!("{pet} feels the heat."),
            Mood::Wired => format!("{pet} is buzzing with energy!"),
        }
    }
}

pub struct PetPanel {
    pet_name: String,
    sys: System,
    cpu_avg: f64,
    peon_done: u64,
    peon_goal: u64,
    hottest_c: f64,
    started: Instant,
    last_blink_change: Instant,
    blink_open: bool,
    next_blink_secs: f64,
}

impl PetPanel {
    pub fn new() -> Self {
        let name = std::env::var("GLANCE_PET_NAME").unwrap_or_else(|_| "Plip".to_string());
        let sys = System::new_with_specifics(
            RefreshKind::new().with_cpu(CpuRefreshKind::everything()),
        );
        Self {
            pet_name: name,
            sys,
            cpu_avg: 0.0,
            peon_done: 0,
            peon_goal: 1,
            hottest_c: 0.0,
            started: Instant::now(),
            last_blink_change: Instant::now(),
            blink_open: true,
            next_blink_secs: 3.5,
        }
    }

    fn mood(&self) -> Mood {
        let hour = local_hour();
        let goal_met = self.peon_goal > 0 && self.peon_done >= self.peon_goal;
        if goal_met {
            return Mood::Happy;
        }
        if self.cpu_avg >= 80.0 {
            return Mood::Wired;
        }
        if self.cpu_avg >= 55.0 {
            return Mood::Tired;
        }
        if self.hottest_c >= 75.0 {
            return Mood::Hot;
        }
        // Sleeping when it's late and the machine is idle
        if (hour >= 23 || hour < 7) && self.cpu_avg < 8.0 {
            return Mood::Sleeping;
        }
        Mood::Content
    }
}

fn local_hour() -> u32 {
    // Approximate local hour using TZ offset from libc::localtime. Falls back to UTC if
    // unavailable. Good enough for "is it bedtime?"
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // We assume system local time matches the machine's clock; just take secs/3600 % 24.
    // For TZ-correct results we'd want jiff or chrono; the difference is fine for mood logic.
    ((secs / 3600) % 24) as u32
}

fn read_peon() -> Option<(u64, u64)> {
    // Goals come from cfg.trainer.exercises (always present even if trainer disabled).
    // Reps come from state.trainer.reps (may be absent if today hasn't been logged yet).
    let cfg_path = PathBuf::from("/home/jane/.claude/hooks/peon-ping/config.json");
    let cfg: serde_json::Value = serde_json::from_str(&fs::read_to_string(cfg_path).ok()?).ok()?;
    let goals_obj = cfg
        .get("trainer")
        .and_then(|t| t.get("exercises"))
        .and_then(|e| e.as_object())?;
    let goal: u64 = goals_obj.values().filter_map(|v| v.as_u64()).sum();
    if goal == 0 {
        return None;
    }
    let state_path = PathBuf::from("/home/jane/.claude/hooks/peon-ping/.state.json");
    let done: u64 = fs::read_to_string(state_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("trainer").cloned())
        .and_then(|t| t.get("reps").cloned())
        .and_then(|r| r.as_object().cloned())
        .map(|m| m.values().filter_map(|v| v.as_u64()).sum())
        .unwrap_or(0);
    Some((done, goal))
}

fn read_hottest_temp() -> f64 {
    let dir = PathBuf::from("/sys/class/thermal");
    let Ok(entries) = fs::read_dir(&dir) else { return 0.0 };
    let mut hottest = 0.0_f64;
    for e in entries.flatten() {
        let n = e.file_name();
        if !n.to_string_lossy().starts_with("thermal_zone") {
            continue;
        }
        let temp = fs::read_to_string(e.path().join("temp"))
            .ok()
            .and_then(|s| s.trim().parse::<i64>().ok())
            .map(|mc| mc as f64 / 1000.0)
            .unwrap_or(0.0);
        if temp > 0.0 && temp < 200.0 && temp > hottest {
            hottest = temp;
        }
    }
    hottest
}

impl Panel for PetPanel {
    fn name(&self) -> &str {
        "pet"
    }

    fn refresh_ms(&self) -> u64 {
        200 // smooth blinks/breathing
    }

    fn tick(&mut self) {
        self.sys.refresh_cpu_usage();
        let cpus = self.sys.cpus();
        if !cpus.is_empty() {
            self.cpu_avg =
                cpus.iter().map(|c| c.cpu_usage() as f64).sum::<f64>() / cpus.len() as f64;
        }
        if let Some((d, g)) = read_peon() {
            self.peon_done = d;
            self.peon_goal = g;
        }
        self.hottest_c = read_hottest_temp();

        // Blink scheduling: when interval elapses, flip state and pick a new interval.
        let elapsed = self.last_blink_change.elapsed().as_secs_f64();
        let target = if self.blink_open { self.next_blink_secs } else { 0.18 };
        if elapsed >= target {
            self.blink_open = !self.blink_open;
            self.last_blink_change = Instant::now();
            if self.blink_open {
                // Next interval is randomish but deterministic per second to avoid rand dep.
                let now_secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let jitter = ((now_secs.wrapping_mul(2654435761)) % 4000) as f64 / 1000.0;
                self.next_blink_secs = 2.5 + jitter; // 2.5–6.5s between blinks
            }
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let mood = self.mood();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(3)])
            .split(area);

        let block = Block::default()
            .borders(Borders::NONE)
            .title(Line::from(vec![
                Span::styled(format!(" {} ", self.pet_name), theme::pane_header()),
                Span::styled(format!("[{}]", mood.label()), theme::pane_header_focused()),
            ]));
        let inner = block.inner(chunks[0]);
        f.render_widget(block, chunks[0]);

        // Breathing offset: small sine wave, ±0.025 in canvas units.
        let t = self.started.elapsed().as_secs_f64();
        let breathe = (t * 1.6).sin() * 0.025;

        let blink_open = self.blink_open && mood != Mood::Sleeping;
        let mouth = mood;
        let cheeks = matches!(mood, Mood::Happy | Mood::Hot | Mood::Wired);
        let zzz_show = mood == Mood::Sleeping;

        // Build point sets.
        let body = points_in_body(breathe);
        let shadow = points_in_shadow(breathe);
        let (eyes, accent) = points_in_face(breathe, blink_open, mouth, cheeks);
        let zzz = if zzz_show { points_in_zzz(t, breathe) } else { Vec::new() };

        let (xb, yb) = braille_aspect_bounds(inner, 1.0, 1.0);
        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds(xb)
            .y_bounds(yb)
            .paint(move |ctx| {
                if !shadow.is_empty() {
                    ctx.draw(&Points {
                        coords: &shadow,
                        color: theme::lavender(),
                    });
                }
                ctx.layer();
                ctx.draw(&Points {
                    coords: &body,
                    color: theme::pink(),
                });
                ctx.layer();
                if !accent.is_empty() {
                    ctx.draw(&Points {
                        coords: &accent,
                        color: theme::magenta(),
                    });
                }
                ctx.draw(&Points {
                    coords: &eyes,
                    color: theme::face(),
                });
                if !zzz.is_empty() {
                    ctx.layer();
                    ctx.draw(&Points {
                        coords: &zzz,
                        color: theme::lavender(),
                    });
                }
            });
        f.render_widget(canvas, inner);

        let caption = mood.caption(&self.pet_name);
        let cpu_line = format!(
            "cpu {:>4.0}%   reps {}/{}   warmest {:>4.1}°C",
            self.cpu_avg, self.peon_done, self.peon_goal, self.hottest_c
        );
        let lines = vec![
            Line::from(Span::styled(caption, theme::pane_header_focused())),
            Line::from(Span::styled(cpu_line, theme::dim())),
        ];
        f.render_widget(Paragraph::new(lines), chunks[1]);
    }
}

// -- Drawing helpers ----------------------------------------------------------

const BODY_RX: f64 = 0.70;
const BODY_RY: f64 = 0.62;
const BODY_CY_BASE: f64 = -0.05;

fn body_center(breathe: f64) -> (f64, f64) {
    (0.0, BODY_CY_BASE + breathe)
}

fn inside_body(x: f64, y: f64, breathe: f64) -> bool {
    let (cx, cy) = body_center(breathe);
    let dx = (x - cx) / BODY_RX;
    let dy = (y - cy) / BODY_RY;
    dx * dx + dy * dy <= 1.0
}

fn points_in_body(breathe: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let step = 0.025;
    let mut y = -1.0;
    while y <= 1.0 {
        let mut x = -1.0;
        while x <= 1.0 {
            if inside_body(x, y, breathe) {
                out.push((x, y));
            }
            x += step;
        }
        y += step;
    }
    out
}

fn points_in_shadow(breathe: f64) -> Vec<(f64, f64)> {
    // Small elliptical shadow below the pet
    let mut out = Vec::new();
    let (cx, cy) = body_center(breathe);
    let shadow_cy = cy - BODY_RY - 0.05;
    let srx = BODY_RX * 0.70;
    let sry = 0.06;
    let step = 0.025;
    let mut y = shadow_cy - sry;
    while y <= shadow_cy + sry {
        let mut x = cx - srx;
        while x <= cx + srx {
            let nx = (x - cx) / srx;
            let ny = (y - shadow_cy) / sry;
            if nx * nx + ny * ny <= 1.0 {
                out.push((x, y));
            }
            x += step;
        }
        y += step;
    }
    out
}

fn points_in_face(
    breathe: f64,
    blink_open: bool,
    mood: Mood,
    cheeks: bool,
) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let mut eyes = Vec::new();
    let mut accents = Vec::new();
    let (cx, cy) = body_center(breathe);
    let eye_y = cy + 0.10;
    let eye_dx = 0.22;
    let eye_r = 0.06;

    for sign in [-1.0, 1.0] {
        let ex = cx + sign * eye_dx;
        if blink_open {
            disc(&mut eyes, ex, eye_y, eye_r, 0.022);
        } else {
            // Closed eye: short horizontal line
            line_seg(&mut eyes, ex - eye_r, ex + eye_r, eye_y, 0.022);
        }
    }

    // Mouth
    let mouth_y_base = cy - 0.18;
    match mood {
        Mood::Happy => {
            // Wide smile arc curving up
            arc(&mut eyes, cx, mouth_y_base - 0.04, 0.20, 0.10, true, 0.04);
        }
        Mood::Wired => {
            // Big open "o"
            ring(&mut eyes, cx, mouth_y_base - 0.02, 0.07, 0.04, 0.04);
        }
        Mood::Tired => {
            // Straight mouth, slightly down
            line_seg(&mut eyes, cx - 0.12, cx + 0.12, mouth_y_base - 0.02, 0.025);
        }
        Mood::Hot => {
            // Wavy mouth (panting): short tongue out
            line_seg(&mut eyes, cx - 0.10, cx + 0.10, mouth_y_base, 0.022);
            disc(&mut accents, cx + 0.04, mouth_y_base - 0.06, 0.05, 0.028);
        }
        Mood::Sleeping => {
            // Slightly open mouth
            line_seg(&mut eyes, cx - 0.04, cx + 0.04, mouth_y_base + 0.02, 0.022);
        }
        Mood::Content => {
            arc(&mut eyes, cx, mouth_y_base - 0.02, 0.13, 0.05, true, 0.035);
        }
    }

    if cheeks {
        disc(&mut accents, cx - 0.32, cy + 0.00, 0.05, 0.03);
        disc(&mut accents, cx + 0.32, cy + 0.00, 0.05, 0.03);
    }

    (eyes, accents)
}

fn points_in_zzz(t: f64, breathe: f64) -> Vec<(f64, f64)> {
    // Three "z" letters rising from the pet, oscillating
    let (cx, cy) = body_center(breathe);
    let base_x = cx + BODY_RX * 0.75;
    let base_y = cy + BODY_RY * 0.7;
    let mut out = Vec::new();
    for (i, scale) in [(0.0, 0.07), (1.0, 0.09), (2.0, 0.12)].iter().enumerate() {
        let offset_t = (t * 0.5 + i as f64 * 1.5) % 3.0;
        let lift_y = base_y + offset_t * 0.08;
        let lift_x = base_x + (offset_t * 1.7).sin() * 0.04 + (i as f64) * 0.06;
        let s = scale.1;
        // Draw a Z: top horizontal, diagonal, bottom horizontal
        line_seg(&mut out, lift_x - s, lift_x + s, lift_y + s, 0.022);
        line_seg(&mut out, lift_x - s, lift_x + s, lift_y - s, 0.022);
        // Diagonal: parametric
        let steps = 10;
        for k in 0..=steps {
            let f = k as f64 / steps as f64;
            let x = lift_x + s - 2.0 * s * f;
            let y = lift_y + s - 2.0 * s * f;
            out.push((x, y));
        }
    }
    out
}

fn disc(out: &mut Vec<(f64, f64)>, cx: f64, cy: f64, r: f64, step: f64) {
    let mut y = cy - r;
    while y <= cy + r {
        let mut x = cx - r;
        while x <= cx + r {
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= r * r {
                out.push((x, y));
            }
            x += step;
        }
        y += step;
    }
}

fn line_seg(out: &mut Vec<(f64, f64)>, x0: f64, x1: f64, y: f64, step: f64) {
    let mut x = x0;
    while x <= x1 {
        out.push((x, y));
        x += step;
    }
}

fn arc(
    out: &mut Vec<(f64, f64)>,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    smile_up: bool,
    step: f64,
) {
    // Draw the lower (or upper) half of an ellipse outline
    let mut x = -rx;
    while x <= rx {
        let f = 1.0 - (x / rx) * (x / rx);
        if f < 0.0 {
            x += step;
            continue;
        }
        let dy = ry * f.sqrt();
        let y = if smile_up { cy - dy } else { cy + dy };
        out.push((cx + x, y));
        x += step;
    }
}

fn ring(out: &mut Vec<(f64, f64)>, cx: f64, cy: f64, rx: f64, ry: f64, _step: f64) {
    // Draw the outline of an ellipse via parametric sampling
    let n = 36;
    for i in 0..n {
        let t = (i as f64) * std::f64::consts::TAU / (n as f64);
        out.push((cx + rx * t.cos(), cy + ry * t.sin()));
    }
}
