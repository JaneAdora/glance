# glance

Tile-mode multi-panel dashboard widget. Fourth in the wt/recall/roam/glance suite.

One binary, many visualizations. Press `1`-`9` to jump to a panel, `n`/`p` to cycle, `q` to quit. Each panel runs at its own refresh rate; only the focused panel ticks. Background threads handle slow data sources (ping, git scans) without blocking the UI.

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

## Panels (v0.2)

| # | Name | What it shows | Refresh |
|---|------|---------------|---------|
| 1 | cpu     | Sparkline per core (last 60s) + top-5 processes by CPU | 500 ms |
| 2 | mem     | RAM gauge + Swap gauge + 5-min RAM history sparkline   | 500 ms |
| 3 | net     | Per-interface ↓/↑ throughput sparklines from /proc/net/dev | 500 ms |
| 4 | disk    | Per-mount usage gauges sorted by % full (df subprocess) | 5 s    |
| 5 | battery | Charge gauge + history sparkline (gracefully reports "no battery" on desktops) | 10 s |
| 6 | ping    | Multi-host latency Chart with one Dataset per host; background ping subprocesses | 1 s |
| 7 | commits | 13-week heatmap of daily commits across $WT_ROOTS / ~/projects (Canvas-rendered) | 5 min |
| 8 | peon    | Today's peon-ping reps vs daily goals as themed gauges | 5 s |

Planned (not in v0.2): emails-per-day (zele bar chart), activity-clock (Canvas radial clock with calendar events).

## Configuration

| Env var | Effect | Default |
|---|---|---|
| `GLANCE_PING_HOSTS` | Comma-separated hosts to ping | `1.1.1.1,8.8.8.8,github.com` |
| `WT_ROOTS` | Colon-separated repo roots for commits heatmap | `~/projects:~/Projects` |

A panels.toml config (~/.config/glance/panels.toml) for enabling/reordering panels is on the roadmap; v0.2 ships with a fixed `default_registry()`.

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

Main app holds `Vec<Box<dyn Panel>>`. Tick loop only refreshes the focused panel at its preferred interval. Adding a panel = one new file in `src/panels/` + one line in `default_registry()`.

**Background data fetching:** panels with slow sources (ping, commits) spawn worker threads and drain results over `mpsc` channels in `tick()`. UI never blocks.

## Theme

Shared with the suite (wt/recall/roam):

- Pink `#e88b9f` — active / now / current values
- Lavender `#c5a3ff` — historical / averages / axis labels
- Magenta `#ff6ec7` — alerts / peaks / "this number is bad"

Color-grading rule applied consistently:
- CPU < 50%, RAM < 70%, disk < 70% → lavender
- CPU 50-85%, RAM 70-90%, disk 70-90% → pink
- CPU ≥ 85%, RAM ≥ 90%, disk ≥ 90%, battery ≤ 15% → magenta-alert

## Suite

- `wt` — worktree picker
- `recall` — session browser
- `roam` — file browser
- `glance` — this thing

Roadmap: `~/projects/.dashboard-roadmap.md`

## License

Private.
