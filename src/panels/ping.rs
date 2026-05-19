use crate::panels::Panel;
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph,
};
use ratatui::Frame;
use std::collections::VecDeque;
use std::process::Command;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

const HIST: usize = 60;

fn palette() -> [Color; 5] {
    [
        theme::magenta(),
        theme::pink(),
        theme::lavender(),
        theme::sage(),
        theme::amber(),
    ]
}

struct HostStats {
    name: String,
    samples: VecDeque<Option<f64>>,
    last_ms: Option<f64>,
}

pub struct PingPanel {
    hosts: Vec<HostStats>,
    last_kick: Instant,
    rx: mpsc::Receiver<(String, Option<f64>)>,
    tx: mpsc::Sender<(String, Option<f64>)>,
    inflight: Arc<Mutex<std::collections::HashSet<String>>>,
}

fn default_hosts() -> Vec<String> {
    if let Ok(s) = std::env::var("GLANCE_PING_HOSTS") {
        let v: Vec<String> = s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
        if !v.is_empty() {
            return v;
        }
    }
    vec!["1.1.1.1".into(), "8.8.8.8".into(), "github.com".into()]
}

impl PingPanel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let hosts: Vec<HostStats> = default_hosts()
            .into_iter()
            .map(|name| HostStats {
                name,
                samples: VecDeque::with_capacity(HIST),
                last_ms: None,
            })
            .collect();
        Self {
            hosts,
            last_kick: Instant::now() - Duration::from_secs(60),
            rx,
            tx,
            inflight: Arc::new(Mutex::new(Default::default())),
        }
    }

    fn kick_one(&self, host: String) {
        let tx = self.tx.clone();
        let inflight = Arc::clone(&self.inflight);
        thread::spawn(move || {
            let out = Command::new("ping")
                .args(["-c", "1", "-W", "1", "-n", &host])
                .output();
            let ms = match out {
                Ok(o) if o.status.success() => parse_ping(&String::from_utf8_lossy(&o.stdout)),
                _ => None,
            };
            let _ = tx.send((host.clone(), ms));
            if let Ok(mut g) = inflight.lock() {
                g.remove(&host);
            }
        });
    }
}

fn parse_ping(s: &str) -> Option<f64> {
    let i = s.find("time=")?;
    let rest = &s[i + 5..];
    let end = rest.find(' ')?;
    rest[..end].parse().ok()
}

impl Panel for PingPanel {
    fn name(&self) -> &str {
        "ping"
    }

    fn refresh_ms(&self) -> u64 {
        1_000
    }

    fn tick(&mut self) {
        // Drain finished pings
        while let Ok((host, ms)) = self.rx.try_recv() {
            if let Some(h) = self.hosts.iter_mut().find(|h| h.name == host) {
                if h.samples.len() == HIST {
                    h.samples.pop_front();
                }
                h.samples.push_back(ms);
                h.last_ms = ms;
            }
        }
        // Kick a new round at most once per second
        if self.last_kick.elapsed() >= Duration::from_millis(900) {
            self.last_kick = Instant::now();
            let hosts: Vec<String> = self.hosts.iter().map(|h| h.name.clone()).collect();
            for h in hosts {
                let mut g = match self.inflight.lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                if g.contains(&h) {
                    continue;
                }
                g.insert(h.clone());
                drop(g);
                self.kick_one(h);
            }
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        if self.hosts.is_empty() {
            f.render_widget(crate::widgets::empty("no hosts; set GLANCE_PING_HOSTS=h1,h2"), area);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(self.hosts.len() as u16 + 1), Constraint::Min(3)])
            .split(area);

        // Header table: host  current  min/max/avg style line per host
        let mut lines: Vec<Line> = Vec::with_capacity(self.hosts.len() + 1);
        lines.push(Line::from(Span::styled(
            " ping latency (ms) ",
            theme::pane_header(),
        )));
        for (i, h) in self.hosts.iter().enumerate() {
            let color = palette()[i % 5];
            let current = match h.last_ms {
                Some(ms) => format!("{:>6.1}", ms),
                None => "  drop".to_string(),
            };
            let live: Vec<f64> = h.samples.iter().filter_map(|s| *s).collect();
            let summary = if live.is_empty() {
                "—".to_string()
            } else {
                let avg = live.iter().sum::<f64>() / live.len() as f64;
                let min = live.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = live.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let drops = h.samples.iter().filter(|s| s.is_none()).count();
                format!(
                    "min {:>5.1}  avg {:>5.1}  max {:>5.1}  drops {}",
                    min, avg, max, drops
                )
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  ● "), Style::default().fg(color)),
                Span::styled(format!("{:<20} ", h.name), theme::pane_header()),
                Span::styled(current, Style::default().fg(color)),
                Span::styled(format!(" ms   {}", summary), theme::dim()),
            ]));
        }
        f.render_widget(Paragraph::new(lines), chunks[0]);

        // Chart: one Dataset per host
        let max_ms = self
            .hosts
            .iter()
            .flat_map(|h| h.samples.iter().filter_map(|s| *s))
            .fold(20.0f64, f64::max);
        let datasets_data: Vec<Vec<(f64, f64)>> = self
            .hosts
            .iter()
            .map(|h| {
                h.samples
                    .iter()
                    .enumerate()
                    .filter_map(|(i, s)| s.map(|ms| (i as f64, ms)))
                    .collect()
            })
            .collect();
        let datasets: Vec<Dataset> = self
            .hosts
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let color = palette()[i % 5];
                Dataset::default()
                    .name(h.name.clone())
                    .marker(Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(color))
                    .data(&datasets_data[i])
            })
            .collect();
        let chart = Chart::new(datasets)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(theme::dim())
                    .title(Line::from(Span::styled(
                        format!(" last {} samples ", HIST),
                        theme::dim(),
                    ))),
            )
            .x_axis(Axis::default().bounds([0.0, HIST as f64]))
            .y_axis(
                Axis::default()
                    .style(theme::dim())
                    .bounds([0.0, (max_ms * 1.2).max(10.0)])
                    .labels(vec![
                        Span::styled("0", theme::dim()),
                        Span::styled(format!("{:.0}", max_ms / 2.0), theme::dim()),
                        Span::styled(format!("{:.0}", max_ms), theme::dim()),
                    ]),
            );
        f.render_widget(chart, chunks[1]);
    }
}
