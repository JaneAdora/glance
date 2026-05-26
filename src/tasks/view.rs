//! Shared view helpers: SessionGroup, count_active, is_blocked, WidthClass.
use crate::tasks::task::{ClaudeTask, SessionId, Status, TaskId};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct SessionGroup {
    pub session_id: SessionId,
    pub label: String,
    pub mtime: SystemTime,
    pub tasks: Vec<ClaudeTask>,
}

/// Count of tasks where `status != Completed`.
pub fn count_active(group: &SessionGroup) -> usize {
    group.tasks.iter().filter(|t| t.status != Status::Completed).count()
}

/// Returns `Some(open_blocker_ids)` if any id in `task.blocked_by` matches a
/// task in `all_in_session` whose status is Pending or InProgress. `None` if
/// `blocked_by` is empty or all referenced blockers are Completed/absent.
pub fn is_blocked(task: &ClaudeTask, all_in_session: &[ClaudeTask]) -> Option<Vec<TaskId>> {
    if task.blocked_by.is_empty() { return None; }
    let open: Vec<TaskId> = task.blocked_by.iter()
        .filter(|bid| {
            all_in_session.iter()
                .find(|t| &t.id == *bid)
                .map(|t| t.status != Status::Completed)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    if open.is_empty() { None } else { Some(open) }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WidthClass {
    Tiny,    // <40 — glyph + id + truncated subject only
    Narrow,  // 40-59 — drop ⛔ markers
    Mid,     // 60-79 — full
    Wide,    // ≥80 — full
}

impl WidthClass {
    pub fn from(cols: u16) -> Self {
        match cols {
            0..=39 => Self::Tiny,
            40..=59 => Self::Narrow,
            60..=79 => Self::Mid,
            _ => Self::Wide,
        }
    }
    pub fn show_count_in_header(&self) -> bool { !matches!(self, Self::Tiny) }
    pub fn show_blocked_marker(&self) -> bool { matches!(self, Self::Mid | Self::Wide) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn t(id: &str, status: Status, blocked_by: Vec<&str>) -> ClaudeTask {
        ClaudeTask {
            id: id.into(), subject: "".into(), description: "".into(),
            active_form: "".into(), status,
            blocks: vec![],
            blocked_by: blocked_by.into_iter().map(String::from).collect(),
        }
    }
    fn g(tasks: Vec<ClaudeTask>) -> SessionGroup {
        SessionGroup {
            session_id: "s".into(), label: "x".into(),
            mtime: SystemTime::UNIX_EPOCH, tasks,
        }
    }

    #[test]
    fn count_active_excludes_completed() {
        let group = g(vec![
            t("1", Status::Pending, vec![]),
            t("2", Status::InProgress, vec![]),
            t("3", Status::Completed, vec![]),
        ]);
        assert_eq!(count_active(&group), 2);
    }

    #[test]
    fn is_blocked_returns_open_blockers_only() {
        let tasks = vec![
            t("1", Status::Pending, vec![]),
            t("2", Status::Completed, vec![]),
            t("3", Status::Pending, vec!["1", "2"]),
        ];
        let blocked = is_blocked(&tasks[2], &tasks).unwrap();
        assert_eq!(blocked, vec!["1".to_string()]);
    }

    #[test]
    fn is_blocked_returns_none_when_no_blockers() {
        let tasks = vec![t("1", Status::Pending, vec![])];
        assert!(is_blocked(&tasks[0], &tasks).is_none());
    }

    #[test]
    fn is_blocked_returns_none_when_all_blockers_completed() {
        let tasks = vec![
            t("1", Status::Completed, vec![]),
            t("2", Status::Pending, vec!["1"]),
        ];
        assert!(is_blocked(&tasks[1], &tasks).is_none());
    }

    #[test]
    fn is_blocked_returns_none_when_blocker_absent() {
        let tasks = vec![t("2", Status::Pending, vec!["99"])];
        assert!(is_blocked(&tasks[0], &tasks).is_none());
    }

    #[test]
    fn width_class_breakpoints() {
        assert_eq!(WidthClass::from(30), WidthClass::Tiny);
        assert_eq!(WidthClass::from(50), WidthClass::Narrow);
        assert_eq!(WidthClass::from(70), WidthClass::Mid);
        assert_eq!(WidthClass::from(100), WidthClass::Wide);
        assert!(!WidthClass::Tiny.show_count_in_header());
        assert!(!WidthClass::Narrow.show_blocked_marker());
        assert!(WidthClass::Mid.show_blocked_marker());
    }
}
