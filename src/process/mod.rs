mod monitor;
mod history;

pub use monitor::*;
pub use history::*;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub name: String,
    pub pid: sysinfo::Pid,
    pub parent_pid: Option<sysinfo::Pid>,
    pub cpu_usage: f32,
    pub memory_mb: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum SortType {
    AvgCpu,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum MetricType {
    Cpu,
    Memory,
}

impl Default for MetricType {
    fn default() -> Self {
        Self::Cpu
    }
}

impl Default for SortType {
    fn default() -> Self {
        Self::AvgCpu
    }
}

#[derive(Debug)]
pub struct ProcessStats {
    pub current_cpu: f32,
    pub avg_cpu: f32,
    pub peak_cpu: f32,
    pub memory_mb: f32,
    pub peak_memory_mb: f32,
    pub processes: Vec<ProcessInfo>,
} 