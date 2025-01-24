use crate::process::{MetricType, ProcessHistory, ProcessStats, SortType};
use sysinfo::Pid;

pub struct ProcessView<'a> {
    pub stats: ProcessStats,
    pub history: &'a ProcessHistory,
    pub process_idx: usize,
    pub sort_type: SortType,
    pub current_metric: &'a mut MetricType,
    pub scroll_target: &'a mut Option<Pid>,
}
