use crate::process::{ProcessStats, ProcessHistory, SortType};

pub struct ProcessView {
    pub stats: ProcessStats,
    pub history: ProcessHistory,
    pub process_idx: usize,
    pub sort_type: SortType,
} 