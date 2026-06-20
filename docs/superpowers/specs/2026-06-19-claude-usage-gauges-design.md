# Claude Usage Gauges Design

**Date:** 2026-06-19
**Status:** Approved, ready for implementation plan

## Goal

Glance at how much of your Claude Max limits you have burned (session 5-hour window, weekly window, and per-model weekly windows) without running the `/usage` command. Ships as a glance panel and a standalone `usage` binary that share one renderer.

## Scope decisions (locked during brainstorming)

- **Data:** live limit gauges (the same data the Claude Code `/usage` view shows), not historical token/cost accounting.
- **Accounts:** the currently logged-in account only, read straight from `~/.claude/.credentials.json`. Multi-account ("view both") is explicitly deferred. The data model leaves a clean seam to add more sources later, but no multi-account machinery is built now.
- **Form factor:** both a glance panel (added to the n/p rotation) and a standalone `usage` binary, via the shared-lib-panel idiom (same pattern as `music`).
- **Token freshness:** approach A, read-only with graceful-stale. Read the access token and `expiresAt` from `.credentials.json`. If valid, fetch live. If expired or the fetch fails, show the last-known gauges dimmed with a stale note. No OAuth refresh logic, no write-back to the credentials file.

## Data source (verified)

The Claude Code binary calls `GET https://api.anthropic.com/api/oauth/usage` with an `Authorization: Bearer <oauth-access-token>` header. The response carries one entry per limit window. Window kinds observed in the client: `five_hour` ("session limit"), `seven_day` ("weekly limit"), `seven_day_opus` ("Opus limit"), `seven_day_sonnet` ("Sonnet limit"), and `overage`/`extra_usage` ("usage credit limit"). Each window exposes a numeric `utilization` (percent) and a `resets_at` timestamp.

The access token lives in `~/.claude/.credentials.json` under `claudeAiOauth.accessToken`, alongside `claudeAiOauth.expiresAt` (epoch milliseconds), `claudeAiOauth.subscriptionType` (e.g. `max`), and `claudeAiOauth.rateLimitTier` (e.g. `default_claude_max_20x`).

The exact request headers beyond the bearer token (a possible `anthropic-version` or `anthropic-beta`) and the precise JSON field nesting are confirmed during the first implementation task by a real `curl` smoke test against the live token. That captured response becomes the unit-test fixture.

## Architecture

Three units, following existing glance idioms (no new third-party crate; `curl` on a background thread, `serde_json` already a dependency).

### 1. `src/usage.rs` (lib data module)

Pure logic with the network call isolated at the edge.

- `read_credentials() -> Result<Creds, CredsError>`: read and parse `~/.claude/.credentials.json`. Extract `accessToken`, `expiresAt`, `subscriptionType`, `rateLimitTier`. `CredsError` distinguishes "file missing/unreadable" from "token expired" (`expiresAt < now`).
- `fetch(token: &str) -> Result<UsageSnapshot, String>`: `curl` the usage endpoint with the bearer header, return raw bytes to `parse_usage`.
- `parse_usage(bytes: &[u8]) -> Result<UsageSnapshot, String>`: pure, unit-tested against the fixture.
- `parse_credentials(text: &str) -> Result<Creds, CredsError>`: pure, unit-tested.

Types:

```
struct Creds { access_token: String, expires_at_ms: i64, subscription: String, tier: String }

enum CredsError { Missing, Expired, Malformed(String) }

struct UsageSnapshot { windows: Vec<Window> }

struct Window { kind: WindowKind, utilization: f64, resets_at: Option<i64> }

enum WindowKind { FiveHour, SevenDay, SevenDayOpus, SevenDaySonnet, Overage }
```

`WindowKind` has a `label()` returning the display string (`session`, `weekly`, `opus`, `sonnet`, `overage`) and parses from the API's snake_case kind string. Unknown kinds are skipped, not errored, so a new server-side window kind does not break the panel.

### 2. `src/panels/usage.rs` (`UsagePanel`)

Implements the `Panel` trait.

- Owns a background fetch thread plus an `mpsc` channel, mirroring `prs.rs` / `weather.rs`.
- `refresh_ms()` returns a short value (500) so the focused panel drains the channel promptly (per the suite async-panel rule: only the focused panel ticks, so an async panel needs a short refresh to pick up its result).
- The actual network fetch is throttled internally to once per 60 seconds (track last-fetch `Instant`) to stay clear of endpoint rate-limiting. `tick()` spawns a fetch only when 60s have elapsed and no fetch is in flight.
- State held: `last_snapshot: Option<UsageSnapshot>`, `last_fetch: Option<Instant>`, `status: PanelStatus` (Loading / Ok / Stale(reason) / NoCreds), plus `subscription`/`tier` strings for the header.
- `render()` draws the gauge rows (see Render).
- No key handling beyond the default (returns false).

### 3. `src/bin/usage.rs` (standalone binary)

A copy of the `src/bin/music.rs` shell: install panic hook, raw mode, alternate screen, event loop. Keys: `q` quit, `Ctrl-C` quit, `[` / `]` brightness via `glance::brightness`, all else delegated to the panel (which ignores them). Supports `--help` and `--version`. Full-screen render of `UsagePanel` with a one-line footer. cargo auto-discovers `src/bin/*.rs`, so no `Cargo.toml` change is required.

### 4. Registration (4 edits in `src/panels/mod.rs`)

- `pub mod usage;`
- `build_panel` arm: `"usage" => Box::new(usage::UsagePanel::new()),`
- add `"usage"` to `DEFAULT_ORDER`
- add `"usage"` to `ALL_PANELS`

## Render

Both the panel and the standalone call the same draw routine.

```
 Claude usage · max · 20x

 session  ████████░░░░░░░░░░░░  41%   resets 3h 12m
 weekly   ██████░░░░░░░░░░░░░░  33%   resets 4d 6h
  opus    ███░░░░░░░░░░░░░░░░░  18%   resets 4d 6h
  sonnet  █░░░░░░░░░░░░░░░░░░░   7%   resets 4d 6h
```

- One row per window the API returns, in a fixed order (session, weekly, opus, sonnet, overage). Rows for windows not present are omitted (for example, per-model rows appear only when the response includes them).
- Bar fill is `utilization / 100 * bar_width`, drawn with `█` (filled) and `░` (empty). Bar width derives from the panel width minus the label and percent columns.
- Bar color by utilization: sage below 50 percent, amber 50 to 80 percent, magenta above 80 percent.
- Reset countdown is `resets_at` minus now, formatted coarsely: `3h 12m`, `4d 6h`, `12m`. If `resets_at` is absent, omit the countdown.
- Header line shows `Claude usage`, the subscription type, and the tier (e.g. `max · 20x`).

## States and error handling

- **First load:** `loading…` until the first fetch returns.
- **No credentials file:** `no claude credentials found`.
- **Token expired** (`expiresAt < now`): do not fetch. Show the last-known snapshot dimmed (or the no-data message if none) with footer `stale · open claude to refresh`.
- **Fetch or HTTP failure, including rate-limited:** keep and dim the last snapshot, footer shows the reason. Partial responses render whatever windows are present.
- The access token is read-only. It is never logged and never written back. The credentials file is never modified.

## Testing

Unit tests cover the pure functions against a captured real fixture:

- `parse_usage`: all window kinds present; per-model windows absent; an unknown window kind is skipped without error; malformed JSON returns an error.
- `parse_credentials`: well-formed creds; missing file maps to `CredsError::Missing`; `expires_at_ms` in the past maps to `CredsError::Expired`; malformed JSON maps to `CredsError::Malformed`.
- Reset-countdown formatting: minutes only, hours and minutes, days and hours, absent timestamp.
- Utilization-to-color thresholds at the boundaries (49/50/80/81).
- Bar-fill width math at 0, partial, and 100 percent for a representative width.

The `curl`/network call itself stays untested, consistent with `weather` and `prs`.

## Out of scope (future seams, not built now)

- Multi-account view. The `Creds` source is a single function today; a later version can return a list of labelled sources and render one gauge block per account.
- Historical token/cost accounting from the session JSONL logs (a separate idea, different data source).
- OAuth refresh / write-back (approaches B and C from brainstorming).
