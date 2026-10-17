# TVIS architecture

TVIS 2 is a Cargo workspace with a strict dependency direction:

```
crates/tvis        (binary: Slint UI, CLI, settings)
   │         │
   │         └──► crates/tvis-gpu   (GPU backends: NVML, DRM, WDDM, IOKit)
   ▼                    │
crates/tvis-core ◄──────┘            (library: no UI, no vendor SDKs)
```

`tvis-core` never imports anything UI related, so it can be reused for a TUI,
a daemon or an agent without touching it.

## Data flow

```
 ProcessSource ──► Frame ──► Probe(s) ──► Store ──► AlertEngine
 (sysinfo / demo)  (one      (extra       (series,   (rules)
                   snapshot)  metrics)     targets)
                                   │
                         Engine thread (owns all of the above)
                                   │ EngineView (borrowed, read-only)
                                   ▼
                              Observer
                                   │  presenter::build()  ← runs on the engine thread
                                   ▼
                              ViewState  (plain, Send)
                                   │  latest-wins slot
                                   ▼
                       UI thread: Ui::apply()  → VecModel rows → Slint
```

The UI never reads the store and the engine never touches Slint. The only
shared mutable thing is a tiny `ViewRequest` (selected target, time window,
page…) that the UI writes and the presenter reads, so the presenter computes
exactly what is on screen and nothing else.

### Why this is fast

egui re-lays-out and re-tessellates every plot on every frame. Slint is
retained-mode, so TVIS only has to hand it *new property values*:

* **Heavy work is off the UI thread.** Statistics, min/max decimation, number
  formatting and SVG path generation happen in the presenter on the engine
  thread. The UI thread only assigns strings and numbers.
* **Latest wins.** If the UI is busy, intermediate `ViewState`s are dropped
  rather than queued.
* **Diffing models.** Every list is a persistent `VecModel`; `ui::sync` writes
  only rows that changed, and Slint only repaints the elements bound to the
  changed properties (typically one `Path` per chart line).
* **Bounded geometry.** Lines are decimated with a min/max bucket algorithm to
  at most two points per pixel, which keeps every spike while capping path size.

## `tvis-core` modules

| module     | purpose |
|------------|---------|
| `metric`   | `MetricId`, `MetricDescriptor` (name, unit, aggregation, flags) and `MetricRegistry` |
| `frame`    | `Frame`, `ProcessSample`, `SystemSample`: one immutable snapshot |
| `source`   | `ProcessSource` (base data) and `Probe` (extra metrics for watched processes) |
| `target`   | `Target::{Name, Pid}` and resolution to a process tree |
| `gpu`      | `GpuSample` and the per-process GPU metrics |
| `series`   | `TimeSeries` ring buffer, `decimate_min_max`, smoothing |
| `stats`    | `Summary` (min/avg/p50/p95/p99/max/σ), OLS `linear_fit` |
| `analysis` | automatic insights: growth (leaks), CPU saturation, spikes |
| `store`    | per-target aggregate and per-process history, system history |
| `alert`    | `AlertRule` / `AlertEngine` with "for N seconds" semantics |
| `engine`   | sampling thread, `Command`s in, `Observer` out |
| `export`   | CSV and JSON writers |
| `units`    | byte/percent/duration formatting, "nice" axis ticks |

## Extending TVIS

### Add a metric (e.g. GPU memory)

1. Pick an id and describe it:

   ```rust
   pub const GPU_MEM: MetricId = MetricId::new("gpu.mem");

   MetricDescriptor {
       id: GPU_MEM,
       name: "GPU memory",
       unit: Unit::Bytes,
       aggregation: Aggregation::Sum,
       per_process_history: false,
       leak_candidate: true,
   }
   ```

2. Implement `Probe`: return the descriptor from `descriptors()` and, in
   `probe()`, call `frame.process_mut(pid).unwrap().set(GPU_MEM, value)` for
   the pids in `ctx.watched`.
3. Pass the probe to `Engine::new(source, vec![Box::new(MyProbe)], config)`.

That is all. The store records it, the monitor page shows a card and a chart,
alerts can target it, insights watch it for growth and exports include it. No
core code changes.

### Add a data source

Implement `ProcessSource` (`info`, `descriptors`, `sample`, optionally
`backfill`). `SyntheticSource` (used by `--demo`) is a complete ~100 line
example; a file replay or remote-agent source looks the same.

### Add an insight

Add a rule to `analysis::analyze`; return an `Insight`. The presenter turns it
into a banner in `presenter::insight`.

### Add a page

1. `ui/pages/<name>.slint` with the view.
2. Properties and callbacks for it in `ui/backend.slint`.
3. A section in `view::ViewState`, built in `presenter::build`, applied in
   `ui::Ui::apply`.

## Testing

* `tvis-core`: unit tests for every module, plus engine tests that run the real
  thread against the synthetic source.
* `tvis`: presenter tests drive the whole view-model pipeline from synthetic
  frames without opening a window; chart geometry, settings persistence and
  the CLI definition are tested separately.
* UI smoke test: `tvis --demo --snapshot out.png --snapshot-page 1` renders any
  page headlessly (`xvfb-run` + `SLINT_BACKEND=winit-software`).

## GPU backends (`tvis-gpu`)

`GpuProbe` implements the core's `Probe` trait and merges a set of private
`Backend`s, each returning a `Reading { devices, processes }`:

* `nvml` — NVIDIA via `nvml-wrapper`; the library is `dlopen`ed so the binary
  runs without a driver. Per-process data from the compute/graphics process
  lists and `process_utilization_stats`.
* `drm` — Linux sysfs, hwmon and `/proc/<pid>/fdinfo` (`drm-engine-*` busy time,
  `drm-resident-*`/`drm-memory-*` memory) following the kernel's
  `drm-usage-stats` format. Clients are de-duplicated by `drm-client-id`.
* `windows` + `wddm` — WDDM performance counters read on a worker thread
  (WMI connections are not `Send`); `wddm` holds the pure instance-name parsing
  and Task-Manager-style aggregation.
* `mac` — `ioreg -a` parsed with the `plist` crate.

Every parser takes its input as data or a configurable root directory, so all
of them are unit tested on every platform; only the OS access is
`cfg`-gated. A backend that fails to initialise is simply absent, and when no
backend works `GpuProbe::detect()` returns `None` and no GPU metrics are
registered. To support another vendor or OS, add a `Backend`.
