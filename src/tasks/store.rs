//! Store layer: load + atomic-write task files under `~/.claude/tasks/`.
//!
//! Coordinates with Claude Code's live writes via `flock(2)` on the
//! persistent `.lock` file each session dir already has (see spec
//! `docs/superpowers/specs/2026-05-25-tasks-design.md`). Writes are
//! atomic via temp + rename. Reads are lock-free; corrupt files are
//! skipped with a toast.

use crate::tasks::task::{ClaudeTask, SessionId, TaskId};
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    LockTimeout,
    Serde(serde_json::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {}", e),
            Self::LockTimeout => write!(f, "lock timeout"),
            Self::Serde(e) => write!(f, "serde: {}", e),
        }
    }
}

impl std::error::Error for StoreError {}
impl From<std::io::Error> for StoreError { fn from(e: std::io::Error) -> Self { Self::Io(e) } }
impl From<serde_json::Error> for StoreError { fn from(e: serde_json::Error) -> Self { Self::Serde(e) } }

pub struct LoadedSession {
    pub session_id: SessionId,
    pub mtime: SystemTime,
    pub tasks: Vec<ClaudeTask>,
}

pub struct LoadResult {
    pub sessions: Vec<LoadedSession>,
    pub toasts: Vec<String>,
}

/// Resolve `~/.claude/tasks/`.
pub fn tasks_root() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".claude/tasks")
}

/// RAII flock release: drops the `File`, which closes the fd and releases the lock.
pub struct LockGuard {
    _file: File,
}

/// Acquire an exclusive flock on `<session_dir>/.lock`. Retries every 50ms up to 1s.
pub fn acquire_lock(session_dir: &Path) -> Result<LockGuard, StoreError> {
    fs::create_dir_all(session_dir)?;
    let lock_path = session_dir.join(".lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(LockGuard { _file: file }),
            Err(_) => {
                if Instant::now() >= deadline {
                    return Err(StoreError::LockTimeout);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// Atomic write of a task to `<session_dir>/<id>.json`. Acquires flock, writes
/// to a `.tmp`, renames over the final path.
pub fn write_task(session_dir: &Path, task: &ClaudeTask) -> Result<(), StoreError> {
    let _guard = acquire_lock(session_dir)?;
    let final_path = session_dir.join(format!("{}.json", task.id));
    let tmp_path = session_dir.join(format!("{}.json.tmp", task.id));
    let json = serde_json::to_vec_pretty(task)?;
    {
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)?;
        f.write_all(&json)?;
        f.sync_data()?;
    }
    fs::rename(&tmp_path, &final_path)?;
    Ok(())
}

/// Delete a task file. `NotFound` is treated as success.
pub fn delete_task(session_dir: &Path, id: &TaskId) -> Result<(), StoreError> {
    let _guard = acquire_lock(session_dir)?;
    let path = session_dir.join(format!("{}.json", id));
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(StoreError::Io(e)),
    }
}

/// Walk every session dir under `tasks_root()` and load their task files.
/// Filters: hidden files (`.lock`, `.highwatermark`, etc.), `.tmp`, non-`.json`.
/// Empty sessions (zero loaded tasks) are dropped. Sessions sorted by mtime desc;
/// tasks within sorted by numeric id asc.
pub fn load_all_sessions() -> LoadResult {
    load_sessions_from(&tasks_root())
}

/// Walk the given root (used by `load_all_sessions` and tests).
pub fn load_sessions_from(root: &Path) -> LoadResult {
    let mut sessions = Vec::new();
    let mut toasts = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return LoadResult { sessions, toasts },
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Ok(meta) = entry.metadata() else { continue; };
        if !meta.is_dir() { continue; }
        let sid = entry.file_name().to_string_lossy().to_string();
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let mut tasks = Vec::new();
        let Ok(rd) = fs::read_dir(&dir) else { continue; };
        for f in rd.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if name.starts_with('.') { continue; }
            if name.ends_with(".tmp") { continue; }
            if !name.ends_with(".json") { continue; }
            let path = f.path();
            match fs::read_to_string(&path) {
                Ok(s) => match serde_json::from_str::<ClaudeTask>(&s) {
                    Ok(t) => tasks.push(t),
                    Err(_) => {
                        let short: String = sid.chars().take(8).collect();
                        toasts.push(format!("skipped {}/{} (parse error)", short, name));
                    }
                },
                Err(_) => {
                    let short: String = sid.chars().take(8).collect();
                    toasts.push(format!("skipped {}/{} (read error)", short, name));
                }
            }
        }
        if tasks.is_empty() {
            continue; // filter out zero-task sessions
        }
        tasks.sort_by_key(|t| t.parse_id());
        sessions.push(LoadedSession { session_id: sid, mtime, tasks });
    }
    sessions.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    LoadResult { sessions, toasts }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::task::Status;
    use tempfile::TempDir;

    fn mk_task(id: &str, subj: &str, status: Status) -> ClaudeTask {
        ClaudeTask {
            id: id.into(),
            subject: subj.into(),
            description: "".into(),
            active_form: "".into(),
            status,
            blocks: vec![],
            blocked_by: vec![],
        }
    }

    #[test]
    fn write_then_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let session = tmp.path().join("sess");
        let t = mk_task("4", "hello", Status::Pending);
        write_task(&session, &t).unwrap();
        let raw = fs::read_to_string(session.join("4.json")).unwrap();
        let parsed: ClaudeTask = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed, t);
        // tmp file should be gone after rename
        assert!(!session.join("4.json.tmp").exists());
    }

    #[test]
    fn delete_removes_file_and_tolerates_missing() {
        let tmp = TempDir::new().unwrap();
        let session = tmp.path().join("s");
        let t = mk_task("9", "x", Status::Pending);
        write_task(&session, &t).unwrap();
        assert!(session.join("9.json").exists());
        delete_task(&session, &"9".to_string()).unwrap();
        assert!(!session.join("9.json").exists());
        delete_task(&session, &"9".to_string()).unwrap(); // NotFound = OK
    }

    #[test]
    fn lock_contention_times_out_under_1500ms() {
        // Hold the lock from a background thread; main thread sees timeout.
        let tmp = TempDir::new().unwrap();
        let session = tmp.path().join("locked-session");
        fs::create_dir_all(&session).unwrap();
        let session_clone = session.clone();
        let (start_tx, start_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let g = acquire_lock(&session_clone).unwrap();
            start_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            drop(g);
        });
        start_rx.recv().unwrap();
        let started = Instant::now();
        let r = acquire_lock(&session);
        let elapsed = started.elapsed();
        release_tx.send(()).unwrap();
        assert!(matches!(r, Err(StoreError::LockTimeout)));
        assert!(elapsed >= Duration::from_millis(900));
        assert!(elapsed < Duration::from_millis(1500), "took {:?}", elapsed);
    }

    #[test]
    fn load_skips_lock_tmp_highwatermark_and_non_json() {
        let tmp = TempDir::new().unwrap();
        let sess = tmp.path().join("sess-1");
        fs::create_dir_all(&sess).unwrap();
        fs::write(sess.join(".lock"), "").unwrap();
        fs::write(sess.join(".highwatermark"), "").unwrap();
        fs::write(sess.join("foo.tmp"), "{}").unwrap();
        fs::write(sess.join("notes.txt"), "x").unwrap();
        let real = mk_task("1", "real", Status::Pending);
        fs::write(sess.join("1.json"), serde_json::to_string(&real).unwrap()).unwrap();
        let result = load_sessions_from(tmp.path());
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.sessions[0].tasks.len(), 1);
        assert_eq!(result.sessions[0].tasks[0].id, "1");
    }

    #[test]
    fn load_filters_empty_sessions() {
        let tmp = TempDir::new().unwrap();
        let empty = tmp.path().join("empty-session");
        fs::create_dir_all(&empty).unwrap();
        fs::write(empty.join(".lock"), "").unwrap();
        let result = load_sessions_from(tmp.path());
        assert_eq!(result.sessions.len(), 0);
    }

    #[test]
    fn load_corrupt_json_yields_toast_and_skips() {
        let tmp = TempDir::new().unwrap();
        let sess = tmp.path().join("sess");
        fs::create_dir_all(&sess).unwrap();
        fs::write(sess.join("1.json"), "not json").unwrap();
        let real = mk_task("2", "real", Status::Pending);
        fs::write(sess.join("2.json"), serde_json::to_string(&real).unwrap()).unwrap();
        let result = load_sessions_from(tmp.path());
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.sessions[0].tasks.len(), 1);
        assert_eq!(result.sessions[0].tasks[0].id, "2");
        assert!(result.toasts.iter().any(|t| t.contains("parse error")));
    }

    #[test]
    fn load_sorts_sessions_by_mtime_desc_tasks_by_numeric_id_asc() {
        let tmp = TempDir::new().unwrap();
        let s1 = tmp.path().join("aaa");
        let s2 = tmp.path().join("bbb");
        for s in [&s1, &s2] {
            fs::create_dir_all(s).unwrap();
            let t10 = mk_task("10", "ten", Status::Pending);
            let t2 = mk_task("2", "two", Status::Pending);
            fs::write(s.join("10.json"), serde_json::to_string(&t10).unwrap()).unwrap();
            fs::write(s.join("2.json"), serde_json::to_string(&t2).unwrap()).unwrap();
        }
        std::thread::sleep(Duration::from_millis(20));
        fs::write(s2.join("2.json"), serde_json::to_string(&mk_task("2", "two!", Status::Pending)).unwrap()).unwrap();
        let result = load_sessions_from(tmp.path());
        assert_eq!(result.sessions.len(), 2);
        assert_eq!(result.sessions[0].tasks[0].id, "2");
        assert_eq!(result.sessions[0].tasks[1].id, "10");
        // newest mtime first
        assert_eq!(result.sessions[0].session_id, "bbb");
    }
}
