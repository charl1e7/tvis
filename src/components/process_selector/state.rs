use std::sync::Arc;

use egui::mutex::RwLock;

use crate::metrics::Metrics;

#[derive(Default)]
pub struct ProcessSelector {
    pub show: bool,
    pub search: String,
    pub search_by_pid: bool,
}
