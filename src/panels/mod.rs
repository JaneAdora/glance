pub mod battery;
pub mod cpu;
pub mod disk;
pub mod mem;
pub mod net;
pub mod peon;

use ratatui::layout::Rect;
use ratatui::Frame;

pub trait Panel {
    fn name(&self) -> &str;
    fn tick(&mut self);
    fn render(&self, f: &mut Frame, area: Rect);
    fn refresh_ms(&self) -> u64 {
        500
    }
}

pub fn default_registry() -> Vec<Box<dyn Panel>> {
    vec![
        Box::new(cpu::CpuPanel::new()),
        Box::new(mem::MemPanel::new()),
        Box::new(net::NetPanel::new()),
        Box::new(disk::DiskPanel::new()),
        Box::new(battery::BatteryPanel::new()),
        Box::new(peon::PeonPanel::new()),
    ]
}
