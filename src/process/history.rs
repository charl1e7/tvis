use std::collections::HashMap;
use sysinfo::Pid;

/// Stores historical data for processes and their children
#[derive(Default)]
pub struct ProcessHistory {
    /// CPU and memory history for each monitored process
    histories: Vec<ProcessMetrics>,
    /// CPU and memory history for child processes
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
}

/// A fixed-size circular buffer for storing historical data
#[derive(Default)]
pub struct CircularBuffer {
    data: Vec<f32>,
    position: usize,
    peak_value: f32,
}

impl CircularBuffer {
    /// Creates a new circular buffer with the specified size
    fn new(size: usize) -> Self {
        Self {
            data: vec![0.0; size],
            position: 0,
            peak_value: 0.0,
        }
    }

    /// Adds a new value to the buffer
    fn push(&mut self, value: f32) {
        self.data[self.position] = value;
        self.position = (self.position + 1) % self.data.len();
        self.peak_value = self.peak_value.max(value);
        
        // Recalculate peak if we overwrote the previous peak
        if self.peak_value == self.data[self.position] {
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
        let mut result = Vec::with_capacity(self.data.len());
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
}

impl ProcessHistory {
    /// Creates a new ProcessHistory with the specified maximum history size
    pub fn new(max_points: usize) -> Self {
        Self {
            histories: Vec::new(),
            child_histories: HashMap::new(),
            history_max_points: max_points,
        }
    }

    /// Updates CPU usage history for a monitored process
    pub fn update_process_cpu(&mut self, process_idx: usize, cpu_usage: f32) {
        if process_idx >= self.histories.len() {
            self.histories.resize_with(process_idx + 1, || ProcessMetrics::new(self.history_max_points));
        }
        self.histories[process_idx].cpu.push(cpu_usage);
    }

    /// Updates memory usage history for a monitored process
    pub fn update_memory(&mut self, process_idx: usize, memory_mb: f32) {
        if process_idx >= self.histories.len() {
            self.histories.resize_with(process_idx + 1, || ProcessMetrics::new(self.history_max_points));
        }
        self.histories[process_idx].memory.push(memory_mb);
    }

    /// Updates CPU usage history for a child process
    pub fn update_child_cpu(&mut self, pid: Pid, cpu_usage: f32) {
        self.child_histories
            .entry(pid)
            .or_insert_with(|| ProcessMetrics::new(self.history_max_points))
            .cpu.push(cpu_usage);
    }

    /// Updates memory usage history for a child process
    pub fn update_child_memory(&mut self, pid: Pid, memory_mb: f32) {
        self.child_histories
            .entry(pid)
            .or_insert_with(|| ProcessMetrics::new(self.history_max_points))
            .memory.push(memory_mb);
    }

    /// Gets CPU usage history for a monitored process
    pub fn get_process_cpu_history(&self, idx: usize) -> Option<Vec<f32>> {
        self.histories.get(idx).map(|h| h.cpu.as_slice())
    }

    /// Gets CPU usage history for a child process
    pub fn get_child_cpu_history(&self, pid: &Pid) -> Option<Vec<f32>> {
        self.child_histories.get(pid).map(|h| h.cpu.as_slice())
    }

    /// Gets memory usage history for a monitored process
    pub fn get_memory_history(&self, idx: usize) -> Option<Vec<f32>> {
        self.histories.get(idx).map(|h| h.memory.as_slice())
    }

    /// Gets memory usage history for a child process
    pub fn get_child_memory_history(&self, pid: &Pid) -> Option<Vec<f32>> {
        self.child_histories.get(pid).map(|h| h.memory.as_slice())
    }

    /// Gets the last CPU value for a monitored process
    pub fn get_last_cpu(&self, idx: usize) -> Option<f32> {
        self.histories.get(idx).map(|h| h.cpu.last_value())
    }

    /// Gets the last memory value for a monitored process
    pub fn get_last_memory(&self, idx: usize) -> Option<f32> {
        self.histories.get(idx).map(|h| h.memory.last_value())
    }

    /// Gets the peak CPU value for a monitored process
    pub fn get_peak_cpu(&self, idx: usize) -> Option<f32> {
        self.histories.get(idx).map(|h| h.cpu.max_value())
    }

    /// Gets the peak memory value for a monitored process
    pub fn get_peak_memory(&self, idx: usize) -> Option<f32> {
        self.histories.get(idx).map(|h| h.memory.max_value())
    }

    /// Removes history entries for child processes that are no longer active
    pub fn cleanup_child_histories(&mut self, active_pids: &[Pid]) {
        self.child_histories.retain(|pid, _| active_pids.contains(pid));
    }

    /// Removes history for a monitored process
    pub fn remove_process(&mut self, idx: usize) {
        if idx < self.histories.len() {
            self.histories.remove(idx);
        }
    }
} 