# Design: `health` — Custom goals tracker (standalone binary + glance panel)

**Date:** 2026-05-23
**Status:** approved-pending-review
**Repo:** `~/projects/glance` (lib+bin), feature branch `health`

## Context

`health` is the suite's flagship goals tracker. Today, glance has two narrow
trackers it replaces:

- `peon` — reads peon-ping's `~/.claude/hooks/peon-ping/.state.json` (`trainer.reps` /
  `trainer.date`) and `config.json` (`trainer.exercises`). Trainer is currently
  `enabled: false` with no `trainer.reps` present.
- `water` — owns `~/.local/share/glance/water.json` (`{date, glasses}`), `+`/`-`/`R` keys,
  midnight rollover.

`health` generalizes both into a configurable, multi-activity, multi-day tracker that
ships in BOTH suite forms (decided 2026-05-23):

- **standalone binary** `health` — an always-on single-surface tile with view switching
  and inline logging. Run in its own tmux pane / on the dashboard / over SSH on mobile.
- **glance panel** `health` — the same tracker as a panel inside glance.

Both forms are thin shells over one shared `HealthCore`. No logic is duplicated.

## Goals / non-goals

**Goals**
- One configurable goals system: arbitrary activities, each with a daily goal, a unit
  string, and an optional weekly target.
- Inline logging (no shell trip): quick `+`/`-` on a focused activity, plus a typed-count
  bulk-log path.
- Multi-day history persisted append-only, powering weekly bars, a 30-day sparkline grid,
  and all-time totals.
- Four views toggled with `v`: Today / Weekly / 30-day grid / All-time.
- Clean retirement of `peon` and `water` with a one-time, non-destructive data import.

**Non-goals (v1)**
- No editing of past events from the UI (append-only; edit the jsonl by hand if needed).
- No per-activity reminders/notifications (peon-ping still owns nudges).
- No sync/cloud; local files only (XDG).
- No plugin activities beyond what `health.toml` declares.

## Decisions (locked 2026-05-23)

1. **Architecture:** shared `health` module inside the glance crate + a second `health`
   bin target. (Not a separate repo; not data-only sharing.)
2. **peon + water:** retire now, with a one-time non-destructive import.
3. **Views:** ship all four.
4. **Data location:** glance namespace — `~/.config/glance/health.toml`,
   `~/.local/share/glance/health.jsonl`.
5. **Starter activities seeded on first run:**
   - pushups — goal 10, unit `reps`
   - squats — goal 10, unit `reps`
   - bike — goal 30, unit `minutes`
   - walking — goal 30, unit `minutes`
   - water — goal 8, unit `glasses`
6. **Today "total":** average of each activity's capped completion % (unit-safe; raw sums
   across reps/minutes/glasses would be meaningless).
7. **Streak:** consecutive days, ending today (or yesterday if today is not yet complete),
   where EVERY configured activity reached 100% of its goal.
8. **Bulk-log key:** `L` opens the typed-count modal. (`+`/`-` are quick ±1 on the focus.)

## Architecture

### Enabling refactor: glance becomes lib + bin

`src/bin/health.rs` (Cargo auto-detects it as a second binary) can only see the package
*library*, not `main.rs`'s module tree. So:

- Add `src/lib.rs` that holds the `pub mod` declarations currently inlined at the top of
  `main.rs` (`app`, `brightness`, `config`, `footer`, `header`, `layout`, `panels`,
  `theme`, `widgets`, and the new `health`).
- `main.rs` shrinks to `use glance::{...};` + arg parsing + raw-mode setup (unchanged
  behavior). Module files are NOT edited — their internal `crate::…` paths resolve against
  the lib crate root just as before.
- Side benefit: glance is now unit-testable as a library.

`centered_rect` (currently private in `app.rs`) is lifted to `pub` (in `app` or a small
`ui` helper) so the standalone binary can reuse it for modals.

### The shared brain: `src/health/`

```
src/health/
  mod.rs        # HealthCore + HealthView; the form-agnostic brain
  config.rs     # HealthConfig + Activity  <- ~/.config/glance/health.toml
  store.rs      # HealthStore (append-only jsonl) + aggregation
  view.rs       # render fns per view (Today / Weekly / Grid / AllTime)
  log_entry.rs  # LogInput state machine (form-agnostic)
  migrate.rs    # one-time peon/water import
```

`HealthCore` owns: `HealthConfig`, `HealthStore` (loaded events), current `HealthView`,
and `LogInput`. Public surface:

- `HealthCore::new() -> Self` — load config (seed+write starter if absent), run one-time
  migration if marker absent, load store.
- `tick(&mut self)` — cheap; re-read store if the jsonl mtime changed (so the panel and a
  separate standalone process stay roughly in sync), handle midnight rollover for "today".
- `handle_key(&mut self, KeyEvent) -> bool` — returns true if consumed. Drives `v` view
  cycle, activity focus (`j`/`k`), quick `+`/`-`, and the `L` typed-count modal.
- `is_capturing(&self) -> bool` — true while `LogInput` is mid-typed-count (see panel hook).
- `render(&self, &mut Frame, Rect)` — header + active view + (modal overlay if logging).

### Two thin shells

- `src/panels/health.rs`: `HealthPanel { core: HealthCore }` implements `Panel`
  (`name()`="health", `tick`/`render`/`handle_key` delegate to `core`, and a new
  `wants_keys()` delegates to `core.is_capturing()`).
- `src/bin/health.rs`: a small single-surface event loop (no panel switching) modeled on
  `app::run`: ticks `core`, draws header/body/footer, routes keys to `core` first, handles
  `[`/`]` brightness, `?` help, `q`/Ctrl-C quit. Reuses `theme`, `brightness`, `footer`,
  `centered_rect` from the lib.

### The capture hook (glance global-key fix)

glance's `app::handle_key` consumes digits (panel switch), `n`/`p`, `r`, `[`/`]` BEFORE the
active panel sees them, so a panel cannot read a typed count. Fix:

- Add to the `Panel` trait: `fn wants_keys(&self) -> bool { false }` (default off).
- In `app::handle_key`: if `state.panels[state.current].wants_keys()`, route the key to the
  panel via `handle_key` and return (skip all global handling) — except Ctrl-C, which always
  quits. The health panel returns `wants_keys()==true` only while typing a count, so normal
  panel navigation is unaffected.

This is small, default-off, and unblocks any future input panel (e.g. `timer`).

## Config + data model

### `~/.config/glance/health.toml`

```toml
[[activity]]
name = "pushups"
goal = 10
unit = "reps"
# weekly_target = 70   # optional

[[activity]]
name = "squats"
goal = 10
unit = "reps"

[[activity]]
name = "bike"
goal = 30
unit = "minutes"

[[activity]]
name = "walking"
goal = 30
unit = "minutes"

[[activity]]
name = "water"
goal = 8
unit = "glasses"
```

- `Activity { name: String, goal: f64, unit: String, weekly_target: Option<f64> }`.
- `goal`/`count` are `f64` to support fractional units later (e.g. 2.5 miles); display drops
  trailing `.0` via a small `fmt_count` helper.
- Missing/empty file -> seed the starter set above AND write it to disk so the user has
  something to edit. Order in the file is the display order.

### `~/.local/share/glance/health.jsonl`

Append-only, one JSON object per line:

```json
{"ts":1779520870,"date":"2026-05-23","activity":"pushups","count":25}
```

- `Event { ts: i64 (unix secs), date: String "YYYY-MM-DD" (local), activity: String, count: f64 }`.
- **Undo = append a negative-count event** (never mutate/rewrite the log). Keeps history
  honest and makes the 30-day series + all-time totals fall out of a single forward scan.
- Today's per-activity display value floors at 0.
- `date` is computed from local time at log moment (jiff `Zoned`), matching `water`'s
  `today_iso()`.

### Aggregation (in `store.rs`)

- `today(date) -> BTreeMap<activity, f64>` — sum counts where `date == today`.
- `weekly(activity, today) -> [f64; 7]` — last 7 local days (oldest..today).
- `daily_series(activity, today, n) -> Vec<f64>` — last `n` days for sparklines.
- `alltime(activity) -> { total: f64, best_day: f64, active_days: usize, avg_per_active_day: f64 }`.
- All from one `load()` of the jsonl into `Vec<Event>` (cheap at expected sizes; re-read on
  mtime change).

## Views (`v` cycles; `HealthView { Today, Weekly, Grid, AllTime }`)

- **Today** — a `Gauge` per activity (`done/goal` + %), colored by threshold reusing the
  peon/water palette: `<50%` lavender, `50–99%` pink, `100%` magenta. Footer line: total %
  (average of capped per-activity %) and current streak (`streak Nd`). The focused activity
  (for `+`/`-`/`L`) is marked.
- **Weekly** — one compact 7-bar row per activity (rolling last 7 days, oldest..today,
  weekday-labeled), goal as the reference; bars at/over goal in magenta. Units never mix because each row is one activity.
- **30-day grid** — one `Sparkline` row per activity over the last 30 days, labeled with
  the activity name.
- **All-time** — a small table per activity: total, best single day, avg per active day.

Narrow-width behavior: views degrade gracefully (drop labels, then bars) reusing glance's
shared `empty`/loading widgets where a view has nothing to show.

## Logging UX

- **Quick path:** Today view, `j`/`k` moves focus, `+` logs +1 of the focused activity,
  `-` logs -1 (undo). Mirrors how `water` works today.
- **Bulk path:** `L` opens the typed-count modal driven by `LogInput`:
  - `PickActivity` — numbered list (`1..N`) of activities; press the digit, or `j`/`k`+Enter.
  - `TypeCount { activity, buf }` — numeric buffer (digits + one `.`), Enter commits an
    append, Esc/`q` cancels, Backspace edits.
  - Commit appends an `Event` and reloads the in-memory store.
- In the **standalone**, the loop routes keys straight to `core`, so no special casing.
- In the **panel**, `core.is_capturing()` drives `Panel::wants_keys()` so glance hands the
  panel every key while typing a count.

## peon/water retirement + migration

- Remove from `src/panels/mod.rs`: the `pub mod peon;` / `pub mod water;` lines, their
  entries in `DEFAULT_ORDER` and `ALL_PANELS`, and their `build_panel` arms. Add `health`
  in their place (same slot region). Delete `src/panels/peon.rs` and `src/panels/water.rs`.
- Register `health` via `build_panel("health") => Box::new(health::HealthPanel::new())`.
- Update `main.rs` HELP and `config::write_template` so the generated `panels.toml` lists
  `health` and not `peon`/`water`.
- **One-time, non-destructive import** (`migrate.rs`), guarded by a marker file
  (`~/.local/share/glance/.health-migrated`) so it runs once:
  - Goals come entirely from the locked starter set (pushups/squats 10, bike/walking 30
    min, water 8), written by `config::load_or_seed`. Migration imports DATA only and never
    overrides goals; peon-ping's old 300/300 goals are intentionally ignored.
  - If `~/.local/share/glance/water.json` `date == today`, append a `water` event for its
    `glasses`.
  - If peon-ping `.state.json` has `trainer.reps` (currently absent), append today's reps
    as events for matching activities.
  - Leave `water.json` and the peon-ping files untouched; just stop reading them.
- Update `~/projects/dashboard-suite/suite.toml` (drop the `peon`/`water` panel entries,
  add a `health` panel entry, and add `health` as an installable tile binary built from the
  glance repo) and `ROADMAP.md` (mark health shipped; note peon/water retired).

## Testing

Unit tests colocated in each health module (buffer-scan render pattern from
`panels/detail.rs` tests):

- `config.rs` — parse a sample toml; missing file -> starter set; order preserved.
- `store.rs` — append + `today`/`weekly`/`daily_series`/`alltime` against a temp jsonl,
  including negative (undo) events; midnight boundary by `date` string.
- `log_entry.rs` — `LogInput` transitions: open -> pick -> type -> commit/cancel; bad input
  rejected; backspace.
- `view.rs` — render each of the 4 views into a `Buffer`, assert activity names + a known
  number appear; narrow-width doesn't panic.
- streak math — all-met vs partial days; gap breaks the streak; today-incomplete falls back
  to counting through yesterday.
- `Panel::wants_keys` — health panel reports capturing only mid-type.

Then: `cargo test`, `cargo build --release`, install BOTH `glance` and `health` to
`~/.local/bin`, and a live tmux smoke: standalone `health` (log via `+` and via `L`, cycle
all four views, undo), and the glance `health` panel (`+`/`-` and `L` through the capture
hook).

## File-by-file change list

New:
- `src/lib.rs`
- `src/bin/health.rs`
- `src/health/{mod,config,store,view,log_entry,migrate}.rs`
- `src/panels/health.rs`
- `docs/superpowers/specs/2026-05-23-health-design.md` (this file)

Edited:
- `src/main.rs` — use the lib; HELP mentions health.
- `src/panels/mod.rs` — drop peon/water, add health; add the `Panel::wants_keys` default
  method (the `Panel` trait is defined in this file).
- `src/app.rs` — `pub` `centered_rect`; capture-hook branch in `handle_key`.
- `src/config.rs` — no change needed; `write_template` is parameterized by
  `panels::{ALL_PANELS, DEFAULT_ORDER}`, which already drop peon/water and add health.
- `Cargo.toml` — (lib target is implicit via `src/lib.rs`; `src/bin/health.rs` is auto.) Add
  `[lib]`/`[[bin]]` stanzas only if Cargo needs disambiguation.
- `~/projects/dashboard-suite/suite.toml` + `ROADMAP.md` (separate repo).

Deleted:
- `src/panels/peon.rs`, `src/panels/water.rs`.

## Open judgment calls (flag for review)

- Streak defined as all-activities-met (locked above) vs any-activity-logged. Chose all-met
  because the goals were deliberately set low/keepable.
- `peon-log-viz` mention in the roadmap stays separate (peon-ping sounds/log is its own
  system); `health` only absorbs the *tracker* panels, not peon-ping itself.
