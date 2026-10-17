//! Builds a [`ViewState`] from an engine snapshot.
//!
//! Runs on the engine thread, so all heavy work (statistics, decimation, path
//! generation, formatting) happens off the UI thread. The result is a plain
//! value the UI merely copies into its models.

use std::collections::HashMap;

use tvis_core::Target;
use tvis_core::analysis::{Insight, InsightKind, Severity, analyze};
use tvis_core::engine::EngineView;
use tvis_core::frame::{Frame, ProcessSample};
use tvis_core::metric::{MetricDescriptor, MetricId, Unit, builtin};
use tvis_core::series::Point;
use tvis_core::stats::{Summary, linear_fit};
use tvis_core::store::{TargetData, TargetStatus};
use tvis_core::units::{
    ByteBase, format_bytes, format_duration, format_percent, format_trend, format_value,
};

use super::chart::{self, Input, Plot, YScale};
use super::view::*;

const TOP_N: usize = 8;
const MAX_PROCESS_ROWS: usize = 400;
const PER_PROCESS_LINES: usize = 6;
const MAX_COMPARE_LINES: usize = 8;

struct Ctx<'a, 'v> {
    view: &'a EngineView<'v>,
    req: &'a ViewRequest,
    cores: f64,
}

impl Ctx<'_, '_> {
    /// Display scale of a metric (CPU may be shown relative to the whole machine).
    fn factor(&self, metric: MetricId) -> f64 {
        if self.req.cpu_normalized && metric == builtin::CPU {
            1.0 / self.cores
        } else {
            1.0
        }
    }

    fn base(&self) -> ByteBase {
        self.req.byte_base
    }

    fn fmt(&self, unit: Unit, v: f64) -> String {
        format_value(unit, v, self.base())
    }

    fn plot(&self) -> Plot {
        Plot {
            now: self.view.now,
            window: self.req.window_secs,
            width_px: self.req.chart_px,
            smoothing: self.req.smoothing,
            gap: (self.view.config.interval.as_secs_f64() * 3.0).max(2.0),
        }
    }

    fn since(&self) -> f64 {
        self.view.now - self.req.window_secs
    }
}

fn scaled_points(series: &tvis_core::series::TimeSeries, from: f64, factor: f64) -> Vec<Point> {
    series
        .since(from)
        .map(|p| Point {
            t: p.t,
            v: p.v * factor,
        })
        .collect()
}

/// Entry point.
pub fn build(view: &EngineView<'_>, req: &ViewRequest) -> ViewState {
    let cores = view.system.cpu_cores.max(1) as f64;
    let cx = Ctx { view, req, cores };

    let firing = view.alerts.active();
    let interval = view.config.interval;
    let sampling_label = format!(
        "every {} · {} of history",
        if interval.as_millis() < 1000 {
            format!("{} ms", interval.as_millis())
        } else {
            format!("{} s", interval.as_secs_f64())
        },
        format_duration(view.config.retention.as_secs_f64())
    );

    let mut state = ViewState {
        sampling_label,
        host: format!("{} · {}", view.system.host_name, view.system.os)
            .trim_matches([' ', '·'])
            .to_owned(),
        live: view.frame.is_some(),
        paused: view.config.paused,
        has_gpu: view.frame.is_some_and(|f| !f.gpus.is_empty()),
        firing_rules: firing.iter().map(|(rule, _)| *rule).collect(),
        targets: view
            .store
            .targets()
            .iter()
            .map(|t| (t.id().to_string(), t.title()))
            .collect(),
        ..ViewState::default()
    };

    state.watchlist = view
        .store
        .targets()
        .iter()
        .map(|t| watch_row(&cx, t, firing.iter().any(|(_, id)| *id == t.id())))
        .collect();

    match req.page {
        Page::Dashboard => state.dashboard = Some(dashboard(&cx)),
        Page::Monitor | Page::Alerts | Page::Settings => {
            // The monitor view is cheap to keep warm and is what users return to.
            if req.page == Page::Monitor {
                state.monitor = req
                    .selected
                    .as_ref()
                    .and_then(|id| view.store.target(id))
                    .map(|t| monitor(&cx, t));
            }
        }
        Page::Processes => state.processes = Some(processes(&cx)),
    }
    state
}

// ── watchlist ────────────────────────────────────────────────────────────

fn watch_row(cx: &Ctx<'_, '_>, t: &TargetData, alerting: bool) -> WatchRowV {
    let running = t.status == TargetStatus::Running;
    let cpu = t
        .current(builtin::CPU)
        .map(|v| format_percent(v * cx.factor(builtin::CPU)))
        .unwrap_or_else(|| "—".into());
    let mem = t
        .current(builtin::MEM_RSS)
        .map(|v| format_bytes(v, cx.base()))
        .unwrap_or_else(|| "—".into());
    let spark = t
        .series(builtin::CPU)
        .map(|s| {
            let pts: Vec<Point> = s.since(cx.view.now - 120.0).collect();
            chart::sparkline(&pts, 120.0, cx.view.now, 40)
        })
        .unwrap_or_default();
    WatchRowV {
        id: t.id().to_string(),
        title: t.title(),
        subtitle: if running {
            format!(
                "{} proc · {} thr",
                t.process_count(),
                t.thread_count().max(thread_metric(t))
            )
        } else {
            "waiting for process…".into()
        },
        running,
        cpu,
        mem,
        spark,
        selected: cx.req.selected.as_ref() == Some(&t.id()),
        alerting,
    }
}

/// Thread total: kernel tasks if listed, otherwise the reported thread metric.
fn thread_metric(t: &TargetData) -> usize {
    t.current(builtin::THREADS).map_or(0, |v| v as usize)
}

// ── dashboard ────────────────────────────────────────────────────────────

fn dashboard(cx: &Ctx<'_, '_>) -> DashboardV {
    let store = cx.view.store;
    let info = cx.view.system;
    let Some(frame) = cx.view.frame else {
        return DashboardV::default();
    };
    let s = &frame.system;
    let base = cx.base();
    let window = cx.req.window_secs;
    let now = cx.view.now;
    let ratio = |used: u64, total: u64| {
        if total == 0 {
            0.0
        } else {
            used as f32 / total as f32
        }
    };

    let spark = |series: &tvis_core::series::TimeSeries| {
        let pts: Vec<Point> = series.since(now - 120.0).collect();
        chart::sparkline(&pts, 120.0, now, 50)
    };
    let have_load = s.load_average.iter().any(|v| *v > 0.0);

    let system = SystemV {
        cpu: format_percent(s.cpu_total),
        cpu_frac: (s.cpu_total / 100.0) as f32,
        cpu_spark: spark(&store.system.cpu),
        mem: format_bytes(s.mem_used as f64, base),
        mem_detail: format!(
            "of {} · {:.0}%",
            format_bytes(s.mem_total as f64, base),
            ratio(s.mem_used, s.mem_total) * 100.0
        ),
        mem_frac: ratio(s.mem_used, s.mem_total),
        mem_spark: spark(&store.system.memory),
        swap: format_bytes(s.swap_used as f64, base),
        swap_detail: if s.swap_total == 0 {
            "no swap configured".into()
        } else {
            format!(
                "of {} · {:.0}%",
                format_bytes(s.swap_total as f64, base),
                ratio(s.swap_used, s.swap_total) * 100.0
            )
        },
        swap_frac: ratio(s.swap_used, s.swap_total),
        load: if have_load {
            format!("{:.2}", s.load_average[0])
        } else {
            "n/a".into()
        },
        load_detail: if have_load {
            format!(
                "5 min {:.2} · 15 min {:.2}",
                s.load_average[1], s.load_average[2]
            )
        } else {
            "not reported on this platform".into()
        },
        uptime: format!("up {}", format_duration(s.uptime_secs as f64)),
        processes: format!("{} processes", s.process_count),
        cpu_model: if info.cpu_brand.is_empty() {
            format!("{} logical cores", info.cpu_cores)
        } else {
            format!("{} · {} threads", info.cpu_brand, info.cpu_cores)
        },
    };

    let cores = s
        .cpu_per_core
        .iter()
        .enumerate()
        .map(|(i, v)| (format!("CPU {i}"), (*v / 100.0) as f32, format!("{v:.0}%")))
        .collect();

    let plot = cx.plot();
    let cpu_chart = chart::build(
        vec![Input {
            name: "CPU".into(),
            color: SERIES_COLORS[0],
            points: scaled_points(&store.system.cpu, cx.since(), 1.0),
        }],
        Unit::Percent,
        base,
        plot,
        YScale::Fixed(100.0),
    );
    let mem_chart = chart::build(
        vec![Input {
            name: "Memory".into(),
            color: SERIES_COLORS[1],
            points: scaled_points(&store.system.memory, cx.since(), 1.0),
        }],
        Unit::Bytes,
        base,
        plot,
        if s.mem_total > 0 {
            YScale::Fixed(s.mem_total as f64)
        } else {
            YScale::Auto
        },
    );
    let _ = window;

    let groups = group_by_name(frame);
    let watched = |name: &str| {
        store
            .targets()
            .iter()
            .any(|t| t.target.id() == Target::Name(name.into()).id())
    };
    let top = |key: fn(&Group) -> f64, fmt: &dyn Fn(f64) -> String| {
        let mut v: Vec<(&String, &Group)> = groups.iter().collect();
        v.sort_by(|a, b| key(b.1).total_cmp(&key(a.1)).then_with(|| a.0.cmp(b.0)));
        let max = v.first().map_or(1.0, |g| key(g.1)).max(f64::EPSILON);
        v.into_iter()
            .take(TOP_N)
            .map(|(name, g)| TopRowV {
                name: name.clone(),
                value: fmt(key(g)),
                frac: (key(g) / max) as f32,
                count: if g.count > 1 {
                    format!("×{}", g.count)
                } else {
                    String::new()
                },
                key: name.clone(),
                watched: watched(name),
            })
            .collect::<Vec<_>>()
    };
    let (f_cpu, f_mem) = (cx.factor(builtin::CPU), 1.0);
    let top_cpu = top(|g| g.cpu, &|v| format_percent(v * f_cpu));
    let top_mem = top(|g| g.mem, &|v| format_bytes(v * f_mem, base));

    let (gpus, gpu_chart, top_gpu) = gpu_section(cx, frame, &groups, &watched);

    DashboardV {
        gpus,
        gpu_chart,
        top_gpu,
        system,
        cores,
        cpu_chart: Some(cpu_chart),
        mem_chart: Some(mem_chart),
        top_cpu,
        top_mem,
    }
}

/// `"12% · 340 MiB"`, with whichever halves are known.
fn gpu_text(cx: &Ctx<'_, '_>, util: Option<f64>, mem: Option<f64>) -> String {
    match (util, mem) {
        (Some(u), Some(m)) => format!("{} · {}", format_percent(u), format_bytes(m, cx.base())),
        (Some(u), None) => format_percent(u),
        (None, Some(m)) => format_bytes(m, cx.base()),
        (None, None) => "—".into(),
    }
}

fn gpu_section(
    cx: &Ctx<'_, '_>,
    frame: &Frame,
    groups: &HashMap<String, Group>,
    watched: &dyn Fn(&str) -> bool,
) -> (Vec<GpuCardV>, Option<ChartV>, Vec<TopRowV>) {
    if frame.gpus.is_empty() {
        return (Vec::new(), None, Vec::new());
    }
    let store = cx.view.store;
    let now = cx.view.now;
    let base = cx.base();
    let spark = |series: Option<&tvis_core::series::TimeSeries>| {
        series
            .map(|s| {
                chart::sparkline(
                    &s.since(now - 120.0).collect::<Vec<Point>>(),
                    120.0,
                    now,
                    50,
                )
            })
            .unwrap_or_default()
    };

    let cards = frame
        .gpus
        .iter()
        .map(|g| {
            let hist = store.gpu_history(&g.id);
            let frac = |used: Option<u64>, total: Option<u64>| match (used, total) {
                (Some(u), Some(t)) if t > 0 => (u as f64 / t as f64) as f32,
                _ => -1.0,
            };
            GpuCardV {
                name: g.name.clone(),
                vendor: g.vendor.name().to_owned(),
                util: g.utilization.map_or_else(|| "—".into(), format_percent),
                util_frac: g.utilization.map_or(-1.0, |u| (u / 100.0) as f32),
                mem: match (g.mem_used, g.mem_total) {
                    (Some(u), Some(t)) => format!(
                        "{} / {}",
                        format_bytes(u as f64, base),
                        format_bytes(t as f64, base)
                    ),
                    (Some(u), None) => format_bytes(u as f64, base),
                    _ => "—".into(),
                },
                mem_frac: frac(g.mem_used, g.mem_total),
                temp: g
                    .temperature
                    .map_or_else(|| "—".into(), |t| format!("{t:.0} °C")),
                power: g.power.map_or_else(|| "—".into(), |w| format!("{w:.0} W")),
                clock: g
                    .clock_mhz
                    .map_or_else(|| "—".into(), |c| format!("{c:.0} MHz")),
                spark: spark(hist.map(|h| &h.utilization)),
            }
        })
        .collect();

    let inputs = frame
        .gpus
        .iter()
        .enumerate()
        .filter_map(|(i, g)| {
            let h = store.gpu_history(&g.id)?;
            Some(Input {
                name: g.name.clone(),
                color: SERIES_COLORS[i % SERIES_COLORS.len()],
                points: scaled_points(&h.utilization, cx.since(), 1.0),
            })
        })
        .collect();
    let gpu_chart = chart::build(inputs, Unit::Percent, base, cx.plot(), YScale::Fixed(100.0));

    // Busiest GPU users, by memory (utilisation breaks ties and covers
    // backends that cannot report memory).
    let mut users: Vec<(&String, &Group)> = groups
        .iter()
        .filter(|(_, g)| g.gpu_mem > 0.0 || g.gpu_util > 0.0)
        .collect();
    users.sort_by(|a, b| {
        b.1.gpu_mem
            .total_cmp(&a.1.gpu_mem)
            .then(b.1.gpu_util.total_cmp(&a.1.gpu_util))
            .then_with(|| a.0.cmp(b.0))
    });
    let max = users
        .first()
        .map_or(1.0, |(_, g)| g.gpu_mem.max(g.gpu_util))
        .max(f64::EPSILON);
    let top = users
        .into_iter()
        .take(TOP_N)
        .map(|(name, g)| TopRowV {
            name: name.clone(),
            value: gpu_text(
                cx,
                (g.gpu_util > 0.0).then_some(g.gpu_util),
                (g.gpu_mem > 0.0).then_some(g.gpu_mem),
            ),
            frac: (g.gpu_mem.max(g.gpu_util) / max) as f32,
            count: if g.count > 1 {
                format!("×{}", g.count)
            } else {
                String::new()
            },
            key: name.clone(),
            watched: watched(name),
        })
        .collect();
    (cards, Some(gpu_chart), top)
}

#[derive(Default)]
struct Group {
    cpu: f64,
    mem: f64,
    threads: f64,
    gpu_util: f64,
    gpu_mem: f64,
    count: usize,
    pid: u32,
}

fn group_by_name(frame: &Frame) -> HashMap<String, Group> {
    let mut map: HashMap<String, Group> = HashMap::new();
    for p in frame.processes.iter().filter(|p| !p.is_thread()) {
        let g = map.entry(p.name.clone()).or_default();
        g.cpu += p.value(builtin::CPU);
        g.mem += p.value(builtin::MEM_RSS);
        g.threads += p.value(builtin::THREADS);
        g.gpu_util += p.value(builtin::GPU_UTIL);
        g.gpu_mem += p.value(builtin::GPU_MEM);
        g.count += 1;
        g.pid = p.pid;
    }
    map
}

// ── processes page ───────────────────────────────────────────────────────

fn processes(cx: &Ctx<'_, '_>) -> ProcessesV {
    let Some(frame) = cx.view.frame else {
        return ProcessesV {
            rows: vec![],
            summary: "Waiting for the first sample…".into(),
        };
    };
    let req = cx.req;
    let needle = req.proc_filter.trim().to_lowercase();
    let f_cpu = cx.factor(builtin::CPU);
    let store = cx.view.store;
    let is_watched = |t: Target| store.targets().iter().any(|d| d.id() == t.id());

    struct Row {
        key: String,
        name: String,
        pid: u32,
        count: usize,
        cpu: f64,
        mem: f64,
        threads: f64,
        gpu_util: f64,
        gpu_mem: f64,
        is_pid: bool,
    }

    let mut rows: Vec<Row> = if req.proc_group {
        group_by_name(frame)
            .into_iter()
            .filter(|(name, _)| needle.is_empty() || name.to_lowercase().contains(&needle))
            .map(|(name, g)| Row {
                key: name.clone(),
                name,
                pid: g.pid,
                count: g.count,
                cpu: g.cpu,
                mem: g.mem,
                threads: g.threads,
                gpu_util: g.gpu_util,
                gpu_mem: g.gpu_mem,
                is_pid: false,
            })
            .collect()
    } else {
        frame
            .processes
            .iter()
            .filter(|p| !p.is_thread())
            .filter(|p| {
                needle.is_empty()
                    || p.name.to_lowercase().contains(&needle)
                    || p.pid.to_string().contains(&needle)
                    || p.command.to_lowercase().contains(&needle)
            })
            .map(|p: &ProcessSample| Row {
                key: p.pid.to_string(),
                name: p.name.clone(),
                pid: p.pid,
                count: 1,
                cpu: p.value(builtin::CPU),
                mem: p.value(builtin::MEM_RSS),
                threads: p.value(builtin::THREADS),
                gpu_util: p.value(builtin::GPU_UTIL),
                gpu_mem: p.value(builtin::GPU_MEM),
                is_pid: true,
            })
            .collect()
    };
    let total = rows.len();

    rows.sort_by(|a, b| {
        let ord = match req.proc_sort {
            ProcSort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            ProcSort::Cpu => a.cpu.total_cmp(&b.cpu),
            ProcSort::Memory => a.mem.total_cmp(&b.mem),
            ProcSort::Threads => a.threads.total_cmp(&b.threads),
            ProcSort::Gpu => a
                .gpu_mem
                .total_cmp(&b.gpu_mem)
                .then(a.gpu_util.total_cmp(&b.gpu_util)),
            ProcSort::Pid if req.proc_group => a.count.cmp(&b.count),
            ProcSort::Pid => a.pid.cmp(&b.pid),
        };
        let ord = if req.proc_ascending {
            ord
        } else {
            ord.reverse()
        };
        ord.then_with(|| a.pid.cmp(&b.pid))
    });
    rows.truncate(MAX_PROCESS_ROWS);

    let max_cpu = rows.iter().map(|r| r.cpu).fold(f64::EPSILON, f64::max);
    let max_mem = rows.iter().map(|r| r.mem).fold(f64::EPSILON, f64::max);
    let shown = rows.len();
    let out = rows
        .into_iter()
        .map(|r| ProcRowV {
            watched: if r.is_pid {
                is_watched(Target::Pid(r.pid))
            } else {
                is_watched(Target::Name(r.name.clone()))
            },
            pid: if req.proc_group {
                format!("×{}", r.count)
            } else {
                r.pid.to_string()
            },
            cpu: format_percent(r.cpu * f_cpu),
            cpu_frac: (r.cpu / max_cpu) as f32,
            mem: format_bytes(r.mem, cx.base()),
            mem_frac: (r.mem / max_mem) as f32,
            gpu: gpu_text(
                cx,
                (r.gpu_util > 0.0).then_some(r.gpu_util),
                (r.gpu_mem > 0.0).then_some(r.gpu_mem),
            ),
            threads: if r.threads > 0.0 {
                format!("{:.0}", r.threads)
            } else {
                "—".into()
            },
            key: r.key,
            name: r.name,
            is_pid: r.is_pid,
        })
        .collect();

    ProcessesV {
        rows: out,
        summary: format!(
            "{shown} of {total} {} · {} running in total",
            if req.proc_group {
                "groups"
            } else {
                "processes"
            },
            frame.system.process_count
        ),
    }
}

// ── monitor page ─────────────────────────────────────────────────────────

fn monitor(cx: &Ctx<'_, '_>, data: &TargetData) -> MonitorV {
    let registry = cx.view.registry;
    let req = cx.req;
    let running = data.status == TargetStatus::Running;

    let available: Vec<&MetricDescriptor> =
        registry.iter().filter(|d| data.has_metric(d.id)).collect();
    let selected = available
        .iter()
        .find(|d| d.id.as_str() == req.metric)
        .or(available.first())
        .copied();

    let procs = data.process_count();
    let threads = data.thread_count().max(thread_metric(data));
    let subtitle = if running {
        let pids: Vec<String> = data
            .members
            .iter()
            .filter(|p| !p.is_thread())
            .take(4)
            .map(|p| p.pid.to_string())
            .collect();
        format!(
            "{procs} process{} · {threads} threads · PID {}{}",
            if procs == 1 { "" } else { "es" },
            pids.join(", "),
            if procs > 4 { "…" } else { "" }
        )
    } else {
        "No matching process right now — the watch stays armed and resumes automatically.".into()
    };

    let cards = available
        .iter()
        .map(|d| card(cx, data, d, selected))
        .collect();

    let (chart, chart_title, stats) = match selected {
        Some(d) => {
            let (chart, title) = main_chart(cx, data, d);
            (Some(chart), title, stats(cx, data, d))
        }
        None => (None, String::new(), Vec::new()),
    };

    let insights = analyze(data, registry, req.window_secs.max(60.0))
        .iter()
        .filter_map(|i| insight(cx, i))
        .collect();

    MonitorV {
        id: data.id().to_string(),
        title: data.title(),
        subtitle,
        running,
        chart_title,
        cards,
        chart,
        stats,
        insights,
        tree: tree(cx, data, selected),
    }
}

fn scale_summary(s: Summary, f: f64) -> Summary {
    Summary {
        min: s.min * f,
        max: s.max * f,
        mean: s.mean * f,
        stddev: s.stddev * f,
        p50: s.p50 * f,
        p95: s.p95 * f,
        p99: s.p99 * f,
        ..s
    }
}

fn card(
    cx: &Ctx<'_, '_>,
    data: &TargetData,
    d: &MetricDescriptor,
    selected: Option<&MetricDescriptor>,
) -> CardV {
    let f = cx.factor(d.id);
    let window = cx.req.window_secs;
    let series = data.series(d.id);
    let summary = data.summary(d.id, window).map(|s| scale_summary(s, f));
    let value = data
        .current(d.id)
        .map(|v| cx.fmt(d.unit, v * f))
        .unwrap_or_else(|| "—".into());
    let detail = summary
        .map(|s| {
            format!(
                "avg {} · p95 {} · max {}",
                cx.fmt(d.unit, s.mean),
                cx.fmt(d.unit, s.p95),
                cx.fmt(d.unit, s.max)
            )
        })
        .unwrap_or_default();

    let (trend, trend_dir) = series
        .and_then(|s| {
            let pts: Vec<Point> = s.since(cx.since()).collect();
            let span = pts.last()?.t - pts.first()?.t;
            if pts.len() < 10 || span < 20.0 {
                return None;
            }
            let fit = linear_fit(pts.iter().map(|p| (p.t, p.v * f)))?;
            let mean = summary?.mean.abs().max(f64::EPSILON);
            let change = fit.slope * span;
            let noticeable = fit.r2 > 0.5 && change.abs() > 0.03 * mean;
            Some(if noticeable {
                (
                    format_trend(d.unit, fit.slope, cx.base()),
                    if fit.slope > 0.0 { 1 } else { -1 },
                )
            } else {
                ("steady".to_owned(), 0)
            })
        })
        .unwrap_or_else(|| (String::new(), 0));

    let spark = series
        .map(|s| {
            let pts: Vec<Point> = s.since(cx.since()).collect();
            chart::sparkline(&pts, window, cx.view.now, 40)
        })
        .unwrap_or_default();

    CardV {
        id: d.id.to_string(),
        name: d.name.to_owned(),
        value,
        detail,
        trend,
        trend_dir,
        spark,
        color: color_for_metric(cx, d.id),
        selected: selected.is_some_and(|s| s.id == d.id),
    }
}

fn color_for_metric(cx: &Ctx<'_, '_>, id: MetricId) -> u32 {
    let idx = cx
        .view
        .registry
        .iter()
        .position(|d| d.id == id)
        .unwrap_or(0);
    SERIES_COLORS[idx % SERIES_COLORS.len()]
}

fn main_chart(cx: &Ctx<'_, '_>, data: &TargetData, d: &MetricDescriptor) -> (ChartV, String) {
    let f = cx.factor(d.id);
    let from = cx.since();
    let plot = cx.plot();
    let base = cx.base();

    let aggregate = |color| Input {
        name: data.title(),
        color,
        points: data
            .series(d.id)
            .map(|s| scaled_points(s, from - plot.gap, f))
            .unwrap_or_default(),
    };

    match cx.req.chart_view {
        ChartView::PerProcess if d.per_process_history => {
            let mut ranked: Vec<(&ProcessSample, f64, Vec<Point>)> = data
                .members
                .iter()
                .filter(|p| !p.is_thread())
                .filter_map(|p| {
                    let pts = scaled_points(data.process_series(p.pid, d.id)?, from - plot.gap, f);
                    let visible: Vec<f64> =
                        pts.iter().filter(|q| q.t >= from).map(|q| q.v).collect();
                    let mean = visible.iter().sum::<f64>() / visible.len().max(1) as f64;
                    Some((p, mean, pts))
                })
                .collect();
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
            let inputs = ranked
                .into_iter()
                .take(PER_PROCESS_LINES)
                .enumerate()
                .map(|(i, (p, _, points))| Input {
                    name: format!("{} ({})", p.name, p.pid),
                    color: SERIES_COLORS[i % SERIES_COLORS.len()],
                    points,
                })
                .collect();
            (
                chart::build(inputs, d.unit, base, plot, YScale::Auto),
                format!("{} by process", d.name),
            )
        }
        ChartView::Compare => {
            let inputs = cx
                .view
                .store
                .targets()
                .iter()
                .filter_map(|t| Some((t, t.series(d.id)?)))
                .take(MAX_COMPARE_LINES)
                .enumerate()
                .map(|(i, (t, s))| Input {
                    name: t.title(),
                    color: SERIES_COLORS[i % SERIES_COLORS.len()],
                    points: scaled_points(s, from - plot.gap, f),
                })
                .collect();
            (
                chart::build(inputs, d.unit, base, plot, YScale::Auto),
                format!("{} across watched targets", d.name),
            )
        }
        _ => (
            chart::build(
                vec![aggregate(color_for_metric(cx, d.id))],
                d.unit,
                base,
                plot,
                YScale::Auto,
            ),
            d.name.to_owned(),
        ),
    }
}

fn stats(cx: &Ctx<'_, '_>, data: &TargetData, d: &MetricDescriptor) -> Vec<(String, String)> {
    let f = cx.factor(d.id);
    let Some(s) = data
        .summary(d.id, cx.req.window_secs)
        .map(|s| scale_summary(s, f))
    else {
        return Vec::new();
    };
    let current = data.current(d.id).map(|v| v * f);
    let fmt = |v: f64| cx.fmt(d.unit, v);
    vec![
        (
            "Current".into(),
            current.map(fmt).unwrap_or_else(|| "—".into()),
        ),
        ("Min".into(), fmt(s.min)),
        ("Average".into(), fmt(s.mean)),
        ("Median".into(), fmt(s.p50)),
        ("P95".into(), fmt(s.p95)),
        ("P99".into(), fmt(s.p99)),
        ("Max".into(), fmt(s.max)),
        ("Std dev".into(), fmt(s.stddev)),
    ]
}

fn insight(cx: &Ctx<'_, '_>, i: &Insight) -> Option<InsightV> {
    let d = cx.view.registry.get(i.metric)?;
    let f = cx.factor(i.metric);
    let severity = match i.severity {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Critical => 2,
    };
    let (title, detail) = match i.kind {
        InsightKind::Growth {
            per_sec,
            confidence,
        } => {
            let title = match i.metric {
                builtin::MEM_RSS => "Possible memory leak".to_owned(),
                builtin::OPEN_FILES => "Open files keep growing — possible handle leak".to_owned(),
                builtin::GPU_MEM => "GPU memory keeps growing — possible VRAM leak".to_owned(),
                builtin::THREADS => "Thread count keeps growing".to_owned(),
                _ => format!("{} keeps growing", d.name),
            };
            (
                title,
                format!(
                    "{} steadily (fit confidence {:.0}%)",
                    format_trend(d.unit, per_sec * f, cx.base()),
                    confidence * 100.0
                ),
            )
        }
        InsightKind::Saturation { mean } => (
            "CPU saturated".to_owned(),
            format!(
                "averaging {} of a core over the recent window",
                format_percent(mean)
            ),
        ),
        InsightKind::Spike { peak, typical } => (
            format!("{} spike", d.name),
            format!(
                "peaked at {} while typically {}",
                cx.fmt(d.unit, peak * f),
                cx.fmt(d.unit, typical * f)
            ),
        ),
    };
    Some(InsightV {
        severity,
        title,
        detail,
    })
}

fn tree(cx: &Ctx<'_, '_>, data: &TargetData, selected: Option<&MetricDescriptor>) -> Vec<TreeRowV> {
    let req = cx.req;
    let f_cpu = cx.factor(builtin::CPU);
    let members = &data.members;
    let index: HashMap<u32, usize> = members
        .iter()
        .enumerate()
        .map(|(i, p)| (p.pid, i))
        .collect();
    let visible = |p: &ProcessSample| req.show_threads || !p.is_thread();

    // Order: hierarchy (DFS, busiest child first) or flat by the selected metric.
    let sort_metric = selected.map_or(builtin::CPU, |d| d.id);
    let mut order: Vec<(usize, i32)> = Vec::with_capacity(members.len());
    if req.tree_flat {
        let mut idx: Vec<usize> = (0..members.len())
            .filter(|&i| visible(&members[i]))
            .collect();
        idx.sort_by(|&a, &b| {
            members[b]
                .value(sort_metric)
                .total_cmp(&members[a].value(sort_metric))
        });
        order.extend(idx.into_iter().map(|i| (i, 0)));
    } else {
        let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut roots = Vec::new();
        for (i, p) in members.iter().enumerate() {
            match p.parent.and_then(|pp| index.get(&pp)) {
                Some(&parent) if parent != i => children.entry(parent).or_default().push(i),
                _ => roots.push(i),
            }
        }
        let by_cpu = |v: &mut Vec<usize>| {
            v.sort_by(|&a, &b| {
                members[b]
                    .value(builtin::CPU)
                    .total_cmp(&members[a].value(builtin::CPU))
            })
        };
        by_cpu(&mut roots);
        let mut stack: Vec<(usize, i32)> = roots.into_iter().rev().map(|i| (i, 0)).collect();
        while let Some((i, depth)) = stack.pop() {
            if visible(&members[i]) {
                order.push((i, depth));
            }
            if let Some(mut kids) = children.remove(&i) {
                by_cpu(&mut kids);
                stack.extend(kids.into_iter().rev().map(|k| (k, depth + 1)));
            }
        }
    }

    let max_cpu = order
        .iter()
        .map(|&(i, _)| members[i].value(builtin::CPU))
        .fold(f64::EPSILON, f64::max);
    let max_mem = order
        .iter()
        .map(|&(i, _)| members[i].value(builtin::MEM_RSS))
        .fold(f64::EPSILON, f64::max);
    let spark_metric = selected
        .filter(|d| d.per_process_history)
        .map_or(builtin::CPU, |d| d.id);
    let base = cx.base();

    order
        .into_iter()
        .map(|(i, depth)| {
            let p = &members[i];
            let rate = |m| format_value(Unit::BytesPerSec, p.value(m), base);
            let has_io = p.get(builtin::IO_READ).is_some();
            let spark = data
                .process_series(p.pid, spark_metric)
                .map(|s| {
                    let pts: Vec<Point> = s.since(cx.since()).collect();
                    chart::sparkline(&pts, req.window_secs, cx.view.now, 30)
                })
                .unwrap_or_default();
            TreeRowV {
                pid: p.pid.to_string(),
                name: p.name.clone(),
                detail: p.command.chars().take(160).collect(),
                depth,
                is_thread: p.is_thread(),
                cpu: format_percent(p.value(builtin::CPU) * f_cpu),
                cpu_frac: (p.value(builtin::CPU) / max_cpu) as f32,
                mem: p
                    .get(builtin::MEM_RSS)
                    .map_or_else(|| "—".into(), |v| format_bytes(v, base)),
                mem_frac: (p.value(builtin::MEM_RSS) / max_mem) as f32,
                threads: p
                    .get(builtin::THREADS)
                    .map_or_else(|| "—".into(), |v| format!("{v:.0}")),
                io: if has_io {
                    format!("{} / {}", rate(builtin::IO_READ), rate(builtin::IO_WRITE))
                } else {
                    "—".into()
                },
                gpu: gpu_text(cx, p.get(builtin::GPU_UTIL), p.get(builtin::GPU_MEM)),
                spark,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tvis_core::alert::AlertEngine;
    use tvis_core::engine::{EngineConfig, UpdateReason};
    use tvis_core::frame::SystemInfo;
    use tvis_core::metric::MetricRegistry;
    use tvis_core::source::{ProcessSource, SyntheticSource};
    use tvis_core::store::{Limits, Store};

    use super::*;

    struct Fixture {
        store: Store,
        registry: MetricRegistry,
        frame: Frame,
        config: EngineConfig,
        info: SystemInfo,
        alerts: AlertEngine,
        now: f64,
    }

    fn fixture(targets: &[&str]) -> Fixture {
        let mut src = SyntheticSource::new(120);
        let mut registry = MetricRegistry::new();
        registry.extend(src.descriptors());
        let mut store = Store::new(Limits {
            retention_secs: 3600.0,
            interval_secs: 1.0,
        });
        for t in targets {
            store.watch(Target::Name((*t).into()));
        }
        for f in src.backfill() {
            store.ingest(&f, &registry);
        }
        let mut frame = src.sample();
        frame.t = 1.0;
        store.ingest(&frame, &registry);
        Fixture {
            info: src.info(),
            store,
            registry,
            frame,
            config: EngineConfig {
                interval: Duration::from_secs(1),
                ..Default::default()
            },
            alerts: AlertEngine::default(),
            now: 1.0,
        }
    }

    fn state(fx: &Fixture, req: &ViewRequest) -> ViewState {
        let view = EngineView {
            reason: UpdateReason::Tick,
            store: &fx.store,
            frame: Some(&fx.frame),
            registry: &fx.registry,
            system: &fx.info,
            alerts: &fx.alerts,
            config: &fx.config,
            now: fx.now,
        };
        build(&view, req)
    }

    fn req(page: Page, selected: Option<&str>) -> ViewRequest {
        ViewRequest {
            page,
            selected: selected.map(|s| Target::Name(s.into()).id()),
            ..Default::default()
        }
    }

    #[test]
    fn dashboard_has_system_charts_and_tops() {
        let fx = fixture(&["nginx"]);
        let s = state(&fx, &req(Page::Dashboard, None));
        let d = s.dashboard.expect("dashboard");
        assert_eq!(d.cores.len(), 16);
        assert_eq!(d.top_cpu[0].name, "ffmpeg");
        assert!(d.top_cpu.iter().any(|r| r.name == "nginx" && r.watched));
        assert!(!d.cpu_chart.unwrap().lines[0].stroke.is_empty());
        assert!(d.system.mem.contains("GiB"));
        assert!(s.monitor.is_none() && s.processes.is_none());
    }

    #[test]
    fn monitor_reports_cards_stats_tree_and_leak() {
        let fx = fixture(&["api-server"]);
        let s = state(&fx, &req(Page::Monitor, Some("api-server")));
        let m = s.monitor.expect("monitor");
        assert!(m.running);
        assert!(m.cards.iter().any(|c| c.id == "mem.rss"));
        assert_eq!(m.cards.iter().filter(|c| c.selected).count(), 1);
        assert_eq!(m.stats.len(), 8);
        assert!(
            m.insights.iter().any(|i| i.title.contains("memory leak")),
            "{:?}",
            m.insights
        );
        // hierarchy: api-server first, workers nested below
        assert_eq!(m.tree[0].name, "api-server");
        assert_eq!(m.tree[0].depth, 0);
        assert!(m.tree[1..].iter().all(|r| r.depth == 1));
        assert_eq!(m.tree.len(), 4);
        assert!(s.watchlist[0].selected && s.watchlist[0].running);
    }

    #[test]
    fn chart_modes_change_line_counts() {
        let fx = fixture(&["api-server", "nginx"]);
        let mut r = req(Page::Monitor, Some("api-server"));
        let lines = |r: &ViewRequest| state(&fx, r).monitor.unwrap().chart.unwrap().lines.len();
        assert_eq!(lines(&r), 1);
        r.chart_view = ChartView::PerProcess;
        assert_eq!(lines(&r), 4);
        r.chart_view = ChartView::Compare;
        assert_eq!(lines(&r), 2);
    }

    #[test]
    fn flat_tree_sorts_by_metric_and_threads_toggle() {
        let fx = fixture(&["postgres"]);
        let mut r = req(Page::Monitor, Some("postgres"));
        r.tree_flat = true;
        let m = state(&fx, &r).monitor.unwrap();
        assert!(m.tree.iter().all(|t| t.depth == 0));
        assert_eq!(m.tree.len(), 5);
        let cpu: Vec<f32> = m.tree.iter().map(|t| t.cpu_frac).collect();
        assert!(cpu.windows(2).all(|w| w[0] >= w[1]));
    }

    #[test]
    fn processes_filter_sort_and_group() {
        let fx = fixture(&[]);
        let mut r = req(Page::Processes, None);
        let rows = |r: &ViewRequest| state(&fx, r).processes.unwrap().rows;
        let grouped = rows(&r);
        assert_eq!(grouped.iter().filter(|p| p.name == "postgres").count(), 1);
        assert_eq!(grouped[0].name, "ffmpeg");
        r.proc_group = false;
        r.proc_filter = "postgres".into();
        assert_eq!(rows(&r).len(), 5);
        r.proc_sort = ProcSort::Name;
        r.proc_ascending = true;
        r.proc_filter = "n".into();
        let names: Vec<String> = rows(&r)
            .into_iter()
            .map(|p| p.name.to_lowercase())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn dashboard_lists_gpus_with_history_and_top_users() {
        let fx = fixture(&["ffmpeg"]);
        let s = state(&fx, &req(Page::Dashboard, None));
        assert!(s.has_gpu);
        let d = s.dashboard.expect("dashboard");
        assert_eq!(d.gpus.len(), 2);
        assert_eq!(d.gpus[0].name, "Demo RTX 4080");
        assert_eq!(d.gpus[0].vendor, "NVIDIA");
        assert!(d.gpus[0].mem.contains(" / 16.0 GiB"), "{}", d.gpus[0].mem);
        assert!(d.gpus[0].mem_frac > 0.0 && d.gpus[0].mem_frac < 1.0);
        assert!(!d.gpus[0].spark.is_empty(), "history feeds the sparkline");
        let chart = d.gpu_chart.expect("gpu chart");
        assert_eq!(chart.lines.len(), 2);
        assert_eq!(chart.y_ticks.last().unwrap().label, "100%");
        assert_eq!(d.top_gpu[0].name, "ffmpeg", "most VRAM first");
        assert!(d.top_gpu.iter().any(|r| r.name == "ffmpeg" && r.watched));
        assert!(d.top_gpu.iter().any(|r| r.name == "api-server" && !r.watched));
        assert!(d.top_gpu[0].value.contains('%') && d.top_gpu[0].value.contains("GiB"));
    }

    #[test]
    fn monitor_shows_gpu_cards_tree_column_and_vram_growth() {
        let fx = fixture(&["api-server"]);
        let m = state(&fx, &req(Page::Monitor, Some("api-server")))
            .monitor
            .unwrap();
        assert!(m.cards.iter().any(|c| c.id == "gpu.util"));
        assert!(m.cards.iter().any(|c| c.id == "gpu.mem"));
        assert!(m.tree[0].gpu.contains("GiB"), "{}", m.tree[0].gpu);
        assert!(
            m.insights.iter().any(|i| i.title.contains("VRAM leak")),
            "{:?}",
            m.insights
        );
    }

    #[test]
    fn processes_page_sorts_by_gpu() {
        let fx = fixture(&[]);
        let mut r = req(Page::Processes, None);
        r.proc_sort = ProcSort::Gpu;
        let rows = state(&fx, &r).processes.unwrap().rows;
        assert_eq!(rows[0].name, "ffmpeg");
        assert_eq!(rows[1].name, "api-server");
        assert!(rows[0].gpu.contains("GiB"));
        let last = rows.last().unwrap();
        assert_eq!(last.gpu, "—");
    }

    #[test]
    fn machines_without_gpu_show_no_gpu_section() {
        let mut fx = fixture(&[]);
        fx.frame.gpus.clear();
        let s = state(&fx, &req(Page::Dashboard, None));
        assert!(!s.has_gpu);
        let d = s.dashboard.unwrap();
        assert!(d.gpus.is_empty() && d.gpu_chart.is_none() && d.top_gpu.is_empty());
    }

    #[test]
    fn waiting_target_has_no_data_but_a_row() {
        let mut fx = fixture(&["does-not-exist"]);
        fx.frame.t = 2.0;
        fx.store.ingest(&fx.frame, &fx.registry);
        let s = state(&fx, &req(Page::Monitor, Some("does-not-exist")));
        assert!(!s.watchlist[0].running);
        let m = s.monitor.unwrap();
        assert!(!m.running && m.cards.is_empty() && m.chart.is_none() && m.tree.is_empty());
    }

    #[test]
    fn cpu_normalisation_scales_display() {
        let fx = fixture(&["ffmpeg"]);
        let mut r = req(Page::Monitor, Some("ffmpeg"));
        let raw = state(&fx, &r).watchlist[0].cpu.clone();
        r.cpu_normalized = true;
        let norm = state(&fx, &r).watchlist[0].cpu.clone();
        let num = |s: &str| s.trim_end_matches('%').parse::<f64>().unwrap();
        assert!((num(&raw) / 16.0 - num(&norm)).abs() < 0.2, "{raw} {norm}");
    }
}
