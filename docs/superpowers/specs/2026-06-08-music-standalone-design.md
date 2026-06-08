# music (standalone) Design -- 2026-06-08

## Goal

A standalone `music` command: the glance `music` panel (now-playing + MPRIS
transport controls via playerctl) running full-screen as its own binary in the
glance crate. The point of a standalone is that it owns the whole keymap, so it
can use the arrow keys for previous / next track. In glance those keys are
reserved for panel navigation, which forced the panel to use `<` / `>`.

Works with any MPRIS player (Spotify, Chromium, a phone via kdeconnect), not only
Spotify; `music` is the accurate name.

## Architecture

A new binary at `src/bin/music.rs` in the existing glance crate. Cargo
auto-discovers `src/bin/*.rs`, so no `[[bin]]` entry is needed (same as `cal`,
`crew`, `health`, `tasks`, `vitals`).

The binary owns one concrete `MusicPanel` (`glance::panels::music::MusicPanel`,
already `pub`) and reuses it directly. There is NO `*Core` extraction: the panel
is already the self-contained unit (it owns its playerctl polling, render, and
`handle_key`), so this is the same "reuse the panel in a bin" approach `vitals`
used, not the `*Core` lib+bin split that `health` / `cal` use.

The run loop mirrors `src/bin/health.rs`: install the suite panic hook
(`suite_term::panic::install_panic_hook()`), enable raw mode, enter the alternate
screen with `SetTitle("music")`, loop, restore the terminal on exit. Each frame
ticks the panel (polls playerctl), renders it, and polls keys; every non-quit key
is forwarded to `panel.handle_key`.

## Keymap

The standalone owns the full keymap. The win over the in-glance panel:
`Left` / `Right` work as previous / next track.

This is achieved by adding two arms to the panel's existing
`key_playerctl_args(code)` map (and its label match in `handle_key`):

- `KeyCode::Left  => previous`
- `KeyCode::Right => next`

These are DORMANT in glance: glance's global key handler consumes `Left` / `Right`
for panel navigation before the focused panel's `handle_key` is ever called, so
the panel never receives them there. They are LIVE in the standalone, where the
bin routes every non-quit key to `handle_key`. The existing `<` / `>` bindings
stay, so both work.

Full standalone keymap:

- `Left` / `Right` (and `<` / `>`): previous / next track
- `Space`: play / pause
- `Up` / `Down`: volume +5% / -5%
- `,` / `.`: seek -5s / +5s
- `s`: shuffle toggle
- `L`: loop cycle (None -> Track -> Playlist)
- `d`: cycle target device / player
- `[` / `]`: dim / brighten (via `glance::brightness`)
- `q` / Ctrl-C: quit

## Rendering

The `MusicPanel` renders into the screen area above a one-line footer. The panel
already draws: now-playing status, marquee-scrolled title, artist / album,
progress, `@device`, shuffle / loop indicators, and a transient action toast. The
footer shows the standalone keymap (including the arrows) so the freed arrow keys
are discoverable.

Layout per frame: `[ Min(0) panel body, Length(1) footer ]`.

Data tick about once per second (playerctl polling); keys polled at 100ms so
input feels instant, independent of the data tick. (`health.rs` uses the same
1s tick / 100ms poll split.)

## Error handling

- No player running: the panel already renders its existing empty / no-player
  state. Key actions are harmless no-ops (playerctl returns nothing; the panel's
  toast reflects the attempt). The bin does not crash or special-case this.
- Terminal too small: ratatui clips what does not fit without panicking.
- playerctl missing entirely: `playerctl()` returns `None` for every call, so the
  panel shows its empty state; the bin still runs and quits cleanly.

## Testing

The panel's existing unit tests (marquee width, player ring-walk, loop cycle,
friendly player labels) continue to cover the reused logic. One new deterministic
test is added next to them:

- `key_playerctl_args(KeyCode::Left)` returns the `previous` args and
  `key_playerctl_args(KeyCode::Right)` returns the `next` args.

The bin's event loop and rendering are verified by build + a pty/tmux smoke test
(launch, observe now-playing render, send `q`, confirm clean exit), consistent
with how the other bins are handled.

## Install

Add a `[[launcher]]` entry to `~/projects/dashboard-suite/suite.toml`:

    [[launcher]]
    name = "music"
    summary = "now-playing + MPRIS controls (standalone)"
    repo = "glance"
    url = "https://github.com/JaneAdora/glance"
    package = "glance"
    artifact = "music"
    bin = "music"
    requires = ["playerctl"]
    default = false

The binary installs to `~/.local/bin/music`. The existing `[[panel]]` named
`music` is a separate manifest list (panels vs launchers), so there is no name
collision; the glance `music` panel is unchanged. This install/manifest step
lives in the dashboard-suite repo, done after the bin is built and verified.

## Out of scope (v1)

- Any full-screen-only feature the panel does not already have: no player picker
  list, no album-art view, no click / drag-to-seek. Just the panel, full-screen,
  with the arrow keys freed. Easy to add later.
- A `*Core` lib extraction (the panel is reused directly).
- Changing the glance `music` panel's behavior (the new arrow arms are dormant
  there).

## Reuse references

- Standalone-bin pattern: `src/bin/health.rs`, `src/bin/vitals.rs` (panic hook,
  raw mode, alt screen, 1s tick / 100ms poll, `q` / Ctrl-C / `[` / `]`,
  `--help` / `--version`).
- The panel: `src/panels/music.rs` (`MusicPanel`, `key_playerctl_args`,
  `handle_key`).
- rsuite manifest: `~/projects/dashboard-suite/suite.toml`.
