# Vitals: per-resource "Top processes" tables (RAM + VRAM)

**Date:** 2026-06-10
**Status:** approved (recommended defaults)

## Goal

Show which processes are consuming RAM and VRAM, mirroring the CPU panel's
existing "Top processes" table (top 5 by CPU%). Answers "what's eating my
memory / video memory right now?" at a glance.

## Approach

"Like we do with CPU" = the existing idiom: a resource panel shows its own
top consumers inline, gated on having enough vertical room. So this is purely
additive to two existing panels, and vitals inherits it for free by reusing
them.

- **`mem` panel** grows a "Top processes" table sorted by **RAM (RSS)**, top 5:
  `RSS | PID | Command`. Shown only when the pane is tall enough (same gate
  pattern as `cpu`); collapses gracefully on short panes / phone width.
- **`gpu` panel** grows a "Top processes" table sorted by **VRAM (MiB)**, top 5:
  `VRAM | PID | Command`. Same height gate.
- **`vitals`** scrollable column: bump the `mem` and `gpu` row heights in
  `panel_heights()` so both tables are visible. No other vitals change.

Because `vitals` embeds the real `mem`/`gpu` panels, and the standalone
`mem`/`gpu` glance commands + the glance grid use the same panels, all
surfaces get the tables from one change.

## Data sources

- **RAM:** the `sysinfo` crate. `MemPanel` starts enumerating processes
  (`with_processes` + `refresh_processes(All)`, mirroring `CpuPanel`); each
  process exposes `memory()` (RSS bytes). Sort desc, take 5, format via the
  existing `human()`.
- **VRAM:** parse `nvidia-smi`'s full **text process table**, NOT the CSV
  `--query-compute-apps` query. The CSV returns compute contexts only and
  silently omits graphics contexts (verified: it missed cosmic-comp at 672 MiB
  and a Chromium tab at 1.66 GiB, the two largest consumers). The text table
  lists every G / C+G process with MiB. We extract `(pid, mib)` and resolve a
  clean command name from `/proc/<pid>/comm` (so `chrome`, not
  `...rack-uuid=3190...`), falling back to the table name. One extra
  `nvidia-smi` spawn per gpu tick (~1.5s cadence); no D-Bus, negligible.

## Parser (the testable unit)

`fn parse_gpu_procs(nvidia_smi_text: &str) -> Vec<(u32, u64)>` — pid, MiB.

Robust tokenization per process row (handles MIG `N/A` GI/CI, multi-word and
`...`-truncated names): find the Type token (`C` | `G` | `C+G`); PID is the
token before it; memory is the trailing token ending in `MiB`. Sum by PID
(defensive against multi-context rows), then the caller sorts and takes top 5.

Unit tests: parse the real captured `nvidia-smi` output; handle empty / no-process
output; a top-N-by-memory sort test for the RAM side.

## Defaults (approved)

- 5 rows per table (matches CPU).
- RAM by RSS (matches `ps`/`htop`, free from sysinfo; PSS rejected as pricier
  with no glance-level benefit).
- Tables in both `mem` and `gpu` panels (lights up vitals + standalone +
  grid).

## Out of scope

- Per-process GPU *utilization* (SM %): available via `nvidia-smi pmon` but
  reports idle for most processes on a desktop/graphics workload; overall GPU
  util% (already shown) covers it. VRAM is the meaningful per-process metric.
- Consolidating the two nvidia-smi calls into one text parse (possible later;
  keeping the structured CSV for gauges is more robust for now).
