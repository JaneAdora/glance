# glance

Tile-mode multi-panel dashboard widget. Part of the wt / recall / roam / glance suite.

One binary, many visualizations. Press `1`-`9` to jump to a panel, `n`/`p` to cycle, `q` to quit. Each panel runs at its own refresh rate; only the focused panel ticks, so a large registry costs nothing while idle. Background threads handle slow data sources (ping, git scans, calendar) and drain over `mpsc` channels without blocking the UI.

## Install

```
cd ~/projects/glance
cargo build --release
install -m 0755 target/release/glance ~/.local/bin/glance
```

Or via the suite installer: `rsuite` (see the `dashboard-suite` repo).

## Usage

```
glance                 # launch dashboard (interactive TTY required)
glance --write-config  # write a starter ~/.config/glance/panels.toml and exit
glance --help          # CLI help
```

## Panels

37 built-in panels, registered in `src/panels/mod.rs`. By default 35 are enabled (`fans` and `battery` are built but excluded from `DEFAULT_ORDER`, since the dev box has neither; enable them via `panels.toml`).

- **System / hardware:** `cpu` `mem` `net` `disk` `loadavg` `entropy` `fans` `io` `gpu` `battery`
- **Network:** `ping` `world-ping` `traceroute` `conn` `tsmap`
- **Work / dev:** `commits` `prs` `issues` `health` `cal` `crew` `tasks` `standup`
- **Environment:** `temp` `weather` `alerts` `hurricane` `solar`
- **Time / decoration:** `clock` `moon` `timer` `music` `pet` `mascot` `starfield` `mandala` `launchers`

Full per-panel reference (purpose, data source, refresh, source file) is in the suite manual: https://dashboard-suite-constellation.netlify.app/#repos/glance/panels

## Configuration

Panel selection and order live in `~/.config/glance/panels.toml` (a `panels = [...]` array). The order is the hotkey-slot order. With no config file, glance falls back to `default_registry()`. Run `glance --write-config` to scaffold one from the current defaults.

Common env vars (panel-specific):

| Env var | Effect | Default |
|---|---|---|
| `GLANCE_PING_HOSTS` | Comma-separated hosts for the `ping` panel | `1.1.1.1,8.8.8.8,github.com` |
| `WT_ROOTS` | Colon-separated repo roots for the `commits` heatmap | `~/projects:~/Projects` |
| `GLANCE_LAT` / `GLANCE_LON` / `GLANCE_LOCATION` | `weather` / `solar` location | Baton Rouge |
| `GLANCE_CAL_SHIM` / `GLANCE_CAL_CACHE` | `cal` panel Python bridge + cache override | shim under skai-work; cache under `~/.cache/glance` |

## Keys

```
1-9 / 0    jump to panel by slot
n / Tab    next panel          p   previous panel
r          force-refresh current panel
[ / ]      dim / brighten (brightness 30-150)
?          help
q / Esc    quit (Ctrl-C also works)
```

Panels can define their own keys via `Panel::handle_key`; when a panel returns `true` from `wants_keys()`, all keys route to it (used by `health`'s inline log-entry mode and `tasks`).

## Architecture

`Panel` trait (`src/panels/mod.rs`):

```rust
pub trait Panel {
    fn name(&self) -> &str;
    fn tick(&mut self);
    fn render(&self, f: &mut Frame, area: Rect);
    fn refresh_ms(&self) -> u64 { 500 }
    fn handle_key(&mut self, _key: KeyEvent) -> bool { false }
    fn wants_keys(&self) -> bool { false }
}
```

The app holds `Vec<Box<dyn Panel>>` and ticks only the focused panel, gated by its `refresh_ms` deadline. Adding a panel = one new file in `src/panels/` + one line in the registry. Several standalone tile binaries (`cal`, `crew`, `health`, `tasks`) reuse the same library crate with their own run loops.

**Background data fetching:** panels with slow sources (ping, commits, calendar) spawn worker threads and drain results over `mpsc` channels in `tick()`. The UI never blocks.

## Theme

Shared with the suite via `~/.config/dashboard-suite/theme.toml` (falls back to the Rep Cap defaults):

- Pink `#e88b9f` for active / now / current values
- Lavender `#c5a3ff` for historical / averages / axis labels
- Magenta `#ff6ec7` for alerts / peaks / "this number is bad"

`[` / `]` scale brightness on top of the palette.

## Suite

- `wt` worktree + session picker
- `recall` session browser
- `roam` file browser
- `glance` this thing, plus `suite-term` (shared crate) and `dashboard-suite` (installer)

Manual: https://dashboard-suite-constellation.netlify.app

## License

Private.
