pub mod battery;
pub mod alerts;
pub mod clock;
pub mod commits;
pub mod cpu;
pub mod disk;
pub mod conn;
pub mod entropy;
pub mod fans;
pub mod mem;
pub mod moon;
pub mod net;
pub mod pet;
pub mod ping;
pub mod issues;
pub mod prs;
pub mod temp;
pub mod timer;
pub mod traceroute;
pub mod tsmap;
pub mod gpu;
pub mod crew;
pub mod health;
pub mod hurricane;
pub mod io;
pub mod loadavg;
pub mod mandala;
pub mod mascot;
pub mod launchers;
pub mod music;
pub mod solar;
pub mod starfield;
pub mod tasks;
pub mod weather;
pub mod world_ping;

use ratatui::layout::Rect;
use ratatui::Frame;

pub trait Panel {
    fn name(&self) -> &str;
    fn tick(&mut self);
    fn render(&self, f: &mut Frame, area: Rect);
    fn refresh_ms(&self) -> u64 {
        500
    }
    /// Panel-specific key handler. Return true if the key was consumed; false
    /// to let the global handler process it. Default: not handled.
    fn handle_key(&mut self, _key: crossterm::event::KeyEvent) -> bool {
        false
    }
    /// When true, the global key handler routes ALL keys to this panel (used by
    /// panels that capture typed input, e.g. health's log entry). Default off.
    fn wants_keys(&self) -> bool {
        false
    }
}

/// Every panel name glance knows how to build, in the default display order.
/// `battery` is appended last and excluded from the default registry (no battery
/// on the dev box) but remains buildable by name from a config file.
pub const DEFAULT_ORDER: &[&str] = &[
    "cpu", "mem", "net", "disk", "loadavg", "entropy", "io", "conn", "gpu",
    "ping", "world-ping", "traceroute", "commits", "health", "prs", "issues", "temp", "tsmap",
    "clock", "weather", "alerts", "hurricane", "solar",
    "timer", "music", "pet", "moon", "mascot", "starfield", "mandala", "launchers", "crew", "tasks",
];

/// All buildable panel names (superset of DEFAULT_ORDER; includes `battery`).
pub const ALL_PANELS: &[&str] = &[
    "cpu", "mem", "net", "disk", "loadavg", "entropy", "fans", "io", "conn", "gpu",
    "ping", "world-ping", "traceroute", "commits", "health", "prs", "issues", "temp", "tsmap",
    "clock", "weather", "alerts", "hurricane", "solar",
    "timer", "music", "pet", "moon", "mascot", "starfield", "mandala", "battery", "launchers", "crew", "tasks",
];

/// Construct a panel by name. Returns None for unknown names.
pub fn build_panel(name: &str) -> Option<Box<dyn Panel>> {
    Some(match name {
        "cpu" => Box::new(cpu::CpuPanel::new()),
        "mem" => Box::new(mem::MemPanel::new()),
        "net" => Box::new(net::NetPanel::new()),
        "disk" => Box::new(disk::DiskPanel::new()),
        "loadavg" => Box::new(loadavg::LoadavgPanel::new()),
        "entropy" => Box::new(entropy::EntropyPanel::new()),
        "fans" => Box::new(fans::FansPanel::new()),
        "io" => Box::new(io::IoPanel::new()),
        "conn" => Box::new(conn::ConnPanel::new()),
        "gpu" => Box::new(gpu::GpuPanel::new()),
        "ping" => Box::new(ping::PingPanel::new()),
        "world-ping" => Box::new(world_ping::WorldPingPanel::new()),
        "traceroute" => Box::new(traceroute::TraceroutePanel::new()),
        "prs" => Box::new(prs::PrsPanel::new()),
        "issues" => Box::new(issues::IssuesPanel::new()),
        "commits" => Box::new(commits::CommitsPanel::new()),
        "health" => Box::new(health::HealthPanel::new()),
        "temp" => Box::new(temp::TempPanel::new()),
        "tsmap" => Box::new(tsmap::TsMapPanel::new()),
        "clock" => Box::new(clock::ClockPanel::new()),
        "weather" => Box::new(weather::WeatherPanel::new()),
        "alerts" => Box::new(alerts::AlertsPanel::new()),
        "hurricane" => Box::new(hurricane::HurricanePanel::new()),
        "solar" => Box::new(solar::SolarPanel::new()),
        "pet" => Box::new(pet::PetPanel::new()),
        "moon" => Box::new(moon::MoonPanel::new()),
        "mascot" => Box::new(mascot::MascotPanel::new()),
        "music" => Box::new(music::MusicPanel::new()),
        "timer" => Box::new(timer::TimerPanel::new()),
        "mandala" => Box::new(mandala::MandalaPanel::new()),
        "starfield" => Box::new(starfield::StarfieldPanel::new()),
        "battery" => Box::new(battery::BatteryPanel::new()),
        "launchers" => Box::new(launchers::LaunchersPanel::new()),
        "crew" => Box::new(crew::CrewPanel::new()),
        "tasks" => Box::new(tasks::TasksPanel::new()),
        _ => return None,
    })
}

/// Build the registry from an explicit ordered list of names. Unknown names are
/// skipped. Falls back to the default registry if the result would be empty.
pub fn registry_from_names(names: &[String]) -> Vec<Box<dyn Panel>> {
    let built: Vec<Box<dyn Panel>> = names.iter().filter_map(|n| build_panel(n)).collect();
    if built.is_empty() {
        default_registry()
    } else {
        built
    }
}

/// Default registry: every panel in DEFAULT_ORDER.
pub fn default_registry() -> Vec<Box<dyn Panel>> {
    DEFAULT_ORDER.iter().filter_map(|n| build_panel(n)).collect()
}
