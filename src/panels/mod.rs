pub mod cpu;
pub mod mem;

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
    ]
}
