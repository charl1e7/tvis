use crate::metrics::process::{
    MetricType, ProcessHistory, ProcessIdentifier, ProcessStats, SortType,
};
use sysinfo::Pid;

#[derive(serde::Deserialize, serde::Serialize, Debug, Default)]
pub struct ProcessView {
    pub sort_type: SortType,
    pub current_metric: MetricType,
    pub scroll_target: Option<ProcessIdentifier>,
}
