# `tasks`: Claude Code task viewer + editor (glance panel + standalone)

**Date:** 2026-05-25 (revised after independent review)
**Status:** spec → plan
**Sibling of:** `health` (2026-05-23), `crew` (2026-05-23)
**Repo home:** `~/projects/glance` (third lib+bin sibling)

---

## Goal

Solve the "lost task list" problem with a live, cross-session view of the `~/.claude/tasks/` store. Ship a glance panel for at-a-glance presence on the dashboard plus a standalone editable TUI for deliberate work. Aggregate all sessions into one grouped, expandable list, label every row by project, and allow status cycle + create + delete with safe atomic writes that coordinate with Claude Code's own writes.

## Background

Claude Code writes per-session task files to `~/.claude/tasks/<session-id>/<task-id>.json`. There is no built-in UI to browse them across sessions, and 66 session dirs locally makes it easy to lose track. The store has hard concurrency constraints (Claude Code writes live; advisory `flock(2)` on a persistent `.lock` file; atomic rename expected) and no timestamps in the JSON (file mtime is the truth).

The widget is the third in the suite's dual-form action-launcher idiom (`health`, `crew`). It is **the first surface in the suite that writes user data**, so the safety boundary at the store layer is load-bearing, and the lock protocol must match Claude Code's actual on-disk scheme.

## Task data model

```rust
// src/tasks/task.rs
pub type SessionId = String;   // session uuid as string, e.g. "86588f23-3017-…"
pub type TaskId = String;      // numeric id as string, e.g. "4"

pub struct ClaudeTask {
    pub id: TaskId,
    pub subject: String,
    pub description: String,
    pub active_form: String,   // serde rename "activeForm"
    pub status: Status,
    pub blocks: Vec<TaskId>,
    pub blocked_by: Vec<TaskId>,   // serde rename "blockedBy"
}

pub enum Status {
    Pending,        // "pending"
    InProgress,     // "in_progress"
    Completed,      // "completed"
    // "deleted" is a write-only sentinel; we remove the file rather than write it.
}
```

`serde_json` with `#[serde(rename_all = "snake_case")]` on `Status`, explicit `rename` on `active_form` and `blocked_by`. Sort uses `id.parse::<u64>().unwrap_or(u64::MAX)` so unparseable ids sink to the bottom; an unparseable id is non-canonical Claude Code data that we render but don't blow up on.

## Architecture

Single core wrapped by two surfaces. The store layer is the safety boundary; every write funnels through it.

```
~/projects/glance/src/
├── lib.rs                 # + pub mod tasks
├── tasks/
│   ├── mod.rs             # TasksCore (state, focus, expansion, action enum,
│   │                      #   new(), tick(), handle_key(), cycle/create/delete)
│   ├── task.rs            # ClaudeTask, Status, SessionId, TaskId
│   ├── store.rs           # load_all_sessions(), write_task(), delete_task(),
│   │                      #   LockGuard (RAII flock release)
│   ├── session.rs         # session_id → project label resolver (+ cache,
│   │                      #   refresh_labels())
│   └── view.rs            # SessionGroup, render helpers shared by panel + bin
├── panels/tasks.rs        # TasksPanel: glance Panel impl
└── bin/tasks.rs           # standalone TUI; uses RunOutcome::Quit
```

### `TasksCore`

```rust
pub struct TasksCore {
    groups: Vec<SessionGroup>,
    focus: Focus,                          // (group_idx, Option<task_idx>)
    expanded: HashSet<SessionId>,          // single source of truth
    last_seen: HashMap<SessionId, SystemTime>,  // mtime-dirty cache
    show_completed: bool,
    create_mode: Option<String>,           // Some(buffer) while typing new subject
    pending_delete: Option<(SessionId, TaskId, Instant)>,
    last_toast: Option<(String, Instant)>,
    filter: Filter,                        // FilterAll | FilterSession(sid) | FilterSubject(needle)
}

pub enum TaskAction {
    None,
    StatusCycled { id: TaskId, new: Status },
    Created { session: SessionId, id: TaskId },
    Deleted { id: TaskId },
    Toast(String),
    Quit,
}

impl TasksCore {
    pub fn new() -> Self;                  // calls load_all_sessions(), falls
                                            //   back to empty on error + toast
    pub fn tick(&mut self);                // mtime-dirty reload; preserves focus
                                            //   by (SessionId, TaskId), not index
    pub fn handle_key(&mut self, key: KeyEvent) -> TaskAction;
    pub fn render(&self, frame: &mut Frame, area: Rect);
}
```

Focus survives reload by storing `(SessionId, TaskId)` not `(usize, usize)`, then resolving to indices each render.

### Two surfaces, shared core

**Panel (`panels/tasks.rs`):**
- `refresh_ms = 2000`. Tick reloads (mtime-dirty per session).
- `wants_keys = false` — Create / `n` is standalone-only, so the panel never needs to capture printable keys. Digit panel-switching is preserved.
- Handles (panel-local keys, must not collide with glance globals): `j`, `k`, `Up`, `Down` (move within visible rows), `o` (toggle expand on focused session header), `space` (cycle status on focused task).
- Does NOT handle: `Tab`, `Left`, `Right` (those are glance global panel-switch keys — `app.rs:209`); `n`, `xx`, `Enter`, `c`, `s`, `/` (standalone-only).

**Standalone (`bin/tasks.rs`):**
- 1000ms tick; full-screen layout (header / list / footer); owns its own event loop, so `Tab`, `Left`, `Right`, arrows are all available.
- Handles: everything the panel handles, plus `Tab` (toggle expand, same as `o`), `n` (create), `xx` (delete with 2s confirm window), `Enter` (detail modal), `c` (toggle show_completed), `s` (filter to focused session), `/` (substring filter on subject), `r` (force reload + label-cache refresh), `?` (help modal), `q` (quit).
- `create_mode = Some(_)` captures all printable keys + Enter/Esc while active; otherwise normal binding table.
- On `q`, persists state to `~/.config/glance/tasks.toml` (TOML, matching health's pattern). Fields: `expanded: Vec<SessionId>` and `show_completed: bool`.

## Store layer (`store.rs`)

The single safety boundary. Coordinates with Claude Code via `flock(2)` on the persistent `.lock` file each session dir already has.

```rust
pub fn load_all_sessions() -> Result<(Vec<SessionGroup>, Vec<String>)>;
//                                                       ^^^ skipped-file toasts
pub fn write_task(session: &SessionId, task: &ClaudeTask) -> Result<()>;
pub fn delete_task(session: &SessionId, id: &TaskId) -> Result<()>;
```

### Lock protocol (matches Claude Code's scheme on disk)

Claude Code keeps a **persistent** `.lock` file in every session dir (verified: 64/64 sessions have one with mtime = session creation time, never removed). The locking is advisory via `flock(2)`. We coordinate with the same mechanism.

```rust
use fs2::FileExt;

struct LockGuard {
    file: std::fs::File,   // dropping the File releases the flock
}

fn acquire_lock(session_dir: &Path) -> Result<LockGuard, LockError> {
    let lock_path = session_dir.join(".lock");
    // Open the existing lock file (don't create_new — it's persistent).
    // create(true) is fine if the dir is fresh and the lock file doesn't exist yet.
    let file = OpenOptions::new()
        .create(true).read(true).write(true)
        .open(&lock_path)?;
    // try_lock_exclusive, retry with 50ms sleep up to 1s total
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(LockGuard { file }),
            Err(_) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(LockError::Timeout),
        }
    }
}
```

`LockGuard` does not remove the `.lock` file on Drop. flock release happens automatically when the `File` is closed (i.e. when `LockGuard` drops), regardless of panic. **The lock file itself stays on disk** — that's Claude Code's design, not ours to fight.

### `write_task` protocol

1. `let _guard = acquire_lock(&session_dir)?;` — flock-exclusive, RAII release. `_guard` binding (not `_`) so it lives through step 4. Constructed BEFORE any further fallible step.
2. Serialize task to JSON, write to `<session>/<id>.json.tmp` (truncate). `flush + sync_data` before close.
3. `fs::rename(<id>.json.tmp, <id>.json)` — atomic on the same filesystem.
4. Guard drops → flock released.

### `delete_task`

Same flock protocol. Inside the guard: `fs::remove_file(<id>.json)`. `NotFound` is success (the task was already removed).

### `load_all_sessions`

- `read_dir(~/.claude/tasks/)`. For each session subdir:
  - `dir.metadata().mtime()` → `SessionGroup.mtime`.
  - `read_dir(<session>)`. Skip every entry that:
    - is not a regular file (don't follow weird entries);
    - has a name starting with `.` (`.lock`, `.highwatermark`, etc.);
    - has a name ending in `.tmp` (someone's mid-write);
    - has a name not ending in `.json`.
  - For each remaining `<id>.json`: `read_to_string` + `serde_json::from_str`. On parse error: skip, push a toast string `"skipped <session-short>/<id>.json (parse error)"` into the returned `Vec<String>`.
- **Filter out sessions with zero loaded tasks** (don't render `· 0 active` headers for ~30 empty dirs).
- Sort surviving groups by `mtime` descending. Sort tasks within a group by numeric `id` ascending.
- Reads do NOT touch the flock. Worst case: a torn read on a file Claude Code wrote without rename → `serde_json` fails → we skip + toast, next tick repairs.

### Reload-if-changed

TasksCore owns `last_seen: HashMap<SessionId, SystemTime>`. On each tick:
- `read_dir(~/.claude/tasks/)`; for each session subdir, `metadata.mtime()`.
- If `last_seen[sid] == new_mtime` → reuse the cached `SessionGroup` from `groups`.
- If `new_mtime > last_seen[sid]` → re-parse that session's JSON files.
- If `sid` is new → parse it.
- If a previously-seen `sid` is absent → remove from `groups` (and from `expanded` set).

## Session resolver (`session.rs`)

```rust
pub fn label_for(session_id: &SessionId) -> String;
pub fn refresh_labels();   // clear cache; next label_for re-scans
```

The on-disk scheme is `~/.claude/projects/<slug>/<session>.jsonl`. The slug is `cwd` with `/`, `.`, `_` all collapsed to `-`, which is lossy and not cleanly invertible. Use the jsonl content instead:

1. Walk `~/.claude/projects/*/`. For each file `<sid>.jsonl`, read up to the first **20 lines**. Find the first line whose top-level JSON object has a string field `cwd`. Take its basename. Cache `sid -> basename`.
2. If no `cwd` found in the first 20 lines, cache `sid -> "<slug>"` (the dirname, with dashes left in — at least informative, even if lossy).
3. If `<sid>.jsonl` isn't found anywhere, return `&session_id[..8]` (8-char prefix).

Cache lives in `OnceLock<Mutex<HashMap<SessionId, String>>>`. `refresh_labels()` clears it; the `r` key in the standalone calls it before kicking a reload.

Reasoning: line-1 is a `last-prompt` record with no cwd in the 3 sampled files; scanning 20 lines is cheap and tolerates Claude Code's evolving JSONL schema.

## View layer (`view.rs`)

```rust
pub struct SessionGroup {
    pub session_id: SessionId,
    pub label: String,          // resolved project label
    pub mtime: SystemTime,
    pub tasks: Vec<ClaudeTask>,
    // No `expanded` field — TasksCore owns the HashSet of expanded session ids.
}

pub fn count_active(group: &SessionGroup) -> usize;
pub fn is_blocked(task: &ClaudeTask, all_in_session: &[ClaudeTask]) -> Option<Vec<TaskId>>;
pub fn render_group(area: Rect, group: &SessionGroup, focus: Focus,
                    expanded: bool, width_class: WidthClass) -> Vec<Line>;
```

`count_active` counts `status != Completed`; not affected by `show_completed` (the toggle filters what renders, not the count).

`is_blocked` returns `Some([open blocker ids])` if any id in `blocked_by` matches a task in the **same session** whose status is `Pending` or `InProgress`; otherwise `None`.

## Edit semantics

### Status cycle (`space`)

- `Pending → InProgress → Completed → Pending`. Skips `deleted` (that's `xx`).
- Calls `store::write_task` with the new status.
- On success: optimistic in-memory update + toast `"#4 → in_progress"`.
- On `LockError::Timeout`: toast `"#4 locked, try again"`, no in-memory change.
- Available in both panel and standalone.

### Create (`n`, standalone only)

- Sets `create_mode = Some(String::new())`. Footer renders an input bar: `new task in <session-label>: <buffer>_`.
- All printable keypresses append; `Backspace` pops; `Esc` cancels (clears `create_mode`); `Enter` submits an empty subject as a no-op (cancel) to avoid blank rows.
- Anchored on the focused session. If `focus.0 = i`, the create lands in `groups[i].session_id` regardless of whether `focus.1` is `Some` or `None` (header-focus and task-focus both write to that session).
- Submit path: `id = max(parse-as-u64 of existing ids) + 1` (or `1` if empty), `status = Pending`, other fields empty (`""` / `[]`). Call `store::write_task`. Toast `"created #N"`.

### Delete (`xx`, standalone only)

- The `xx` motion is a 2-press sequence with a 2s window.
- First `x` on a focused task: set `pending_delete = Some((sid, id, Instant::now()))`. Toast `"x again within 2s to delete #N"`.
- Second `x` within 2s on the same task: call `store::delete_task` (which `rm`s the file). Toast `"deleted #N"`.
- Any other keypress, focus change, or `>2s` elapsed clears `pending_delete`.
- Help text and toast wording both say "within 2s" (consistent).

## Display details

### Status glyph
- `○` pending
- `◐` in_progress
- `✓` completed
- (`deleted` = file absent = no row)

### Row format

```
[glyph] #id  subject                        ⛔#3,#5
```

- `⛔#3,#5` appears only if `is_blocked(task, all_in_session)` returns `Some`. Auto-clears as blockers complete.
- Subject truncated to `cols - (id_width + 4 + marker_width)`, ellipsis on overflow. `marker_width = 0` when no `⛔` marker, else `len("⛔#x,y,...") + 2`.

### Session header

- Expanded: `▾ skai-work · 3 active`
- Collapsed: `▸ skai-work · 3 active`
- Focused header (navigation is on the header itself, not a task within): pink bold. Unfocused: dim lavender.

### Detail modal (standalone, `Enter`)

Full-screen modal:
```
┌─ #4 · skai-work ─────────────────────────┐
│ Status: in_progress                       │
│                                           │
│ Subject:   add validation to form          │
│ Active:    Adding validation               │
│                                           │
│ Description:                               │
│   <wraps the description body>             │
│                                           │
│ Blocks:    #5 build submit handler         │
│ Blocked by: #3 design schema [open]        │
│                                           │
│ Esc / Enter / q to close                   │
└───────────────────────────────────────────┘
```

Blocker rows look up the blocker's subject from the same session and tag `[open]` or `[done]`.

## Mobile / narrow-width layout

Detected via `frame.size().width`.

| Width | Behavior |
|---|---|
| ≥ 80 | Full layout: header, list with `⛔` markers, count in session header |
| 60–79 | Same as ≥80 but subject truncates tighter |
| 40–59 | Drop `⛔` markers (Enter shows blockers). Keep `· N active` counts. |
| < 40 | Glyph + id + truncated subject only. Drop `· N active`. Detail modal becomes full-screen, paginated. |

Footer always visible.

## Keybindings

### Footer

**Standalone** (the bin's full-screen footer, always visible):

```
space cycle · Tab/o collapse · Enter detail · n new · xx delete · / filter · ? help · q quit
```

**Glance panel** (rendered by glance's footer line when this panel is focused):

```
space cycle · o collapse · j/k move
```

`q` in the panel is glance's global quit, not panel-local. `n`, `xx`, `Enter`, `c`, `s`, `/`, `?`, `r`, `Tab` are standalone-only.

### Full bindings (in `?` help, standalone only)

```
NAVIGATION
  j / ↓        next visible row (skips collapsed session contents)
  k / ↑        prev visible row
  Tab / o      toggle expand on focused session header
  g            top
  G            bottom
  s            filter to focused session (toggle)
  /            substring filter on subject (Esc clears)

EDIT
  space        cycle status (pending → in_progress → completed → pending)
  n            new task in focused session
  xx           delete (press x twice within 2s)

VIEW
  c            toggle show completed (default: hide)
  r            force reload + label-cache refresh
  Enter        detail modal for focused task

EXIT
  q            quit; persist expanded + show_completed
  Esc          cancel input mode / dismiss modal
```

## Error handling

| Failure | Behavior |
|---|---|
| Corrupt JSON in a task file | Skip file, toast `"skipped <session-short>/<id>.json (parse error)"` once per reload |
| `flock` 1s timeout | Toast `"locked, try again"`, no state change |
| Missing or unreadable `<session>.jsonl` | Fallback chain: cwd-from-content → slug-as-label → 8-char session-id prefix |
| Permission denied on a session dir | Skip session, toast once per session per process |
| Empty store, or all sessions are zero-task | Render placeholder: `"no tasks. press n to create one."` (standalone) / `"no tasks"` (panel) |
| `fs::rename` fails (cross-fs / OS error) | Clean up `.tmp`, toast `"write failed: <err>"`, no change |
| `~/.claude/tasks/` missing entirely | Render `"no Claude Code tasks found"`; first `write_task` will fail until Claude Code creates the dir |
| Session dir with only `.lock`/`.highwatermark`, no `.json` | Filter out — don't render a `· 0 active` header |
| Stale `<id>.json.tmp` on disk | Skip during read (already covered by the `.tmp` skip rule); overwritten on next `write_task` |

## Testing

Inline `#[cfg(test)] mod tests` blocks per module, matching the sibling pattern (`health`, `crew` both inline-test). No `tests/` directory.

### `store.rs` tests
- Round-trip: `write_task` → `load_all_sessions` returns the same task.
- Atomic-rename: simulate a panic between write-tmp and rename (use a flag-controlled wrapper); assert the original file remains intact.
- flock contention: spawn a second `acquire_lock` on the same dir from a thread while the first holds; assert second times out with `LockError::Timeout` after ~1s.
- Skip rules: drop a `.lock`, a `.highwatermark`, a `foo.tmp`, a `notes.txt`, and a `4.json` in a temp session dir; assert only `4.json` is loaded.
- Corrupt JSON: write garbage to `5.json`; assert `load_all_sessions` skips it and returns a `"parse error"` string in the toast vec.
- Delete: `delete_task` removes the file; second call is `Ok(())` (NotFound is success).

### `session.rs` tests
- Reads a temp `<sid>.jsonl` where line 1 has no `cwd` and line 4 has `cwd: /tmp/foo`. Asserts `label_for(sid) == "foo"`.
- Fallback to slug when no line has `cwd`: returns the dirname string.
- Fallback to 8-char prefix when no jsonl exists at all.
- Cache: second call after writing a different `cwd` to the jsonl still returns the cached value; `refresh_labels()` + label_for picks up the new one.

### `view.rs` tests
- `is_blocked` returns `Some` when a blocker is Pending or InProgress; `None` otherwise.
- `count_active` excludes Completed.

### `mod.rs` (TasksCore) tests
- Cycle order: Pending → InProgress → Completed → Pending.
- Create assigns `max(id) + 1`; empty session assigns `1`.
- Create with empty subject is a no-op.
- Delete excises from the focused group; second `x` outside the 2s window clears `pending_delete`.
- Focus survives a reload that adds tasks above the focused one (focus tracks by `(SessionId, TaskId)`, not index).
- Expanded set survives a reload; sessions that vanish are pruned from the set.

### Integration smoke (manual, after build + install)
- Run `tasks` against the real `~/.claude/tasks/`: eyeball that sessions load, sorted newest first, with project labels resolved (most non-empty sessions show a real project name, not the 8-char fallback).
- Pick a throwaway session; `space` cycles a task; confirm the JSON on disk has the new `"status": "in_progress"`.
- `n`, type `"smoke test task"`, Enter; confirm a new `<max+1>.json` appears with `status: "pending"`.
- `xx` on it within 2s; confirm the file is gone.
- Launch `glance`, switch to the `tasks` panel; confirm tile rendering and that `space` cycles status.
- Mobile widths: `COLUMNS=32 ./target/release/tasks`, `COLUMNS=58`, `COLUMNS=120` — verify markers, counts, modal full-screen behavior.

## Suite registration

`~/projects/dashboard-suite/suite.toml`:

```toml
[[launcher]]
name = "tasks"
summary = "Claude Code task viewer + editor"
repo = "glance"
package = "glance"
artifact = "tasks"
bin = "tasks"
requires = []
default = false

[[panel]]
name = "tasks"
summary = "Claude Code tasks (cross-session)"
default = true
```

`~/projects/glance/src/panels/mod.rs`:
- `pub mod tasks;`
- Add `"tasks"` to `DEFAULT_ORDER` and `ALL_PANELS`.
- Add `"tasks" => Box::new(tasks::TasksPanel::new()),` to `build_panel`.

`~/projects/dashboard-suite/ROADMAP.md`: add a `tasks` entry under "shipped" after merge.

## File plan

| File | Action | Purpose |
|---|---|---|
| `src/lib.rs` | Modify | `pub mod tasks;` |
| `src/tasks/mod.rs` | Create | `TasksCore`, `TaskAction`, `Focus`, `Filter` |
| `src/tasks/task.rs` | Create | `ClaudeTask`, `Status`, `SessionId`, `TaskId` |
| `src/tasks/store.rs` | Create | `load_all_sessions`, `write_task`, `delete_task`, `LockGuard` (flock-based) |
| `src/tasks/session.rs` | Create | `label_for`, `refresh_labels`, cwd-from-content resolver + cache |
| `src/tasks/view.rs` | Create | `SessionGroup`, render helpers, `is_blocked`, `count_active` |
| `src/panels/tasks.rs` | Create | `TasksPanel: Panel` |
| `src/panels/mod.rs` | Modify | register module, `DEFAULT_ORDER`, `ALL_PANELS`, `build_panel` |
| `src/bin/tasks.rs` | Create | standalone TUI on `TasksCore` (auto-registered by Cargo from `src/bin/`) |
| `Cargo.toml` | Modify | add `fs2 = "0.4"` to `[dependencies]` (the only new dep) |
| `~/projects/dashboard-suite/suite.toml` | Modify | register launcher + panel |
| `~/projects/dashboard-suite/ROADMAP.md` | Modify | shipped entry after merge |

**No `[[bin]]` table change to `Cargo.toml`** — Cargo auto-registers `src/bin/<name>.rs` as a binary, matching how `health` and `crew` are wired.

## Out of scope (v1)

- In-place subject / description / activeForm editing
- `blocks` / `blockedBy` graph editing
- Real-time inotify (polling is sufficient for the cadence Claude Code writes)
- Multi-select / bulk operations
- Search-as-you-type across all sessions (`/` is intra-view substring filter, not a cross-store index)
- Drag-reorder of tasks (numeric id = insertion order is fine)
- Slug-inverse decoding (`-home-jane--config-thelma-...` is genuinely lossy; we read cwd from content instead)
- Force-steal stale locks (Claude Code's `.lock` files are persistent by design; we just contend politely)

## Open questions

None — all forks resolved during brainstorming, and all review-blocking findings folded into this revision:
- Lock scheme matches Claude Code (flock on persistent `.lock`, `fs2` dep added).
- Session resolver reads cwd from jsonl content, not first line; fallback chain documented.
- Panel keybindings cleared of glance-global collisions (no `Tab`, no `Left`/`Right`); standalone keeps them via its own event loop.
- Cargo `[[bin]]` claim removed; auto-registration matches siblings.
- Empty-session dirs filtered out before render.
- `expanded` lives only on `TasksCore`, not duplicated on `SessionGroup`.
- `TasksCore::new()` and `tick()` signatures specified.
- Persistence is `.toml` (matching `health`).
- Inline `#[cfg(test)] mod tests` (matching siblings), not a `tests/` dir.

## Review log

- 2026-05-25, drafted from brainstorm forks (aggregate-all + status+delete+create + name=tasks + grouped layout + panel cycles status).
- 2026-05-25, independent review verdict **RED** — six blocking findings on lock-scheme, jsonl first-line cwd, slug invertibility, glance global key collisions, Cargo `[[bin]]` misclaim, and empty-session handling. Twelve SHOULD-FIX and nine NIT findings folded in.
- 2026-05-25, revision committed; ready for plan.

