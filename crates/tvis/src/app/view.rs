//! Plain-data description of what the UI should show.
//!
//! These types are `Send` and know nothing about Slint: the presenter builds
//! them on the engine thread and the UI thread merely copies them into models.

use std::sync::{Arc, Mutex};

use tvis_core::metric::Unit;
use tvis_core::units::ByteBase;
use tvis_core::TargetId;

/// Series colours; keep in sync with `Theme.series` in `ui/theme.slint`.
pub const SERIES_COLORS: [u32; 8] = [
    0x6d7cff, 0x22d3ee, 0x34d399, 0xfbbf24, 0xfb7185, 0xa78bfa, 0x38bdf8, 0xfb923c,
];

pub const ACCENTS: [u32; 6] = [0x6d7cff, 0x22d3ee, 0x34d399, 0xfbbf24, 0xfb7185, 0xa78bfa];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Dashboard,
    Monitor,
    Processes,
    Alerts,
    Settings,
}

impl Page {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Monitor,
            2 => Self::Processes,
            3 => Self::Alerts,
            4 => Self::Settings,
            _ => Self::Dashboard,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChartView {
    Aggregate,
    PerProcess,
    Compare,
}

impl ChartView {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::PerProcess,
            2 => Self::Compare,
            _ => Self::Aggregate,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcSort {
    Name,
    Cpu,
    Memory,
    Threads,
    Pid,
}

impl ProcSort {
    pub fn from_index(i: i32) -> Self {
        match i {
            0 => Self::Name,
            2 => Self::Memory,
            3 => Self::Threads,
            4 => Self::Pid,
            _ => Self::Cpu,
        }
    }

    pub fn index(self) -> i32 {
        match self {
            Self::Name => 0,
            Self::Cpu => 1,
            Self::Memory => 2,
            Self::Threads => 3,
            Self::Pid => 4,
        }
    }
}

/// What the UI currently needs; the presenter only computes that.
#[derive(Clone, Debug)]
pub struct ViewRequest {
    pub page: Page,
    pub selected: Option<TargetId>,
    pub metric: String,
    pub window_secs: f64,
    pub chart_view: ChartView,
    pub tree_flat: bool,
    pub show_threads: bool,
    pub proc_filter: String,
    pub proc_group: bool,
    pub proc_sort: ProcSort,
    pub proc_ascending: bool,
    /// Exponential smoothing weight in `[0, 1)`.
    pub smoothing: f64,
    pub byte_base: ByteBase,
    pub cpu_normalized: bool,
    /// Plot width in device pixels; bounds the points per line.
    pub chart_px: usize,
}

impl Default for ViewRequest {
    fn default() -> Self {
        Self {
            page: Page::Dashboard,
            selected: None,
            metric: "cpu".into(),
            window_secs: 300.0,
            chart_view: ChartView::Aggregate,
            tree_flat: false,
            show_threads: false,
            proc_filter: String::new(),
            proc_group: true,
            proc_sort: ProcSort::Cpu,
            proc_ascending: false,
            smoothing: 0.0,
            byte_base: ByteBase::Binary,
            cpu_normalized: false,
            chart_px: 900,
        }
    }
}

pub type SharedRequest = Arc<Mutex<ViewRequest>>;

#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    pub frac: f32,
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct LineV {
    pub name: String,
    pub color: u32,
    pub stroke: String,
    pub fill: String,
    pub last: String,
    /// Decimated `(x fraction, value)` points used to answer hover queries.
    pub points: Vec<(f32, f64)>,
}

#[derive(Clone, Debug)]
pub struct ChartV {
    pub lines: Vec<LineV>,
    pub y_ticks: Vec<Tick>,
    pub x_ticks: Vec<Tick>,
    pub unit: Unit,
    pub base: ByteBase,
    pub window_secs: f64,
}

impl ChartV {
    pub fn empty() -> Self {
        Self {
            lines: Vec::new(),
            y_ticks: Vec::new(),
            x_ticks: Vec::new(),
            unit: Unit::Count,
            base: ByteBase::Binary,
            window_secs: 60.0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct WatchRowV {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub running: bool,
    pub cpu: String,
    pub mem: String,
    pub spark: String,
    pub selected: bool,
    pub alerting: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CardV {
    pub id: String,
    pub name: String,
    pub value: String,
    pub detail: String,
    pub trend: String,
    pub trend_dir: i32,
    pub spark: String,
    pub color: u32,
    pub selected: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TreeRowV {
    pub pid: String,
    pub name: String,
    pub detail: String,
    pub depth: i32,
    pub is_thread: bool,
    pub cpu: String,
    pub cpu_frac: f32,
    pub mem: String,
    pub mem_frac: f32,
    pub threads: String,
    pub io: String,
    pub spark: String,
}

#[derive(Clone, Debug, Default)]
pub struct InsightV {
    pub severity: i32,
    pub title: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default)]
pub struct MonitorV {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub running: bool,
    pub chart_title: String,
    pub cards: Vec<CardV>,
    pub chart: Option<ChartV>,
    pub stats: Vec<(String, String)>,
    pub insights: Vec<InsightV>,
    pub tree: Vec<TreeRowV>,
}

#[derive(Clone, Debug, Default)]
pub struct SystemV {
    pub cpu: String,
    pub cpu_frac: f32,
    pub cpu_spark: String,
    pub mem: String,
    pub mem_detail: String,
    pub mem_frac: f32,
    pub mem_spark: String,
    pub swap: String,
    pub swap_detail: String,
    pub swap_frac: f32,
    pub load: String,
    pub load_detail: String,
    pub uptime: String,
    pub processes: String,
    pub cpu_model: String,
}

#[derive(Clone, Debug, Default)]
pub struct TopRowV {
    pub name: String,
    pub value: String,
    pub frac: f32,
    pub count: String,
    pub key: String,
    pub watched: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DashboardV {
    pub system: SystemV,
    pub cores: Vec<(String, f32, String)>,
    pub cpu_chart: Option<ChartV>,
    pub mem_chart: Option<ChartV>,
    pub top_cpu: Vec<TopRowV>,
    pub top_mem: Vec<TopRowV>,
}

#[derive(Clone, Debug, Default)]
pub struct ProcRowV {
    pub key: String,
    pub name: String,
    pub pid: String,
    pub cpu: String,
    pub cpu_frac: f32,
    pub mem: String,
    pub mem_frac: f32,
    pub threads: String,
    pub watched: bool,
    pub is_pid: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ProcessesV {
    pub rows: Vec<ProcRowV>,
    pub summary: String,
}

/// Everything one engine update contributes to the screen.
#[derive(Clone, Debug, Default)]
pub struct ViewState {
    pub sampling_label: String,
    pub host: String,
    pub live: bool,
    pub paused: bool,
    pub watchlist: Vec<WatchRowV>,
    /// `(id, title)` of every watched target, for pickers.
    pub targets: Vec<(String, String)>,
    pub firing_rules: Vec<u64>,
    pub dashboard: Option<DashboardV>,
    pub monitor: Option<MonitorV>,
    pub processes: Option<ProcessesV>,
}
