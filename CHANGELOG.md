# Changelog

## 2.1.1

### Fixed
- **Windows: every GPU is listed.** Adapters are enumerated through DXGI, so an
  integrated GPU now shows up next to a discrete NVIDIA card (NVML still owns the
  NVIDIA device, so nothing is listed twice). Dedicated memory totals and exact
  adapter names come from DXGI.

### Performance
- Dashboard CPU use drops roughly 5x (measured): bar animations forced about
  15 full repaints after every tick; they are gone.
- Chart paths hold one min/max pair per two *logical* pixels instead of per
  device pixel, which matters on HiDPI screens.
- Windows GPU counters are read with PDH on the engine thread instead of three
  WMI queries per second (WMI "formatted data" classes keep a provider process busy).
- Command lines are no longer requested from sysinfo: protected Windows
  processes never yield one, so every tick re-opened dozens of processes for
  nothing. The process filter matches name and PID.
- Disk throughput and open files are measured only for watched processes
  (`ProcessDetailProbe`, replacing `OpenFilesProbe`); rates use cumulative
  counters so a newly watched process no longer reports a spike.
- Nothing is formatted or drawn while the window is minimised or hidden;
  recording continues.
- Settings → Performance shows the engine's per-tick cost.

## 2.1.0

### Added
- **GPU monitoring** in a new `tvis-gpu` crate, plugged into the engine as a
  regular probe. Per-device utilisation, VRAM, temperature, power, clock and fan,
  plus **per-process GPU utilisation and GPU memory** that work with the existing
  charts, tiles, trends, alerts, leak detection ("possible VRAM leak") and exports.
  - NVIDIA (Linux, Windows): NVML, loaded at run time (no driver needed to run TVIS).
  - AMD / Intel (Linux): sysfs + hwmon, and DRM `fdinfo` for per-process engine time and memory.
  - Any vendor (Windows): WDDM performance counters through WMI.
  - macOS: device statistics through IOKit (`ioreg`); the OS exposes no per-process counters.
- Dashboard: GPU cards, utilisation chart and a "Top by GPU" list.
  Monitor and Processes pages get a GPU column; the Processes page can be sorted by it.
- `--no-gpu` flag to skip GPU probing.

## 2.0.0

A ground-up rewrite.

### Changed
- **New UI toolkit: [Slint](https://slint.dev) replaces egui.** Retained-mode
  rendering means charts no longer redraw the whole interface every frame; the
  old lag with several live graphs is gone.
- **Workspace architecture.** `tvis-core` (engine, no UI) and `tvis` (app).
  Adding a metric, a data source or a page no longer touches the core.
- Rust edition 2024, toolchain `stable` (MSRV 1.95), every dependency updated
  to its latest release.
- Settings are stored as TOML in the platform config directory.
- Releases are built natively on GitHub Actions for Linux (x86_64, aarch64),
  Windows and macOS (Intel, Apple Silicon).

### Added
- Dashboard: system CPU/memory/swap/load, per-core utilisation, top processes.
- Monitor: per-target KPI tiles, min/avg/p95/max, trend per minute, process tree
  with sparklines, hover crosshair with exact values, 30 s – 1 h windows.
- Chart modes: aggregate, per process, and compare all watched targets.
- Metrics: CPU, resident and virtual memory, threads, disk read/write rate,
  open files.
- Insights: automatic memory/handle/thread growth detection, CPU saturation
  and spike detection.
- Alerts: threshold rules with "for N seconds" semantics, event log, toasts.
- CSV and JSON export of recorded history.
- `tvis record <target>` for headless recording in CI and scripts.
- `--demo` mode with a synthetic machine.
- Dark, light and system themes with selectable accent colour.

### Removed
- The web (wasm) build and its GitHub Pages workflow.
