mod history;
mod monitor;

pub use history::*;
pub use monitor::*;

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessIdentifier {
    Name(String),
    Pid(sysinfo::Pid),
}

impl From<&str> for ProcessIdentifier {
    fn from(s: &str) -> Self {
        if s.starts_with("pid:") {
            if let Ok(pid) = s[4..].parse::<usize>() {
                return ProcessIdentifier::Pid(sysinfo::Pid::from(pid));
            }
        }
        ProcessIdentifier::Name(s.to_string())
    }
}

impl ToString for ProcessIdentifier {
    fn to_string(&self) -> String {
        match self {
            ProcessIdentifier::Name(name) => name.clone(),
            ProcessIdentifier::Pid(pid) => format!("pid:{}", pid),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub name: String,
    pub pid: sysinfo::Pid,
    pub parent_pid: Option<sysinfo::Pid>,
    pub cpu_usage: f32,
    pub memory_mb: f32,
    pub is_thread: bool,
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
    pub thread_count: usize,
}
