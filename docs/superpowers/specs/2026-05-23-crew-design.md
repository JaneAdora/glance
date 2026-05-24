# Design: `crew` — background Claude Code session view (standalone + glance panel)

**Date:** 2026-05-23
**Status:** approved-pending-review
**Repo:** `~/projects/glance` (lib+bin), feature branch `crew`

## Context

`crew` is a live view of the user's **background Claude Code sessions** (daemon-backed
jobs). Each job writes a rich record at `~/.claude/jobs/<short>/state.json`:

- `state` ∈ {working, done, stopped, blocked, failed}
- `tempo` ∈ {active, idle, blocked}
- `inFlight.tasks` (count), `name`, `detail` (latest result/status line)
- `cwd`, `resumeSessionId`, `intent` (the original prompt), `createdAt`/`updatedAt`

There are ~51 such jobs today. This is an **operational** view ("what are my background
agents doing right now, and let me jump back into one"), distinct from `recall`, which
searches the historical LLM-summarized session index. `crew` reads the live job files
directly; no index, no MCP.

Ships in BOTH suite forms (the dual-form convention):
- **standalone `crew`** — a launcher: open, pick a session, `d` drops back in.
- **glance panel `crew`** — a live dashboard tile of the same list.

Both are thin shells over one shared `CrewCore`.

## Goals / non-goals

**Goals**
- List all background jobs, newest-first, with colored state badges + a live-only filter.
- `d` = resume a session with `--dangerously-skip-permissions` (drop straight in).
- `c` = copy the resume command to the clipboard (OSC 52) for paste on mobile/SSH.
- `Enter` = read-only detail (intent, detail, cwd, timestamps, session id).

**Non-goals (v1)**
- No killing/cancelling jobs from the UI (read + resume only).
- No `/` free-text search (51 jobs browse fine with `f` + `j/k`; deferred).
- No editing job state; no daemon control.
- Not a replacement for `recall` (historical search stays recall's job).

## Decisions (locked 2026-05-23)

1. **Home:** glance lib+bin — `src/crew/` shared core + `src/panels/crew.rs` + `src/bin/crew.rs`.
2. **Form:** both (standalone launcher + glance panel).
3. **Scope:** all jobs, newest-first by `updatedAt`, with an `f` filter to live-only.
4. **Name:** `crew` (avoids the `bg`/`jobs` shell-builtin collision).
5. **Resume command** always includes `--dangerously-skip-permissions`.
6. **Keys:** `d` = drop in (resume), `c` = copy, `Enter` = detail, `f` = filter, `j/k`/arrows, `q`.

## Architecture

`crew` lives in the glance crate (already lib+bin after `health`).

```
src/crew/
  mod.rs     # CrewCore + CrewAction; load/sort/filter/focus, handle_key -> action, render, detail modal
  job.rs     # Job struct, load_jobs(), is_live(), resume_command()/resume_parts(), humanize age
src/panels/crew.rs   # glance Panel: d = spawn in tmux (fallback copy), c = copy
src/bin/crew.rs      # standalone launcher: d = copy + PrintAndExit, c = copy
```

`CrewCore` owns: `Vec<Job>` (sorted newest-first), `focus: usize`, `filter_live: bool`,
`show_detail: bool`, a transient toast. Public surface:

- `CrewCore::new() -> Self` — `load_jobs()` + sort.
- `tick(&mut self)` — reload jobs (cheap: ~51 small files) so the panel stays current.
- `handle_key(&mut self, KeyEvent) -> CrewAction` — drives focus/filter/detail; returns an
  action for the shell to perform.
- `visible(&self) -> Vec<&Job>` — applies the live filter.
- `render(&self, &mut Frame, Rect)` — list + header + footer + optional detail modal.

```rust
pub enum CrewAction {
    None,
    Drop { command: String, cwd: Option<String>, claude: String }, // d
    Copy { command: String },                                       // c
}
```

The two shells interpret a `Drop`/`Copy` differently:
- **bin** (`src/bin/crew.rs`): `Copy` -> OSC 52 copy + toast; `Drop` -> OSC 52 copy AND
  `RunOutcome::PrintAndExit(command)` so the parent shell can `eval "$(crew)"`.
- **panel** (`src/panels/crew.rs`): `Copy` -> OSC 52 copy + toast; `Drop` -> if `$TMUX` is
  set, `tmux new-window -c <cwd> claude --resume <id> --dangerously-skip-permissions`
  (spawn it live from the dashboard); else OSC 52 copy + toast "no tmux: copied".

No glance capture-hook is needed: none of `crew`'s keys (`d`/`c`/`f`/`j`/`k`/Enter) collide
with glance's global keys (digits/`n`/`p`/`r`/`[`/`]`/`?`/`q`).

## Data model + source

```rust
pub struct Job {
    pub short: String,          // dir name, e.g. "31006806"
    pub name: String,           // "" if unnamed
    pub state: String,          // working|done|stopped|blocked|failed|...
    pub tempo: String,          // active|idle|blocked|...
    pub in_flight: u64,         // inFlight.tasks
    pub detail: String,         // latest result/status line
    pub cwd: String,
    pub resume_session_id: String,
    pub updated_at: String,     // ISO 8601
    pub intent: String,
}
```

- `load_jobs()` globs `~/.claude/jobs/*/state.json`, tolerant-parses each (missing fields
  default; a dir without state.json is skipped), returns newest-first by `updated_at`
  (ISO 8601 strings sort lexically; ties broken by `short`).
- `is_live(&self) -> bool` = `state ∈ {working, blocked}` OR `tempo == "active"` OR
  `in_flight > 0`.
- `resume_parts(&self) -> (Option<String>, String)` = (cwd-if-nonempty, `claude --resume
  <resume_session_id> --dangerously-skip-permissions`).
- `resume_command(&self) -> String` = `cd '<esc cwd>' && <claude cmd>` (single-quote-escaped,
  `'` -> `'\''`); bare claude cmd when cwd empty. Falls back to `short` if
  `resume_session_id` is empty.
- `age(now) -> String` humanizes `now - updated_at` as `s`/`m`/`h`/`d` (jiff Timestamp parse;
  "?" if unparseable).

## Rendering

**Header:** `claude bg sessions   <live> live · <total> total` (append ` · [live]` when the
filter is on).

**Row (per visible job):** `▸ ` focus marker · state glyph · name (or `(<short>)` if unnamed,
truncated) · state text · tempo · `N▸` when `in_flight>0` · age · dim `detail` snippet filling
the remaining width.

**State glyph + color:**
- working `●` pink (now) · done `✓` lavender (historical) · stopped `⏹` dim · blocked `◍`
  amber · failed `✗` magenta-bold (alert).

**Footer:** `d drop-in  ·  c copy  ·  enter detail  ·  f live  ·  q quit`.

**Detail modal (`Enter`, `centered_rect` 70x70):** name + short, state/tempo/inFlight, cwd,
session id, created/updated, the full `detail`, and the `intent` prompt (wrapped). `Enter`/
`Esc`/`q` closes. Empty list -> `widgets::empty("no background sessions")`.

## Glance panel specifics

Panel `crew`, `refresh_ms` 2000 (re-`load_jobs()` each tick; 51 small reads is cheap).
Registered in `src/panels/mod.rs` (`DEFAULT_ORDER`, `ALL_PANELS`, `build_panel`). Clipboard
reuses the `launchers` panel's OSC 52 + `wl-copy` path (extract a small shared helper if it
is currently private to `launchers.rs`). Added to `dashboard-suite/suite.toml` as a
`[[panel]]` and a `[[launcher]]` (tile binary from the glance repo), plus a ROADMAP note.

## Testing

Unit (colocated; buffer-scan render pattern):
- `job.rs`: tolerant parse of a sample state.json (incl. missing fields); newest-first sort;
  `is_live` truth table across the state/tempo vocab; `resume_command` (cd+escape, empty cwd,
  embedded single quote, includes `--dangerously-skip-permissions`); age humanize.
- `mod.rs`: `handle_key` returns `Drop`/`Copy` for `d`/`c`; `f` toggles `filter_live` and
  `visible()` shrinks; focus wraps; `Enter` toggles detail.
- render smoke: list renders names + a state glyph; detail modal renders the intent.

Then: `cargo test`, build, install `crew` + `glance`, live tmux smoke of the standalone
(`f` filters, `c` copies the right command, `d` prints `… --dangerously-skip-permissions`),
and `crew --version/--help` (non-TTY). Panel smoke: `c` copies; `d` opens a new tmux window
running the resume (verify a new window appears).

## File-by-file change list

New:
- `src/crew/mod.rs`, `src/crew/job.rs`
- `src/panels/crew.rs`
- `src/bin/crew.rs`
- `docs/superpowers/specs/2026-05-23-crew-design.md` (this file)

Edited:
- `src/lib.rs` — add `pub mod crew;`
- `src/panels/mod.rs` — add `pub mod crew;`, register in `DEFAULT_ORDER`/`ALL_PANELS`/`build_panel`
- `src/panels/launchers.rs` — extract/share the OSC 52 + wl-copy helper (if needed)
- `dashboard-suite/suite.toml` + `ROADMAP.md` (separate repo)

## Open judgment calls (flag for review)

- Panel `d` spawns a new tmux window (vs copy-only). Chose spawn — it is the natural
  "drop in" from a dashboard tile and matches the suite's panel-action convention; falls
  back to copy when not in tmux.
- Both `d` and `c` use the same `--dangerously-skip-permissions` command. (If `c` should copy
  a non-dangerous variant, say so.)
- `/` free-text search deferred to a follow-up.
