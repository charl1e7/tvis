# Changelog

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
