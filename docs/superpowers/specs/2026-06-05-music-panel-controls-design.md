# Music Panel Controls (interactive MPRIS control)

**Goal:** Turn glance's read-only `music` panel into an interactive MPRIS
controller: full transport, volume, seek, shuffle, loop, and per-player (device)
targeting, all via `playerctl`. Panel-only (no standalone binary).

## Context and why

- The machine runs COSMIC on Wayland. The Aula F99's media/volume keys are
  grabbed by the compositor and routed globally (volume to the system mixer,
  media keys to the active MPRIS player). A TUI never receives those keysyms on
  Wayland, so hardware keys already cover global play/pause/next/prev/volume and
  this panel neither can nor should intercept them.
- The gap they leave: several MPRIS players are usually live at once (observed:
  `spotify`, `chromium`, and two `kdeconnect` phones). The hardware keys hit
  whatever the compositor considers "active" (often the phone), and `playerctld`
  is not running, so there is no deterministic way to target a specific device,
  nor to toggle shuffle/loop or seek per player.
- This panel fills exactly that gap: explicit device selection plus the controls
  the hardware keys do not expose, on top of the existing now-playing display.

## Architecture

Extend `MusicPanel` in `src/panels/music.rs`. No new files. Implement
`Panel::handle_key`; keep `wants_keys()` returning `false` so glance's panel
navigation still works (a non-capturing panel only receives the keys the global
handler does not claim).

### State additions on `MusicPanel`

- `players: Vec<String>` refreshed each tick from `playerctl -l`.
- `selected: Option<String>` the targeted player; `None` means "follow the
  active player" (bare `playerctl`). Pruned to `None` if the selected player
  disappears from `players`.
- `shuffle: bool` and `loop_status: String` ("None" / "Track" / "Playlist"),
  read each tick for display.
- `toast: Option<(String, Instant)>` transient feedback, ~2 seconds.

### Reserved keys (glance globals, must not be reused)

`q ? r [ ] n p Tab Esc`, `Ctrl-C`, the digits `0`-`9`, and the arrows `Left`
(previous panel) and `Right` (next panel). `Up`, `Down`, and `Space` are free.
Because Left/Right/Tab/n/p are all panel navigation, track next/prev use `<`/`>`.

### Keymap (`handle_key`)

| Key       | Action                                                        |
|-----------|---------------------------------------------------------------|
| `Space`   | play/pause (`playerctl ... play-pause`)                       |
| `>` / `<` | next / previous track                                         |
| Up / Down | volume up / down by 5% (`volume 0.05+` / `volume 0.05-`)      |
| `.` / `,` | seek forward / back 5s (`position 5+` / `position 5-`)        |
| `s`       | shuffle toggle (`shuffle toggle`)                             |
| `L`       | loop cycle: None -> Track -> Playlist (`loop <state>`)        |
| `d`       | cycle target device: auto -> spotify -> chromium -> phone ... |

Every control targets the selected player via `-p <name>`, or bare `playerctl`
when `selected` is `None`.

### Device cycle (`d`)

Pure helper `next_player(players: &[String], current: &Option<String>) ->
Option<String>` that walks the ring `[None, players...]` and wraps. A second
helper gives a friendly label for the header (e.g. a `kdeconnect.mpris_*` id ->
"phone", `chromium.instance*` -> "chromium", `spotify` -> "spotify").

### tick()

Read `playerctl -l` into `players`; prune `selected` if it is gone; then read
status / metadata / position / shuffle / loop for the target (selected, else
active). The existing graceful "nothing playing" path stays when no players.

### render()

Keep the existing now-playing lines, marquee title, and progress gauge. Add a
header showing the target device label and shuffle/loop glyphs, and show the
`toast` (when present) on the key-hint line.

## Error handling

`playerctl()` already returns `Option` (None on non-zero exit or spawn failure).
A failed control action sets a toast (e.g. "no player") and is a no-op; nothing
panics. Parse failures fall back to defaults, as today.

## Testing

Pure, unit-testable helpers (the shell-out itself stays a thin wrapper,
smoke-tested via tmux capture-pane per suite convention):

- `next_player` device-cycle: list + current -> next, including wrap and a
  selected player that is no longer present.
- loop-state cycle: None -> Track -> Playlist -> None.
- key -> `playerctl` args mapping (volume/seek/transport).
- friendly player label: kdeconnect id -> "phone", chromium -> "chromium",
  spotify -> "spotify", unknown -> the raw name.

## Out of scope

- Spotify Web API (playlists, catalog search, Connect device control).
- A standalone `music` binary or `MusicCore` refactor (panel-only by choice;
  could be a later dual-form follow-up).
- Running or managing `playerctld`. Starting it would make the hardware keys and
  bare `playerctl` follow a stable "active player"; that is a separate
  desktop-level option, noted here as a possible follow-up, not part of this build.
