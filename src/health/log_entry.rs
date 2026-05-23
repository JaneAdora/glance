//! Form-agnostic log-entry state machine shared by the panel and the binary.
use crate::health::config::Activity;
use crossterm::event::KeyCode;

#[derive(Debug, Clone, PartialEq)]
pub enum LogInput {
    Idle,
    Pick { sel: usize },
    Type { activity: usize, buf: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogAction {
    None,
    Commit { activity: usize, count: f64 },
}

impl LogInput {
    pub fn is_capturing(&self) -> bool {
        !matches!(self, LogInput::Idle)
    }

    pub fn open(&mut self) {
        *self = LogInput::Pick { sel: 0 };
    }

    pub fn cancel(&mut self) {
        *self = LogInput::Idle;
    }

    /// Drive the machine with one key. Returns Commit when an event is ready.
    pub fn handle(&mut self, key: KeyCode, activities: &[Activity]) -> LogAction {
        let n = activities.len();
        if n == 0 {
            *self = LogInput::Idle;
            return LogAction::None;
        }
        match self {
            LogInput::Idle => LogAction::None,
            LogInput::Pick { sel } => {
                match key {
                    KeyCode::Esc | KeyCode::Char('q') => *self = LogInput::Idle,
                    KeyCode::Char('j') | KeyCode::Down => *sel = (*sel + 1) % n,
                    KeyCode::Char('k') | KeyCode::Up => {
                        *sel = if *sel == 0 { n - 1 } else { *sel - 1 }
                    }
                    KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                        let idx = (c as u8 - b'1') as usize;
                        if idx < n {
                            *self = LogInput::Type { activity: idx, buf: String::new() };
                        }
                    }
                    KeyCode::Enter => {
                        let a = *sel;
                        *self = LogInput::Type { activity: a, buf: String::new() };
                    }
                    _ => {}
                }
                LogAction::None
            }
            LogInput::Type { activity, buf } => match key {
                KeyCode::Esc | KeyCode::Char('q') => {
                    *self = LogInput::Idle;
                    LogAction::None
                }
                KeyCode::Backspace => {
                    buf.pop();
                    LogAction::None
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    buf.push(c);
                    LogAction::None
                }
                KeyCode::Char('.') if !buf.contains('.') => {
                    buf.push('.');
                    LogAction::None
                }
                KeyCode::Char('-') if buf.is_empty() => {
                    buf.push('-');
                    LogAction::None
                }
                KeyCode::Enter => {
                    let parsed = buf.parse::<f64>().ok();
                    let act = *activity;
                    *self = LogInput::Idle;
                    match parsed {
                        Some(v) if v != 0.0 => LogAction::Commit { activity: act, count: v },
                        _ => LogAction::None,
                    }
                }
                _ => LogAction::None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::config::Activity;

    fn acts() -> Vec<Activity> {
        vec![
            Activity { name: "pushups".into(), goal: 10.0, unit: "reps".into(), weekly_target: None },
            Activity { name: "water".into(), goal: 8.0, unit: "glasses".into(), weekly_target: None },
        ]
    }

    #[test]
    fn full_typed_commit() {
        let a = acts();
        let mut s = LogInput::Idle;
        assert!(!s.is_capturing());
        s.open();
        assert!(s.is_capturing());
        assert_eq!(s.handle(KeyCode::Char('2'), &a), LogAction::None);
        assert!(matches!(s, LogInput::Type { activity: 1, .. }));
        s.handle(KeyCode::Char('2'), &a);
        s.handle(KeyCode::Char('5'), &a);
        let out = s.handle(KeyCode::Enter, &a);
        assert_eq!(out, LogAction::Commit { activity: 1, count: 25.0 });
        assert!(!s.is_capturing());
    }

    #[test]
    fn esc_cancels() {
        let a = acts();
        let mut s = LogInput::Idle;
        s.open();
        s.handle(KeyCode::Enter, &a);
        let out = s.handle(KeyCode::Esc, &a);
        assert_eq!(out, LogAction::None);
        assert_eq!(s, LogInput::Idle);
    }

    #[test]
    fn empty_or_zero_does_not_commit() {
        let a = acts();
        let mut s = LogInput::Idle;
        s.open();
        s.handle(KeyCode::Enter, &a);
        let out = s.handle(KeyCode::Enter, &a);
        assert_eq!(out, LogAction::None);
        assert_eq!(s, LogInput::Idle);
    }

    #[test]
    fn negative_undo_value() {
        let a = acts();
        let mut s = LogInput::Idle;
        s.open();
        s.handle(KeyCode::Char('1'), &a);
        s.handle(KeyCode::Char('-'), &a);
        s.handle(KeyCode::Char('3'), &a);
        let out = s.handle(KeyCode::Enter, &a);
        assert_eq!(out, LogAction::Commit { activity: 0, count: -3.0 });
    }
}
