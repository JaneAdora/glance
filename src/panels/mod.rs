pub mod battery;
pub mod clock;
pub mod commits;
pub mod cpu;
pub mod disk;
pub mod mem;
pub mod moon;
pub mod net;
pub mod peon;
pub mod pet;
pub mod ping;
pub mod temp;
pub mod tsmap;

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
}

// Default registry: cpu, mem, net, disk, ping, commits, peon, moon.
// `battery` is intentionally NOT registered here — this machine has no battery.
// To enable on a laptop, add `Box::new(battery::BatteryPanel::new()),` after disk.
pub fn default_registry() -> Vec<Box<dyn Panel>> {
    vec![
        Box::new(cpu::CpuPanel::new()),
        Box::new(mem::MemPanel::new()),
        Box::new(net::NetPanel::new()),
        Box::new(disk::DiskPanel::new()),
        Box::new(ping::PingPanel::new()),
        Box::new(commits::CommitsPanel::new()),
        Box::new(peon::PeonPanel::new()),
        Box::new(temp::TempPanel::new()),
        Box::new(tsmap::TsMapPanel::new()),
        Box::new(pet::PetPanel::new()),
        Box::new(moon::MoonPanel::new()),
        Box::new(clock::ClockPanel::new()),
    ]
}
