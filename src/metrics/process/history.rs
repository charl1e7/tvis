use std::collections::HashMap;
use sysinfo::Pid;

use super::circular_buffer::CircularBuffer;

/// Stores historical data for processes and their children
#[derive(Default, Debug, Clone)]
pub struct ProcessHistory {
    /// Groups process metrics with its children
    histories: HashMap<Pid, ProcessMetrics>,
    /// Maximum number of data points to store in history
    pub history_len: usize,
}

/// Stores CPU and memory metrics for a process
#[derive(Debug, Clone)]
pub struct ProcessMetrics {
    cpu: CircularBuffer<f32>,
    memory: CircularBuffer<usize>,
}

impl ProcessMetrics {
    fn new(size: usize) -> Self {
        Self {
            cpu: CircularBuffer::new(size),
            memory: CircularBuffer::new(size),
        }
    }

    fn update_cpu(&mut self, value: f32) {
        self.cpu.push(value);
    }

    fn update_memory(&mut self, value: usize) {
        self.memory.push(value);
    }

    pub fn get_cpu_history(&self) -> Vec<f32> {
        self.cpu.as_vec()
    }

    pub fn get_memory_history(&self) -> Vec<usize> {
        self.memory.as_vec()
    }
}

impl ProcessHistory {
    pub fn new(max_points: usize) -> Self {
        Self {
            histories: HashMap::new(),
            history_len: max_points,
        }
    }

    pub fn update_cpu(&mut self, pid: Pid, cpu_usage: f32) {
        self.histories
            .entry(pid)
            .or_insert_with(|| ProcessMetrics::new(self.history_len))
            .update_cpu(cpu_usage);
    }

    pub fn update_memory(&mut self, pid: Pid, memory: usize) {
        self.histories
            .entry(pid)
            .or_insert_with(|| ProcessMetrics::new(self.history_len))
            .update_memory(memory);
    }

    pub fn get_cpu_history(&self, pid: &Pid) -> Option<Vec<f32>> {
        self.histories
            .get(pid)
            .map(|metrics| metrics.get_cpu_history())
    }

    pub fn get_memory_history(&self, pid: &Pid) -> Option<Vec<usize>> {
        self.histories
            .get(pid)
            .map(|metrics| metrics.get_memory_history())
    }

    pub fn get_data_history(&self, pid: &Pid) -> (f32, usize, f32, usize) {
        if let (Some(cpu_history), Some(mem_history)) =
            (self.get_cpu_history(pid), self.get_memory_history(pid))
        {
            let mut max_cpu = 0.0;
            let mut max_memory = 0_usize;
            let mut sum_cpu = 0.0;
            let mut sum_memory = 0_usize;
            let len = cpu_history.len();

            for i in 0..len {
                let cpu_val = cpu_history[i];
                let mem_val = mem_history[i];
                max_cpu = f32::max(max_cpu, cpu_val);
                max_memory = usize::max(max_memory, mem_val);
                sum_cpu += cpu_val;
                sum_memory += mem_val;
            }

            let avg_cpu = if len == 0 { 0.0 } else { sum_cpu / len as f32 };
            let avg_memory = if len == 0 {
                0
            } else {
                sum_memory / len
            };

            (max_cpu, max_memory, avg_cpu, avg_memory)
        } else {
            (0.0, 0, 0.0, 0)
        }
    }

    pub fn cleanup_histories(&mut self, active_pids: &[Pid]) {
        self.histories.retain(|pid, _| active_pids.contains(pid));
    }
}
