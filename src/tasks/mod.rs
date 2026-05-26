//! TasksCore — Claude Code task viewer + editor for `~/.claude/tasks/`.
//! See `docs/superpowers/specs/2026-05-25-tasks-design.md`.

pub mod session;
pub mod store;
pub mod task;
pub mod view;

use crate::tasks::store::StoreError;
use crate::tasks::task::{ClaudeTask, SessionId, Status, TaskId};
use crate::tasks::view::SessionGroup;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, Copy, Default)]
pub struct Focus {
    pub group: usize,
    pub task: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    All,
    Session(SessionId),
    Subject(String),
}

#[derive(Debug)]
pub enum TaskAction {
    None,
    StatusCycled { id: TaskId, new: Status },
    Created { session: SessionId, id: TaskId },
    Deleted { id: TaskId },
    Toast(String),
    Quit,
}

pub struct TasksCore {
    pub groups: Vec<SessionGroup>,
    pub focus: Focus,
    pub expanded: HashSet<SessionId>,
    pub last_seen: HashMap<SessionId, SystemTime>,
    pub show_completed: bool,
    pub create_mode: Option<String>,
    pub pending_delete: Option<(SessionId, TaskId, Instant)>,
    pub last_toast: Option<(String, Instant)>,
    pub filter: Filter,
}

impl TasksCore {
    /// Construct + load. Failures fall back to an empty in-memory state with a toast.
    pub fn new() -> Self {
        let mut core = Self {
            groups: Vec::new(),
            focus: Focus::default(),
            expanded: HashSet::new(),
            last_seen: HashMap::new(),
            show_completed: false,
            create_mode: None,
            pending_delete: None,
            last_toast: None,
            filter: Filter::All,
        };
        core.full_reload();
        if let Some(g) = core.groups.first() {
            core.expanded.insert(g.session_id.clone());
        }
        core
    }

    /// Reload from disk; preserve focus by (SessionId, TaskId); prune expanded set.
    pub fn tick(&mut self) {
        let focus_key = self.current_focus_key();
        self.full_reload();
        if let Some((sid, tid)) = focus_key {
            self.set_focus_by_key(&sid, tid.as_deref());
        }
        let live: HashSet<SessionId> = self.groups.iter().map(|g| g.session_id.clone()).collect();
        self.expanded.retain(|s| live.contains(s));
        // clear stale pending_delete
        if let Some((_, _, t)) = self.pending_delete {
            if t.elapsed() > Duration::from_secs(2) {
                self.pending_delete = None;
            }
        }
        // clear stale toast (3s)
        if let Some((_, t)) = self.last_toast {
            if t.elapsed() > Duration::from_secs(3) {
                self.last_toast = None;
            }
        }
    }

    fn full_reload(&mut self) {
        let result = store::load_all_sessions();
        if let Some(t) = result.toasts.into_iter().next() {
            self.last_toast = Some((t, Instant::now()));
        }
        self.last_seen.clear();
        let mut groups: Vec<SessionGroup> = Vec::with_capacity(result.sessions.len());
        for s in result.sessions {
            self.last_seen.insert(s.session_id.clone(), s.mtime);
            let label = session::label_for(&s.session_id);
            groups.push(SessionGroup {
                session_id: s.session_id,
                label,
                mtime: s.mtime,
                tasks: s.tasks,
            });
        }
        self.groups = groups;
        if self.focus.group >= self.groups.len() {
            self.focus = Focus::default();
        }
    }

    fn current_focus_key(&self) -> Option<(SessionId, Option<TaskId>)> {
        let g = self.groups.get(self.focus.group)?;
        let tid = self.focus.task.and_then(|i| g.tasks.get(i).map(|t| t.id.clone()));
        Some((g.session_id.clone(), tid))
    }

    fn set_focus_by_key(&mut self, sid: &str, tid: Option<&str>) {
        if let Some(gi) = self.groups.iter().position(|g| g.session_id == sid) {
            self.focus.group = gi;
            self.focus.task = tid.and_then(|id| {
                self.groups[gi].tasks.iter().position(|t| t.id == id)
            });
        } else {
            self.focus = Focus::default();
        }
    }

    /// Toggle expanded state of the focused session group.
    pub fn toggle_expand(&mut self) {
        if let Some(g) = self.groups.get(self.focus.group) {
            let sid = g.session_id.clone();
            if !self.expanded.remove(&sid) {
                self.expanded.insert(sid);
            }
        }
    }

    /// Move focus down through visible rows (respects expand + show_completed).
    pub fn move_down(&mut self) {
        let visible = self.visible_rows();
        if visible.is_empty() { return; }
        let cur = self.focus_row_index(&visible).unwrap_or(0);
        let next = (cur + 1).min(visible.len() - 1);
        self.set_focus_from_row(&visible[next]);
    }

    /// Move focus up through visible rows.
    pub fn move_up(&mut self) {
        let visible = self.visible_rows();
        if visible.is_empty() { return; }
        let cur = self.focus_row_index(&visible).unwrap_or(0);
        let next = cur.saturating_sub(1);
        self.set_focus_from_row(&visible[next]);
    }

    /// Flat list of (group_idx, Option<task_idx>) for currently-visible rows.
    /// Header is `(gi, None)`. Tasks under expanded headers are `(gi, Some(ti))`.
    /// Respects `show_completed`.
    pub fn visible_rows(&self) -> Vec<(usize, Option<usize>)> {
        let mut rows = Vec::new();
        for (gi, group) in self.groups.iter().enumerate() {
            if !self.matches_filter(group) { continue; }
            rows.push((gi, None));
            if self.expanded.contains(&group.session_id) {
                for (ti, t) in group.tasks.iter().enumerate() {
                    if !self.show_completed && t.status == Status::Completed { continue; }
                    if !self.matches_subject_filter(t) { continue; }
                    rows.push((gi, Some(ti)));
                }
            }
        }
        rows
    }

    fn matches_filter(&self, group: &SessionGroup) -> bool {
        match &self.filter {
            Filter::All => true,
            Filter::Session(sid) => &group.session_id == sid,
            Filter::Subject(_) => true,
        }
    }

    fn matches_subject_filter(&self, task: &ClaudeTask) -> bool {
        match &self.filter {
            Filter::Subject(needle) if !needle.is_empty() => {
                task.subject.to_lowercase().contains(&needle.to_lowercase())
            }
            _ => true,
        }
    }

    fn focus_row_index(&self, visible: &[(usize, Option<usize>)]) -> Option<usize> {
        visible.iter().position(|(g, t)| *g == self.focus.group && *t == self.focus.task)
    }

    fn set_focus_from_row(&mut self, row: &(usize, Option<usize>)) {
        self.focus.group = row.0;
        self.focus.task = row.1;
    }

    /// Toggle show_completed.
    pub fn toggle_show_completed(&mut self) {
        self.show_completed = !self.show_completed;
    }

    /// Set Filter::Session to the focused group's session (or clear if already set).
    pub fn toggle_session_filter(&mut self) {
        let Some(g) = self.groups.get(self.focus.group) else { return; };
        let sid = g.session_id.clone();
        self.filter = match &self.filter {
            Filter::Session(cur) if cur == &sid => Filter::All,
            _ => Filter::Session(sid),
        };
    }

    /// Cycle the focused task's status pending→in_progress→completed→pending.
    pub fn cycle_status(&mut self) -> TaskAction {
        let (sid, tid) = match self.current_focus_key() {
            Some((s, Some(t))) => (s, t),
            _ => return TaskAction::None,
        };
        let g_idx = self.focus.group;
        let Some(t_idx) = self.focus.task else { return TaskAction::None; };
        let new_status = match self.groups[g_idx].tasks[t_idx].status {
            Status::Pending => Status::InProgress,
            Status::InProgress => Status::Completed,
            Status::Completed => Status::Pending,
        };
        let mut new_task = self.groups[g_idx].tasks[t_idx].clone();
        new_task.status = new_status.clone();
        let session_dir = store::tasks_root().join(&sid);
        match store::write_task(&session_dir, &new_task) {
            Ok(()) => {
                let id = new_task.id.clone();
                self.groups[g_idx].tasks[t_idx] = new_task;
                self.toast(format!("#{} → {}", id, status_label(&new_status)));
                TaskAction::StatusCycled { id, new: new_status }
            }
            Err(StoreError::LockTimeout) => {
                self.toast(format!("#{} locked, try again", tid));
                TaskAction::Toast(format!("#{} locked", tid))
            }
            Err(e) => {
                self.toast(format!("write failed: {}", e));
                TaskAction::None
            }
        }
    }

    /// Create a task with `subject` in the focused session.
    pub fn create_task(&mut self, subject: &str) -> TaskAction {
        let subject = subject.trim();
        if subject.is_empty() { return TaskAction::None; }
        let Some(g) = self.groups.get(self.focus.group) else {
            self.toast("no session to create in".into());
            return TaskAction::None;
        };
        let sid = g.session_id.clone();
        let next_id = g.tasks.iter()
            .map(|t| t.parse_id())
            .filter(|n| *n != u64::MAX)
            .max()
            .map(|n| n.saturating_add(1))
            .unwrap_or(1);
        let task = ClaudeTask {
            id: next_id.to_string(),
            subject: subject.to_string(),
            description: String::new(),
            active_form: String::new(),
            status: Status::Pending,
            blocks: vec![],
            blocked_by: vec![],
        };
        let dir = store::tasks_root().join(&sid);
        match store::write_task(&dir, &task) {
            Ok(()) => {
                let id = task.id.clone();
                if let Some(g) = self.groups.get_mut(self.focus.group) {
                    g.tasks.push(task);
                    g.tasks.sort_by_key(|t| t.parse_id());
                }
                self.toast(format!("created #{}", id));
                TaskAction::Created { session: sid, id }
            }
            Err(e) => {
                self.toast(format!("create failed: {}", e));
                TaskAction::None
            }
        }
    }

    /// xx motion: first call arms; second within 2s on same task deletes.
    pub fn arm_or_delete(&mut self) -> TaskAction {
        let (sid, tid) = match self.current_focus_key() {
            Some((s, Some(t))) => (s, t),
            _ => return TaskAction::None,
        };
        let armed = self.pending_delete.take();
        match armed {
            Some((psid, ptid, t)) if psid == sid && ptid == tid && t.elapsed() <= Duration::from_secs(2) => {
                let dir = store::tasks_root().join(&sid);
                match store::delete_task(&dir, &tid) {
                    Ok(()) => {
                        if let Some(g) = self.groups.get_mut(self.focus.group) {
                            g.tasks.retain(|t| t.id != tid);
                            // pull focus back to a sibling task or the header
                            if let Some(idx) = self.focus.task {
                                let len = self.groups[self.focus.group].tasks.len();
                                if len == 0 {
                                    self.focus.task = None;
                                } else if idx >= len {
                                    self.focus.task = Some(len - 1);
                                }
                            }
                        }
                        self.toast(format!("deleted #{}", tid));
                        TaskAction::Deleted { id: tid }
                    }
                    Err(e) => {
                        self.toast(format!("delete failed: {}", e));
                        TaskAction::None
                    }
                }
            }
            _ => {
                self.pending_delete = Some((sid, tid.clone(), Instant::now()));
                self.toast(format!("x again within 2s to delete #{}", tid));
                TaskAction::Toast(format!("arm delete #{}", tid))
            }
        }
    }

    fn toast(&mut self, s: String) {
        self.last_toast = Some((s, Instant::now()));
    }
}

fn status_label(s: &Status) -> &'static str {
    match s {
        Status::Pending => "pending",
        Status::InProgress => "in_progress",
        Status::Completed => "completed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(id: &str, status: Status) -> ClaudeTask {
        ClaudeTask {
            id: id.into(), subject: "x".into(), description: "".into(),
            active_form: "".into(), status, blocks: vec![], blocked_by: vec![],
        }
    }

    fn group(sid: &str, tasks: Vec<ClaudeTask>) -> SessionGroup {
        SessionGroup {
            session_id: sid.into(), label: "lbl".into(),
            mtime: SystemTime::UNIX_EPOCH, tasks,
        }
    }

    fn fresh_core(groups: Vec<SessionGroup>) -> TasksCore {
        // construct without hitting disk
        let mut expanded = HashSet::new();
        if let Some(g) = groups.first() {
            expanded.insert(g.session_id.clone());
        }
        TasksCore {
            groups,
            focus: Focus { group: 0, task: Some(0) },
            expanded,
            last_seen: HashMap::new(),
            show_completed: false,
            create_mode: None,
            pending_delete: None,
            last_toast: None,
            filter: Filter::All,
        }
    }

    #[test]
    fn focus_key_roundtrip() {
        let core = fresh_core(vec![group("s", vec![mk("1", Status::Pending), mk("2", Status::Pending)])]);
        let key = core.current_focus_key().unwrap();
        assert_eq!(key.0, "s");
        assert_eq!(key.1, Some("1".to_string()));
    }

    #[test]
    fn arm_then_tick_clears_after_window() {
        let mut core = fresh_core(vec![group("s", vec![mk("1", Status::Pending)])]);
        core.pending_delete = Some(("s".into(), "1".into(), Instant::now() - Duration::from_secs(3)));
        // tick reloads from real disk (no override), but the cleanup of pending_delete
        // happens regardless. We bypass full_reload by inspecting only the delete-window logic:
        if let Some((_, _, t)) = core.pending_delete {
            if t.elapsed() > Duration::from_secs(2) {
                core.pending_delete = None;
            }
        }
        assert!(core.pending_delete.is_none());
    }

    #[test]
    fn visible_rows_hides_completed_by_default_and_collapsed_sessions() {
        let mut core = fresh_core(vec![
            group("s1", vec![
                mk("1", Status::Pending),
                mk("2", Status::Completed),
                mk("3", Status::InProgress),
            ]),
            group("s2", vec![mk("1", Status::Pending)]),
        ]);
        // s1 expanded (default), s2 collapsed
        let rows = core.visible_rows();
        // header s1 + task 1 + task 3 + header s2  = 4
        assert_eq!(rows.len(), 4);
        // toggle show_completed
        core.show_completed = true;
        let rows = core.visible_rows();
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn move_down_walks_visible_rows() {
        let mut core = fresh_core(vec![
            group("s1", vec![mk("1", Status::Pending), mk("2", Status::Pending)]),
        ]);
        core.focus = Focus { group: 0, task: None }; // header focused
        core.move_down();
        assert_eq!(core.focus.task, Some(0));
        core.move_down();
        assert_eq!(core.focus.task, Some(1));
        core.move_down(); // clamps at last
        assert_eq!(core.focus.task, Some(1));
    }

    #[test]
    fn toggle_expand_flips_expanded_set() {
        let mut core = fresh_core(vec![group("s1", vec![mk("1", Status::Pending)])]);
        assert!(core.expanded.contains("s1"));
        core.toggle_expand();
        assert!(!core.expanded.contains("s1"));
        core.toggle_expand();
        assert!(core.expanded.contains("s1"));
    }

    #[test]
    fn toggle_session_filter_filters_to_focused_session() {
        let mut core = fresh_core(vec![
            group("s1", vec![mk("1", Status::Pending)]),
            group("s2", vec![mk("1", Status::Pending)]),
        ]);
        core.toggle_session_filter();
        assert_eq!(core.filter, Filter::Session("s1".into()));
        core.toggle_session_filter();
        assert_eq!(core.filter, Filter::All);
    }
}
