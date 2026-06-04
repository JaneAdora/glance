# `cal`: Google Calendar agenda tile (glance panel + standalone)

**Date:** 2026-05-26
**Status:** spec -> plan
**Sibling of:** `health` (2026-05-23), `crew` (2026-05-23), `tasks` (2026-05-25)
**Repo home:** `~/projects/glance` (fourth lib+bin sibling)
**Bridge:** `~/Projects/skai-work/scripts/zele/cal_json.py` (REST-API shim shipped 2026-05-26, commit `259797b` on `feat/gdocs-native-md-export`)

---

## Goal

A glance panel + standalone TUI that shows today's calendar plus the next 7 days, grouped by day, with one-keystroke copy-URL-of-next-meeting so Jane can jump into her browser and join. Cal is a **pure tile**: no `RunOutcome::PrintAndExit`. The destination of a meeting URL is the browser, not the terminal, so the eval-style exit pattern from `tasks` and `crew` does not apply.

## Background

`zele` already wraps Gmail / Slack / Monday / Calendar, but its calendar code uses CalDAV (legacy) and has both a Google-Cloud-project-API-enablement gate AND a date-filter bug that returns un-expanded recurring-event masters from 2024 instead of expanded 2026 instances. The auth side was fixed today (CalDAV API enabled in GCP project `471304390066`, propagation 90 s, stale CalendarList cache invalidated; `zele cal list` now returns 42 calendars cleanly). The date-filter bug is a deeper change in `zele/src/calendar-client.ts` and is left as a separate zele PR.

For `cal`, the path forward is the **Calendar v3 REST API directly via a Python shim** (`cal_json.py`). The shim:
- reads OAuth credentials from 1Password (same Work-vault items zele uses),
- refreshes the access token if expired (POST to `oauth2.googleapis.com/token` with the stored refresh_token; writes the new credential back to 1P),
- calls `calendars/<id>/events?singleEvents=true&orderBy=startTime&timeMin&timeMax`,
- emits a clean snake_case JSON envelope that Rust can consume with `serde_json::from_slice`.

Verified 2026-05-26: `--today` returns 3 real events (Daily Huddle, LI Retargeting, placeholder); `--week` returns 15 events with all recurring instances correctly expanded into 2026 dates, Meet URLs and attendees included.

This widget is the fourth in the suite's lib+bin idiom (after `health`, `crew`, `tasks`) but the **first pure-tile** of the four. `space` copies a URL via OSC 52 and stays in the TUI.

## Event data model

```rust
// src/cal/event.rs
pub struct Event {
    pub id: String,
    pub summary: String,
    pub description: String,        // raw HTML; rendered through desc::strip_html
    pub location: String,
    pub start: Zoned,               // jiff Zoned in local tz
    pub end: Zoned,
    pub all_day: bool,
    pub status: String,             // "confirmed" | "tentative" | "cancelled"
    pub html_link: String,          // Google Calendar event page
    pub hangout_link: String,       // canonical Meet URL when present
    pub meet_url: String,           // hangout_link or first conferenceData video entry
    pub attendees: Vec<Attendee>,
    pub is_recurring: bool,
    pub recurring_event_id: String,
    pub calendar_id: String,
}

pub struct Attendee {
    pub email: String,
    pub name: String,
    pub response_status: ResponseStatus,   // accepted | declined | tentative | needs_action
    pub is_self: bool,
    pub organizer: bool,
}

pub enum ResponseStatus { Accepted, Declined, Tentative, NeedsAction }
```

Helpers:
- `Event::is_past(&self, now: &Zoned) -> bool` -> compares against `self.end`.
- `Event::is_declined(&self) -> bool` -> any self-attendee with `Declined`.
- `Event::duration(&self) -> jiff::Span` -> `end - start`.
- `Event::time_until(&self, now: &Zoned) -> Option<jiff::Span>` -> `None` if past, else `start - now`.

## Architecture

Single core wrapped by two surfaces, same shape as the three siblings. Bridge is shell-out to the Python shim.

```
~/projects/glance/src/
├── lib.rs                 # + pub mod cal
├── cal/
│   ├── mod.rs             # CalCore (state, focus, expanded, action enum, drill)
│   ├── event.rs           # Event, Attendee, ResponseStatus, helpers
│   ├── bridge.rs          # spawn cal_json.py, parse JSON, cache to ~/.cache/glance/cal.json
│   ├── desc.rs            # HTML strip + entity decode + URL extraction
│   └── view.rs            # DayGroup, day-bucketing, NOW-marker placement, time fmt
├── panels/cal.rs          # CalPanel: glance Panel impl
└── bin/cal.rs             # standalone TUI; q to quit; NO PrintAndExit
```

### `CalCore`

```rust
pub struct CalCore {
    days: Vec<DayGroup>,
    focus: Focus,                          // (day_idx, Option<event_idx>)
    expanded: HashSet<jiff::civil::Date>,  // today auto-expanded on new()
    show_detail: bool,
    show_past: bool,                       // default: true (past dimmed-with-checkmark)
    last_fetched: Option<Instant>,
    last_toast: Option<(String, Instant)>,
    fetch_in_flight: bool,                 // gate against concurrent ticks
    rx: Option<mpsc::Receiver<bridge::FetchResult>>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Focus {
    pub day: usize,
    pub event: Option<usize>,
}

pub enum CalAction {
    None,
    CopiedUrl,
    CopiedDetail,
    Toast(String),
    Quit,
}

impl CalCore {
    pub fn new() -> Self;                 // serves cache instantly; kicks bg fetch
    pub fn tick(&mut self);               // poll receiver; refresh on 5-min cadence
    pub fn render(&self, f: &mut Frame, area: Rect);
    pub fn handle_key(&mut self, key: KeyEvent) -> CalAction;
    // navigation
    pub fn move_down(&mut self);
    pub fn move_up(&mut self);
    pub fn toggle_expand(&mut self);
    pub fn drill_in(&mut self);           // header -> expand+first event; event -> detail
    pub fn drill_out(&mut self);          // detail -> close; event -> collapse+header
    // actions
    pub fn copy_url(&mut self) -> CalAction;
    pub fn detail_text(&self) -> Option<String>;
    pub fn toggle_past(&mut self);
    pub fn refresh(&mut self);
}
```

Focus survives reload by storing `(date, event_id)` and re-resolving to indices each render. Same pattern as TasksCore.

## Bridge layer (`bridge.rs`)

```rust
pub struct FetchResult {
    pub events: Vec<Event>,
    pub fetched_at: SystemTime,
    pub stale_cache: bool,                // true if served from cache after a fetch error
}

pub fn fetch_async() -> mpsc::Receiver<FetchResult>;
pub fn fetch_sync() -> Result<FetchResult, BridgeError>;
pub fn load_cache() -> Option<FetchResult>;
pub fn write_cache(events: &[Event]) -> Result<(), std::io::Error>;
```

### Shell-out

```rust
let shim = dirs::home_dir().unwrap().join("Projects/skai-work/scripts/zele/cal_json.py");
let out = Command::new("python3")
    .arg(&shim)
    .arg("--week")
    .arg("--account").arg("jane@repcap.com")
    .output()?;
```

Configurable via env: `GLANCE_CAL_SHIM` overrides the default path, `GLANCE_CAL_ACCOUNT` overrides the account. Tests use both.

### Cache

- Path: `~/.cache/glance/cal.json` (XDG cache dir; via `dirs::cache_dir()`).
- Format: same JSON shape the shim emits, plus a top-level `_fetched_at` ISO timestamp inserted by `bridge::write_cache`.
- TTL: 5 minutes. `bridge::load_cache` returns `Some` only if `_fetched_at + 5min > now`. Cold launches with no cache return `None` and render `"loading..."`.
- Stale-on-error: if the bg fetch fails, fall back to the cache **regardless of age** with `stale_cache = true` and a toast `"refresh failed; cached Xm ago"`.

### Refresh flow

```text
new()
  └─ try load_cache()
       ├─ Some(fresh) -> seed CalCore; skip first fetch
       └─ None        -> render "loading..."; spawn first fetch

tick() (every 100ms in UI loop)
  ├─ poll rx for pending FetchResult
  ├─ if last_fetched > 5min ago AND !fetch_in_flight -> spawn fetch
  └─ housekeep: expire toasts (3s), prune pending_anything

`r` (manual refresh) -> spawn fetch immediately, bypassing 5-min gate.
```

Background fetch via `std::thread::spawn`; result lands on `mpsc::Receiver`; UI thread reads non-blocking on each tick. UI never blocks on the subprocess.

## Description rendering (`desc.rs`)

```rust
pub fn strip_html(raw: &str) -> String;     // -> plain text, paragraphs preserved
pub fn extract_urls(raw: &str) -> Vec<String>;   // dedupe; preserve first-seen order
```

### `strip_html`

Tiny state machine, no external HTML parser:
- Inside `<tag>`: skip.
- Outside `<tag>`: emit chars, decoding entities (`&amp; &lt; &gt; &nbsp; &quot; &#39; &#NN; &#xHH;`).
- Block-level tags (`</p>`, `</li>`, `</div>`, `<br>`) emit `\n`.
- Multiple `\n` collapsed to one.

Tradeoff: loses nested-list indentation. Acceptable; Google's event descriptions are flat in practice.

### `extract_urls`

Regex `https?://[^\s<>"]+` applied to the raw HTML (catches `href="..."` URLs without needing to parse anchors). Trim trailing punctuation (`.,;:`). Dedupe preserving first-seen order.

## Filtering, sorting, visual rules

### Filter (which events render)

- **Cancelled events**: hidden.
- **Declined events**: NOT hidden (Jane's call: she wants to see what she said no to in case her plans shift). Rendered with `Modifier::CROSSED_OUT`.
- **Multi-day events**: render only on the first day they span. Note this in `?` help.
- **`show_past` toggle (`p` key)**: default `true` (past events shown dimmed with `✓`). When false, today's past events are hidden but the day-header count includes them (e.g. "TODAY · 3 events · 1 upcoming").

### Sort (within a day)

1. All-day events first.
2. Then by `start` ascending.

### Style matrix

| Time | Response | Style |
| --- | --- | --- |
| Future | accepted / pending / tentative / no-response | normal color, no marker |
| Future | declined | strikethrough, normal color, no marker |
| Past | accepted / pending / tentative / no-response | dim + `✓` |
| Past | declined | dim + `✓` + strikethrough ("missed by choice") |

Composed via `Style::default().add_modifier(Modifier::DIM | Modifier::CROSSED_OUT)`.

### NOW marker

A thin horizontal separator line `─── NOW ───` rendered between the last past event and the first upcoming event **on today's day-group only**. Rendered in `theme::magenta()` (alert-pink) to draw the eye. Hidden when:
- It's a day other than today.
- Today has no past events (separator would be at the top, awkward).
- Today has no upcoming events (separator would be at the bottom, useless).

## Layout

### Wide (>= 80 cols)

```
┌─ cal · Tue May 26 ───────────────────────────────────────────────┐
│                                                                  │
│ ▾ TODAY · Tue May 26 · 3 events                                  │
│   ✓ 09:00–09:30  Daily Huddle                          📹        │
│   ─── NOW ───────────────────────────────────────────────────    │
│ ▸ 17:00–17:30  LI Retargeting and Video Ads            📹        │
│   23:47–00:17  (no title)                                        │
│                                                                  │
│ ▾ WED May 27 · 3 events                                          │
│   09:00–09:30  Daily Huddle                            📹        │
│   12:00–13:00  Design/Dev Weekly Checkin               📹        │
│   13:00–14:00  X-Team Internal Huddle                  📹        │
│                                                                  │
│ ▸ THU May 28 · 1 event                                           │
│ ▸ FRI May 29 · 2 events                                          │
│ ▸ SAT May 30 · 1 event                                           │
│ ▸ SUN May 31 · 1 event                                           │
│                                                                  │
└─ space copy URL · c copy detail · y url · r refresh · ? help · q ┘
```

- Day-header glyph: `▾` expanded / `▸` collapsed.
- Today's header always renders first; future days follow chronologically.
- Focused day header: pink-bold (`theme::pane_header_focused()`). Unfocused: dim lavender.
- Focused event row: `theme::active_row()` (pink with focus marker `▸`).
- Event row format: `[glyph] HH:MM–HH:MM  Summary  📹` where `glyph` is `✓` for past, blank otherwise. `📹` is the conferencing glyph (present iff `meet_url` non-empty); shown only at width >= 60.

### Detail modal (Enter / `l` on a focused event)

```
┌─ Daily Huddle · Tue May 26 ──────────────────────────┐
│ 09:00–09:30 · 30 min · in 18 min                     │
│ 📹 https://meet.google.com/abc-defg-hij                │
│                                                       │
│ Attendees (10)                                        │
│   ★ ✓ Avery Quinn             <avery@example.com>     │
│     ✓ Jane Mitchell (you)     <jane@repcap.com>       │
│     ✓ Jordan Lee              <jordan@example.com>    │
│     ? Sam Carter              <sam@example.com>       │
│     ✗ Pat Morgan              <pat@example.com>       │
│   …5 more                                              │
│                                                       │
│ Description                                            │
│   We'll go in alphabetical order. You have two        │
│   options for your checkin here: On Track / Off…      │
│                                                       │
│ Links                                                  │
│   • https://otter.ai/mt/example-transcript-id         │
│   • https://www.google.com/calendar/event?eid=…       │
│                                                       │
│ Esc / Enter / q / h / Left to close                   │
└───────────────────────────────────────────────────────┘
```

- Time line: `start–end · duration · time-until` (or `· just ended` / `· N min ago` if past).
- Attendee glyphs: `★` organizer · `✓` accepted · `✗` declined · `?` pending or no-response · `~` tentative.
- `(you)` after Jane's own name (from `is_self`).
- Attendees truncated to **8** with `…N more`.
- Links section combines `extract_urls(description)` plus `html_link`, deduped.
- Description text from `desc::strip_html`. Wraps at modal width.
- `c` while modal open: copy whole detail to clipboard (same text as `c` in normal mode). Matches `tasks`.

### Mobile / narrow widths

| Width | Behavior |
| --- | --- |
| ≥ 80 | Full layout above |
| 60–79 | Time collapses to start only: `09:00 Daily Huddle 📹` |
| 40–59 | Drop `📹` glyph (detail modal still shows it). Day-header counts drop. |
| < 40 | Single column: glyph + start-time + truncated summary. Detail modal full-screen, paginated by `j`/`k`. |

`WidthClass::from(cols)` shared helper, same shape as `tasks::view::WidthClass`.

## Keybindings

### Standalone footer (always visible)

```
space copy URL · c copy detail · y url · p past · r refresh · ? help · q quit
```

### Glance panel footer (rendered by glance when this panel is focused)

```
space copy · o expand · j/k move
```

`n` / `Tab` / `→` are glance global panel-switch keys; the panel does NOT bind them. `wants_keys = false` so digit-shortcuts continue to work.

### Full bindings (in `?` help, standalone)

```
NAVIGATION
  j / ↓        next visible row
  k / ↑        prev visible row
  Tab / o      toggle expand on focused day header
  l / →        drill in (header -> expand + first event; event -> detail)
  h / ←        drill out (detail -> close; event -> collapse + header)
  g            top
  G            bottom

ACTIONS
  space        copy Meet URL of focused event (OSC 52)
  y            copy URL (alias for space; finger-memory with tasks/crew)
  c            copy event detail as plain text (paste into a Claude prompt)
  Enter        open detail modal (alias for l on an event)

VIEW
  p            toggle show past (default: shown dimmed with ✓)
  r            force refresh (bypass 5-min cache)

EXIT
  q            quit
  Esc          cancel mode / close modal

NOTES
  Cancelled events are hidden.
  Declined events are shown with strikethrough.
  Multi-day events render only on the first day they span.
```

## Refresh + error handling

| Failure | Behavior |
| --- | --- |
| `cal_json.py` missing | Toast `"shim missing at <path>"`; render last-good cache if any, else `"no cal data"` |
| `cal_json.py` exit non-zero | Toast `"bridge error: <stderr first line>"`; render last-good cache (any age) with stale toast |
| OAuth refresh failure (network, 1P down) | Same as bridge error (the shim writes the error to stderr) |
| Empty week (no events at all) | Render `"no events this week"` placeholder |
| JSON parse error | Toast `"bridge JSON error"`; render last-good cache |
| Cache write fails | Continue without caching; toast `"cache write failed"` once per process |
| `~/.cache/glance/` missing | `bridge::write_cache` creates it (`fs::create_dir_all`) |

## Testing

Inline `#[cfg(test)] mod tests` per module, matching siblings.

### `cal/event.rs` tests
- Parse a fixture matching the shim's emitted JSON; assert all fields populated.
- Detect declined: when `attendees[].is_self && response_status == "declined"`.
- `is_past`: end < now -> true; end > now -> false.
- Duration math: 09:00 / 09:30 -> 30 min span.

### `cal/desc.rs` tests
- Strip HTML: input `"<p>Hello <b>world</b></p><p>Line 2</p>"` -> `"Hello world\nLine 2"`.
- Entity decode: `"&amp;&lt;&gt;&nbsp;&quot;&#39;"` -> `"&<> \"'"`.
- Extract URLs: pull `https://otter.ai/...` and `https://meet.google.com/...` from the real Daily Huddle description fixture.
- Dedupe URLs preserving order.

### `cal/view.rs` tests
- Day-bucket: 5 events across Tue/Wed/Fri grouped into 3 DayGroups with correct dates.
- All-day sort: all-day event lands at index 0 of its day even when added last.
- NOW marker placement: only on today; only when both past and upcoming events exist.

### `cal/mod.rs` (CalCore) tests
- Focus survives reload by `(date, event_id)`.
- Drill cycle: header -> task -> detail -> back walks correctly.
- Toggle past clears focus on a now-hidden event (re-anchors to next visible).
- `copy_url` returns `CopiedUrl` when meet_url present; `None` action when empty.
- `detail_text` shape: starts with `Event #<id> from <calendar>`, includes time, attendees, description, links.

### `cal/bridge.rs` tests
- Cache round-trip via a temp dir override.
- Stale-on-error: simulate a fetch failure; assert load_cache returns cached + stale flag set.
- Mock subprocess via `GLANCE_CAL_SHIM` env pointing at a fixture-emitting script (echoes a fixed JSON blob).

### Integration smoke (manual after install)
- `cal` against real shim: today renders with 3 events, NOW marker between past and upcoming.
- `space` on the Daily Huddle row copies `https://meet.google.com/abc-defg-hij` (paste into terminal to verify).
- `Enter` opens detail modal; Otter link visible in Links footer.
- A declined event renders with strikethrough.
- Mobile widths via `COLUMNS=32 cal`, `COLUMNS=58 cal`, `COLUMNS=120 cal`.
- `r` triggers a fresh fetch; toast confirms refresh.

## Suite registration

`~/projects/dashboard-suite/suite.toml`:

```toml
[[launcher]]
name = "cal"
summary = "Google Calendar agenda (week view)"
repo = "glance"
package = "glance"
artifact = "cal"
bin = "cal"
requires = ["python3", "op"]
default = false

[[panel]]
name = "cal"
summary = "today + upcoming calendar events"
default = true
```

`~/projects/glance/src/panels/mod.rs`:
- `pub mod cal;`
- Add `"cal"` to `DEFAULT_ORDER` (before `tasks`) and `ALL_PANELS`.
- Add `"cal" => Box::new(cal::CalPanel::new()),` to `build_panel`.

`~/projects/dashboard-suite/ROADMAP.md`: add `cal` to the shipped panels list and a shipped entry below `tasks`.

## File plan

| File | Action | Purpose |
|---|---|---|
| `src/lib.rs` | Modify | `pub mod cal;` |
| `src/cal/mod.rs` | Create | `CalCore`, `CalAction`, `Focus`, drill methods |
| `src/cal/event.rs` | Create | `Event`, `Attendee`, `ResponseStatus`, helpers |
| `src/cal/bridge.rs` | Create | `fetch_async`, `fetch_sync`, cache read/write |
| `src/cal/desc.rs` | Create | `strip_html`, `extract_urls`, tiny state machine |
| `src/cal/view.rs` | Create | `DayGroup`, day-bucketing, WidthClass, NOW marker |
| `src/panels/cal.rs` | Create | `CalPanel: Panel` |
| `src/panels/mod.rs` | Modify | register module, DEFAULT_ORDER, ALL_PANELS, build_panel |
| `src/bin/cal.rs` | Create | standalone TUI; q to quit; no PrintAndExit |
| `~/projects/dashboard-suite/suite.toml` | Modify | register launcher + panel |
| `~/projects/dashboard-suite/ROADMAP.md` | Modify | shipped entry after merge |

**No new Rust deps.** `jiff` for time, `serde`/`serde_json` for parsing, `dirs` for cache path; all present. `mpsc` is stdlib.

## Out of scope (v1)

- Multi-calendar merge (just `primary`; the 42 other calendars Jane sees are mostly old-colleague mailboxes and group calendars whose relevant events already show up on primary because she's an attendee).
- Multi-day event rendering (render once on first day; full multi-day span is rare and complicates layout).
- Event creation / RSVP changes / decline-here (zele `cal create` / `cal respond` exist but this is a read tile).
- Event search across the calendar (use Google Calendar's own search).
- Multiple OAuth accounts merged into one view (the shim supports `--account` but cal v1 binds one).
- Notification / "meeting starts in 5 min" alerts (would need a daemon; out of pure-tile scope).
- Otter transcript fetch (Links footer surfaces the URL; clicking it in the browser is the workflow).
- Worklist merger (the renamed Monday+local-todo tile stays separate; revisit after using both for a week).

## Open questions

None. All forks resolved during brainstorming:
1. View scope: today + this week, day-grouped, today auto-expanded.
2. Join action: `space` copies URL (no exit-with-command; browser destination).
3. Past events: shown dimmed with `✓` marker; `p` toggles.
4. Declined events: shown with strikethrough (Jane's call: visible in case plans shift).
5. Detail modal: full detail + extracted-links footer.
6. Bridge: REST-API Python shim, not zele CalDAV.
7. Refresh: 5-min poll + cache + stale-on-error fallback; `r` forces fresh.

## Review log

- 2026-05-26, auth investigation: CalDAV API was disabled on the OAuth GCP project (`471304390066`); enabled via console, propagated in ~90 s, stale CalendarList cache invalidated. zele cal list now returns 42 calendars; cal events still buggy (RRULE expansion), so cal widget bypasses zele's calendar code via a REST shim.
- 2026-05-26, shim shipped: `cal_json.py` (214 lines, vanilla stdlib + `op` CLI) returning a clean snake_case JSON envelope. Verified end-to-end against real account.
- 2026-05-26, brainstormed forks and committed to this design.
- 2026-05-26, spec drafted; ready for review.

