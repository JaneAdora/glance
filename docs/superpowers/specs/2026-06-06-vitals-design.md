# vitals (Cockpit) Design -- 2026-06-06

## Goal

A standalone suite app, `vitals`: a single-screen "is my hardware OK right now?"
dashboard. Unlike glance (one panel at a time, cycle with n/p), it shows the key
hardware vitals all at once: a big color-coded top-line readout plus a grid of
detail panels. It is an APP IN THE SUITE, built in the glance crate, not a new
suite.

It answers four worries at a glance:

- Is my video card overloaded? -> GPU util + VRAM
- How much RAM is used?         -> RAM %
- Is my processor under load?   -> CPU %
- Is it overheating?            -> hottest temp (thermal zones + GPU)

## Architecture

A new binary at `src/bin/vitals.rs` in the existing glance crate. Cargo
auto-discovers `src/bin/*.rs`, so no `[[bin]]` entry is needed (same as the
existing `cal`, `crew`, `health`, `tasks` bins).

The binary owns CONCRETE panel instances (not `Box<dyn Panel>`), because the
vitals row needs typed, read-only accessors on specific panels:

```rust
struct Cockpit {
    cpu: CpuPanel,
    mem: MemPanel,
    gpu: GpuPanel,
    temp: TempPanel,
    fans: FansPanel,
    disk: DiskPanel,
    net: NetPanel,
    io: IoPanel,
}
```

There is exactly one instance per panel. Each frame, every panel is ticked once,
then used in two ways: read for the vitals row, and `render`ed into its grid
cell. No double reads, no duplicated metric logic.

The only genuinely new rendering code is the vitals row and the status line. The
detail grid is just `Rect` splits that call each existing panel's `render`.

### Why concrete instances, not the Panel registry

glance's `build_panel` returns `Box<dyn Panel>`, which only exposes the trait
(`name`/`tick`/`render`/`refresh_ms`/`handle_key`/`wants_keys`). The vitals row
needs to READ each panel's current metric (CPU %, RAM %, GPU util/VRAM/temp,
hottest temp). Concrete fields let us add small read-only accessor methods and
call them directly. This also keeps the metric logic in ONE place (the panel
already computes it for its own render); we just expose it.

## Vitals row (top)

Four big readouts, left to right, answering the four worries:

- `CPU  NN%`    -- average of per-core usage from CpuPanel.
- `RAM  NN%`    -- used memory percentage from MemPanel.
- `GPU  NN% V.V/V.VG` -- GPU utilization + VRAM used/total from GpuPanel; shows
  a dash (`GPU  --`) when no NVIDIA GPU is present.
- `TEMP NN°C`   -- the hottest of {hottest thermal zone, GPU temp}; shows a dash
  when no sensor is readable.

Below the four numbers, a one-line status:

- `ALL NOMINAL` when every readable metric is under its alarm threshold.
- Otherwise a list of offenders, e.g. `GPU 94%  ·  CPU 92%  ·  TEMP 84°C`
  (CPU/RAM/GPU shown as a percentage, TEMP in celsius).

Each metric is classified into two tiers for v1:

- Normal: rendered in the suite's normal colors (pink / lavender).
- Alarm: rendered in magenta (matches the temp panel's existing alert color).

A metric with no reading (no NVIDIA, no sensor) is "Unknown": it renders as a
dash, never counts as an alarm, and never appears in the offenders list.

### Alarm thresholds (v1, hardcoded constants)

```rust
const CPU_ALARM: u16 = 90;   // percent
const RAM_ALARM: u16 = 90;   // percent
const GPU_ALARM: u16 = 90;   // percent
const TEMP_ALARM: f64 = 80.0; // celsius
```

A configurable threshold file is explicitly OUT OF SCOPE for v1 (YAGNI). These
constants live at the top of `vitals.rs` and are easy to lift into config later.

### Panel accessors to add

Small, additive, read-only methods. They expose what each panel already computes
for its own render, so there is no duplicated metric logic. These also make the
panels more reusable across the suite.

- `MemPanel::used_pct(&self) -> u16` -- currently private; make `pub`.
- `CpuPanel::overall_pct(&self) -> u16` -- new; average of the latest per-core
  sample (mean of the most recent value in each core's history; 0 if no history
  yet).
- `GpuPanel::util(&self) -> Option<u16>` -- new; `Some(util)` when available,
  else `None`.
- `GpuPanel::vram(&self) -> Option<(u64, u64)>` -- new; `Some((used, total))`
  bytes-or-MiB as the panel already stores them, else `None`.
- `GpuPanel::temp(&self) -> Option<u16>` -- new; `Some(temp_c)` when available.
- `TempPanel::hottest(&self) -> Option<f64>` -- new; the hottest zone's celsius
  (zones are already sorted hottest-first), or `None` if no zones.
- `CpuPanel::core_count(&self) -> usize` -- new; number of logical cores, used to
  size the CPU row in the scrollable column so every core shows.

Note on GPU VRAM units: GpuPanel stores `mem_used` / `mem_total` from
`nvidia-smi --format=...,nounits` for the `memory.used,memory.total` fields,
which nvidia-smi reports in MiB. The accessor returns those raw MiB values; the
vitals row formats them as `V.V/V.VG` by dividing by 1024.

## Detail column (scrollable, below the vitals row)

> Revised 2026-06-07: the original 3x2 desktop grid was replaced by a single
> scrollable column. The real use case is a quick hardware check from a phone /
> SSH terminal (narrow and tall), where a grid does not fit. On the desktop the
> user already has taskbar gauges; `vitals` is the phone-friendly glance.

The screen is divided top to bottom: a pinned vitals row (2 lines), a separator
(1 line), the scrollable detail column (fills the middle), and a footer (1 line)
with the scroll keys and a scroll-position percentage. The vitals row and footer
stay fixed; only the column scrolls.

The detail column stacks all eight panels full-width, one above the next, in this
order: cpu, mem, gpu, temp, fans, disk, net, io. Each panel keeps its own border
and title (panels render their own `Block`); this is pure reuse, no new widget
code. CPU is given a row tall enough to show every core (`core_count + 2 + 9`);
the others use fixed generous heights and clip to their row if a machine has more
items than fit.

Scrolling is smooth and line-by-line. Because the `Panel` trait renders into a
`Frame` (not a free `Buffer`), the column is first rendered in full into an
off-screen `Terminal<TestBackend>` sized `width x content_height`, then the
visible slice `[scroll, scroll + viewport_height)` is blitted cell-by-cell into
the real frame via `Frame::buffer_mut`. This works for any column height and any
viewport size without per-widget clipping logic.

`loadavg` and `entropy` panels exist but are intentionally excluded from v1 (they
do not map to the four worries). Adding either later is appending one entry to the
column order plus its height.

## Layout behavior (no discrete modes)

There is no Full/Compact mode switch. Every frame renders the same structure:
pinned vitals row + separator + scrollable column + footer. On a tiny terminal
the column viewport is simply short (still scrollable); ratatui clips anything
that does not fit without panicking. This removes the earlier surprise where a
small terminal showed only a single line.

## Keys, flags, refresh

- `q` and Ctrl-C quit.
- `j` / `k` and Down / Up scroll the detail column (2 lines per press).
- Space / `b` (or PageDown / PageUp) page the column; `g` / `G` (or Home / End)
  jump to top / bottom.
- `[` / `]` dim / brighten, via the existing `glance::brightness` module (suite
  consistency with the other bins).
- `--help` / `--version` flags, matching the other bins' style.
- A single 1-second tick drives ALL panels each interval (one tick per panel per
  interval; avoids spawning `nvidia-smi` more often than needed).
- Key polling at 100ms so `q` and scrolling feel instant, independent of the 1s
  data tick.

The run loop mirrors `src/bin/health.rs`: install the suite panic hook
(`suite_term::panic::install_panic_hook()`), enable raw mode, enter the alternate
screen with `SetTitle("vitals")`, loop, and restore the terminal on exit.

## Error handling

- No NVIDIA GPU: GpuPanel renders its existing "unavailable" state in the column;
  the vitals GPU readout shows a dash and never alarms.
- No thermal zones / no fan sensors: those panels already degrade to an empty
  state; the vitals TEMP readout shows a dash when there is no readable zone and
  no GPU temp.
- Terminal too small: the column viewport is short but still scrollable; ratatui
  clips anything that does not fit without panicking.

## Testing

Unit tests cover the new pure logic (rendering is not unit-tested, consistent
with the existing panels):

- Threshold classification: a `classify(value, threshold) -> Status` style
  helper returns Normal below the threshold and Alarm at/above it; an Unknown
  (None) input returns Unknown.
- Status-line builder: given the four classified metrics, returns `ALL NOMINAL`
  when all are Normal/Unknown, and a `·`-joined offenders string (in a fixed
  order: GPU, CPU, RAM, TEMP) when any are Alarm; Unknown metrics never appear.
- Temperature combine: `combine_temp(zone, gpu_temp)` returns the hotter of the
  two, or whichever is present, or None when both are absent.

The panel accessors are thin and exercised indirectly (the accessor tests assert
the deterministic pre-tick states). The scrolling/blit and layout are verified by
build + pty/tmux smoke tests rather than unit tests, consistent with how the
existing panels' rendering is handled.

## Install

After it builds and tests pass, add a `[[launcher]]` entry (with a `url`
pointing at the glance repo, matching the other glance-crate bins) to
`~/projects/dashboard-suite/suite.toml` so `rsuite` builds and installs it. The
binary installs to `~/.local/bin/vitals` (the same prefix the other glance bins
use). This install/manifest step is done after the feature is built and verified;
it is noted here for completeness but lives in the dashboard-suite repo, not the
glance crate.

## Out of scope (v1)

- Configurable thresholds (hardcoded constants for v1).
- loadavg / entropy panels in the column.
- Per-panel refresh cadences (single 1s tick for all).
- Any interactivity beyond scrolling, quit, and brightness (no focus, no
  drill-in).
- A third "warning" tier (only Normal and Alarm for v1).

## Reuse references

- Standalone-bin pattern: `src/bin/health.rs` (panic hook, raw mode, alt screen,
  1s tick loop, `q`/Ctrl-C/`[`/`]` handling, `--help`/`--version`).
- Panel trait + registry: `src/panels/mod.rs`.
- Hardware panels: `src/panels/{cpu,mem,gpu,temp,fans,disk,net,io}.rs`.
- Brightness + theme: `glance::brightness`, `glance::theme` (pink / lavender /
  magenta).
- rsuite manifest: `~/projects/dashboard-suite/suite.toml`.
