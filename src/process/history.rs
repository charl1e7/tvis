use std::collections::HashMap;
use sysinfo::Pid;

/// Stores historical data for processes and their children
#[derive(Default)]
pub struct ProcessHistory {
    /// for each monitored process
    histories: Vec<ProcessMetrics>,
    /// for child processes
    child_histories: HashMap<Pid, ProcessMetrics>,
    /// Maximum number of data points to store in history
    pub history_max_points: usize,
}

/// Stores CPU and memory metrics for a process
struct ProcessMetrics {
    cpu: CircularBuffer,
    memory: CircularBuffer,
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

    fn update_memory(&mut self, value: f32) {
        self.memory.push(value);
    }

    fn get_cpu_history(&self) -> Vec<f32> {
        self.cpu.as_slice()
    }

    fn get_memory_history(&self) -> Vec<f32> {
        self.memory.as_slice()
    }
}

/// A fixed-size circular buffer for storing historical data
#[derive(Default)]
pub struct CircularBuffer {
    data: Vec<f32>,
    position: usize,
    peak_value: f32,
    sum: f32,  // Track sum for efficient average calculation
    len: usize, // Track actual number of values
}

impl CircularBuffer {
    /// Creates a new circular buffer with the specified size
    fn new(size: usize) -> Self {
        Self {
            data: vec![0.0; size],
            position: 0,
            peak_value: 0.0,
            sum: 0.0,
            len: 0,
        }
    }

    /// Adds a new value to the buffer
    fn push(&mut self, value: f32) {
        if self.len == self.data.len() {
            self.sum -= self.data[self.position];
        } else {
            self.len += 1;
        }
        self.sum += value;

        self.data[self.position] = value;
        self.position = (self.position + 1) % self.data.len();
        
        if value > self.peak_value {
            self.peak_value = value;
        } else if self.peak_value == self.data[self.position] {
            self.peak_value = self.data.iter().copied().fold(0.0, f32::max);
        }
    }

    /// Returns two slices representing the data in chronological order
    fn as_slices(&self) -> (&[f32], &[f32]) {
        let (first, second) = self.data.split_at(self.position);
        (second, first)
    }

    /// Returns a slice of the buffer's data in chronological order
    /// with newest values at the end
    fn as_slice(&self) -> Vec<f32> {
        let (first, second) = self.as_slices();
        let mut result = Vec::with_capacity(self.len);
        result.extend_from_slice(first);
        result.extend_from_slice(second);
        result
    }

    /// Returns the maximum value in the buffer
    fn max_value(&self) -> f32 {
        self.peak_value
    }

    /// Returns the last value in the buffer
    fn last_value(&self) -> f32 {
        if self.position == 0 {
            self.data[self.data.len() - 1]
        } else {
            self.data[self.position - 1]
        }
    }

    fn average(&self) -> f32 {
        if self.len == 0 {
            0.0
        } else {
            self.sum / self.len as f32
        }
    }
}

impl ProcessHistory {
    pub fn new(max_points: usize) -> Self {
        Self {
            histories: Vec::new(),
            child_histories: HashMap::new(),
            history_max_points: max_points,
        }
    }

    fn ensure_process_exists(&mut self, process_idx: usize) {
        if process_idx >= self.histories.len() {
            self.histories.resize_with(process_idx + 1, || ProcessMetrics::new(self.history_max_points));
        }
    }

    pub fn update_process_cpu(&mut self, process_idx: usize, cpu_usage: f32) {
        self.ensure_process_exists(process_idx);
        self.histories[process_idx].update_cpu(cpu_usage);
    }

    pub fn update_memory(&mut self, process_idx: usize, memory_mb: f32) {
        self.ensure_process_exists(process_idx);
        self.histories[process_idx].update_memory(memory_mb);
    }

    pub fn update_child_cpu(&mut self, pid: Pid, cpu_usage: f32) {
        self.child_histories
            .entry(pid)
            .or_insert_with(|| ProcessMetrics::new(self.history_max_points))
            .update_cpu(cpu_usage);
    }

    pub fn update_child_memory(&mut self, pid: Pid, memory_mb: f32) {
        self.child_histories
            .entry(pid)
            .or_insert_with(|| ProcessMetrics::new(self.history_max_points))
            .update_memory(memory_mb);
    }

    pub fn get_process_cpu_history(&self, idx: usize) -> Option<Vec<f32>> {
        self.histories.get(idx).map(|h| h.get_cpu_history())
    }

    pub fn get_child_cpu_history(&self, pid: &Pid) -> Option<Vec<f32>> {
        self.child_histories.get(pid).map(|h| h.get_cpu_history())
    }

    pub fn get_memory_history(&self, idx: usize) -> Option<Vec<f32>> {
        self.histories.get(idx).map(|h| h.get_memory_history())
    }

    pub fn get_child_memory_history(&self, pid: &Pid) -> Option<Vec<f32>> {
        self.child_histories.get(pid).map(|h| h.get_memory_history())
    }

    pub fn get_last_cpu(&self, idx: usize) -> Option<f32> {
        self.histories.get(idx).map(|h| h.cpu.last_value())
    }

    pub fn get_last_memory(&self, idx: usize) -> Option<f32> {
        self.histories.get(idx).map(|h| h.memory.last_value())
    }

    pub fn get_peak_cpu(&self, idx: usize) -> Option<f32> {
        self.histories.get(idx).map(|h| h.cpu.max_value())
    }

    pub fn get_peak_memory(&self, idx: usize) -> Option<f32> {
        self.histories.get(idx).map(|h| h.memory.max_value())
    }

    pub fn cleanup_child_histories(&mut self, active_pids: &[Pid]) {
        self.child_histories.retain(|pid, _| active_pids.contains(pid));
    }

    pub fn remove_process(&mut self, idx: usize) {
        if idx < self.histories.len() {
            self.histories.remove(idx);
        }
    }

    pub fn clear_process(&mut self, idx: usize) {
        if idx < self.histories.len() {
            // Clear main process data
            self.histories[idx] = ProcessMetrics::new(self.history_max_points);
            
            // Clear child processes data
            self.child_histories.clear();
        }
    }
} 