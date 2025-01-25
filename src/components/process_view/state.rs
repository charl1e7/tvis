use crate::metrics::process::{MetricType, ProcessHistory, ProcessStats, SortType};
use sysinfo::Pid;

pub struct ProcessView<'a> {
    pub sort_type: SortType,
    pub current_metric: &'a mut MetricType,
    pub scroll_target: &'a mut Option<Pid>,
}
