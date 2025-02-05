mod circular_buffer;
mod history;
mod monitor;
pub use history::*;
pub use monitor::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct ProcessData {
    pub history: ProcessHistory,
    pub genereal: ProcessGeneral,
    pub processes_stats: Vec<ProcessInfo>,
}

#[derive(Debug, Clone, PartialEq, Hash, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProcessIdentifier {
    Name(String),
    #[serde(serialize_with = "serialize_pid", deserialize_with = "deserialize_pid")]
    Pid(sysinfo::Pid),
}

impl ProcessIdentifier {
    pub fn to_pid(&self) -> Option<sysinfo::Pid> {
        match self {
            ProcessIdentifier::Pid(pid) => Some(*pid),
            ProcessIdentifier::Name(_) => None,
        }
    }
}

fn serialize_pid<S>(pid: &sysinfo::Pid, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_u32(pid.as_u32())
}

fn deserialize_pid<'de, D>(deserializer: D) -> Result<sysinfo::Pid, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let pid = u32::deserialize(deserializer)?;
    Ok(sysinfo::Pid::from(pid as usize))
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
    pub is_thread: bool,
    pub current_cpu: f32,
    pub avg_cpu: f32,
    pub peak_cpu: f32,
    pub current_memory: usize,
    pub peak_memory: usize,
    pub avg_memory: usize,
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

#[derive(Debug, Clone, Default)]
pub struct ProcessGeneral {
    pub stats: ProcessGeneralStats,
    pub history: ProcessHistory,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessGeneralStats {
    pub current_cpu: f32,
    pub peak_cpu: f32,
    pub avg_cpu: f32,
    pub current_memory: usize,
    pub peak_memory: usize,
    pub avg_memory: usize,
    pub process_count: usize,
    pub thread_count: usize,
}
