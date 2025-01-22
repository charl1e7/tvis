mod monitor;
mod history;

pub use monitor::*;
pub use history::*;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub name: String,
    pub pid: sysinfo::Pid,
    pub cpu_usage: f32,
    pub memory_mb: f32,
}

#[derive(Debug)]
pub struct ProcessStats {
    pub current_cpu: f32,
    pub avg_cpu: f32,
    pub memory_mb: f32,
    pub child_processes: Vec<ProcessInfo>,
    pub children_avg_cpu: f32,
    pub children_current_cpu: f32,
    pub children_memory_mb: f32,
} 