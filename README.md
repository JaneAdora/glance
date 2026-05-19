# glance

Tile-mode multi-panel dashboard widget. Fourth in the wt/recall/roam/glance suite.

One binary, many visualizations. Press `1`-`9` to jump to a panel, `n`/`p` to cycle, `q` to quit. Each panel runs at its own refresh rate; only the focused panel ticks.

## Install

```
cd ~/projects/glance
cargo build --release
install -m 0755 target/release/glance ~/.local/bin/glance
```

## Usage

```
glance            # launch dashboard (interactive TTY required)
glance --help     # show CLI help
```

## Built-in panels (v0.1)

- **cpu** — sparkline per core (last 60 samples) + top-5 processes by CPU
- **mem** — RAM gauge + Swap gauge + 5-min RAM history sparkline

Roadmap: disk-viz, net-graph, ping-graph, battery, peon-log-viz, commits-heatmap, emails-per-day, activity-clock. See `~/projects/.dashboard-roadmap.md`.

## Keys

```
1-9 / 0    jump to panel by slot
n / Tab    next panel
p          previous panel
r          force-refresh current panel
?          help
q / Esc    quit (Ctrl-C also works)
```

## Architecture

`Panel` trait (`src/panels/mod.rs`):

```rust
pub trait Panel {
    fn name(&self) -> &str;
    fn tick(&mut self);
    fn render(&self, f: &mut Frame, area: Rect);
    fn refresh_ms(&self) -> u64 { 500 }
}
```

Main app holds `Vec<Box<dyn Panel>>`. Tick loop only refreshes the focused panel at its preferred interval. Adding a panel = one new file in `src/panels/` + a line in `default_registry()`.

## Theme

Shared with the suite (wt/recall/roam):

- Pink `#e88b9f` — active / now / current values
- Lavender `#c5a3ff` — historical / averages / axis labels
- Magenta `#ff6ec7` — alerts / peaks / "this number is bad"

## Suite

- `wt` — worktree picker
- `recall` — session browser
- `roam` — file browser
- `glance` — this thing

## License

Private.
