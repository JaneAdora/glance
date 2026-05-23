//! glance panel form of the health tracker (thin wrapper over HealthCore).
use crate::health::HealthCore;
use crate::panels::Panel;
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

pub struct HealthPanel {
    core: HealthCore,
}

impl HealthPanel {
    pub fn new() -> Self {
        Self { core: HealthCore::new() }
    }
}

impl Panel for HealthPanel {
    fn name(&self) -> &str {
        "health"
    }
    fn refresh_ms(&self) -> u64 {
        1_000
    }
    fn tick(&mut self) {
        self.core.tick();
    }
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.core.handle_key(key)
    }
    fn wants_keys(&self) -> bool {
        self.core.is_capturing()
    }
    fn render(&self, f: &mut Frame, area: Rect) {
        self.core.render(f, area);
    }
}
