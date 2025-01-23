use crate::process::{ProcessStats, ProcessHistory, SortType, MetricType};
use sysinfo::Pid;

pub struct ProcessView<'a> {
    pub stats: ProcessStats,
    pub history: &'a ProcessHistory,
    pub process_idx: usize,
    pub sort_type: SortType,
    pub current_metric: MetricType,
    pub scroll_target: &'a mut Option<Pid>,
} 