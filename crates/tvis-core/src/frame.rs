//! A [`Frame`] is one immutable snapshot of the machine taken by a source.

use std::collections::HashMap;

use crate::metric::MetricId;

pub type Pid = u32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcessKind {
    Process,
    /// Kernel-scheduled thread (Linux). Its cost is already part of its
    /// owner, so threads are never double counted in aggregates.
    Thread,
}

/// One process (or thread) at one instant.
#[derive(Clone, Debug)]
pub struct ProcessSample {
    pub pid: Pid,
    pub parent: Option<Pid>,
    pub name: String,
    pub command: String,
    pub kind: ProcessKind,
    /// Seconds since the unix epoch at which the process started.
    pub start_time: u64,
    values: Vec<(MetricId, f64)>,
}

impl ProcessSample {
    pub fn new(pid: Pid, parent: Option<Pid>, name: impl Into<String>) -> Self {
        Self {
            pid,
            parent,
            name: name.into(),
            command: String::new(),
            kind: ProcessKind::Process,
            start_time: 0,
            values: Vec::with_capacity(8),
        }
    }

    pub fn is_thread(&self) -> bool {
        self.kind == ProcessKind::Thread
    }

    /// Sets a metric value, overwriting an earlier value of the same metric.
    pub fn set(&mut self, id: MetricId, value: f64) {
        match self.values.iter_mut().find(|(m, _)| *m == id) {
            Some(slot) => slot.1 = value,
            None => self.values.push((id, value)),
        }
    }

    pub fn get(&self, id: MetricId) -> Option<f64> {
        self.values.iter().find(|(m, _)| *m == id).map(|(_, v)| *v)
    }

    pub fn value(&self, id: MetricId) -> f64 {
        self.get(id).unwrap_or(0.0)
    }

    pub fn values(&self) -> &[(MetricId, f64)] {
        &self.values
    }
}

/// Static facts about the host.
#[derive(Clone, Debug, Default)]
pub struct SystemInfo {
    pub host_name: String,
    pub os: String,
    pub kernel: String,
    pub cpu_brand: String,
    pub cpu_cores: usize,
    pub physical_cores: usize,
}

/// Machine-wide gauges at one instant.
#[derive(Clone, Debug, Default)]
pub struct SystemSample {
    pub cpu_total: f64,
    pub cpu_per_core: Vec<f64>,
    pub mem_total: u64,
    pub mem_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub load_average: [f64; 3],
    pub uptime_secs: u64,
    pub process_count: usize,
}

/// Everything a source observed at one instant.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    /// Seconds since the engine started.
    pub t: f64,
    /// Milliseconds since the unix epoch.
    pub wall_ms: u64,
    pub system: SystemSample,
    pub processes: Vec<ProcessSample>,
    /// Graphics processors, filled by GPU probes (empty when there are none).
    pub gpus: Vec<crate::gpu::GpuSample>,
    index: HashMap<Pid, usize>,
}

impl Frame {
    pub fn new(system: SystemSample, processes: Vec<ProcessSample>) -> Self {
        let mut frame = Self {
            system,
            processes,
            ..Self::default()
        };
        frame.reindex();
        frame
    }

    fn reindex(&mut self) {
        self.index = self
            .processes
            .iter()
            .enumerate()
            .map(|(i, p)| (p.pid, i))
            .collect();
    }

    pub fn process(&self, pid: Pid) -> Option<&ProcessSample> {
        self.index.get(&pid).map(|&i| &self.processes[i])
    }

    pub fn process_mut(&mut self, pid: Pid) -> Option<&mut ProcessSample> {
        let i = *self.index.get(&pid)?;
        Some(&mut self.processes[i])
    }

    pub fn children_of(&self) -> HashMap<Pid, Vec<Pid>> {
        let mut map: HashMap<Pid, Vec<Pid>> = HashMap::new();
        for p in &self.processes {
            if let Some(parent) = p.parent {
                map.entry(parent).or_default().push(p.pid);
            }
        }
        map
    }
}
