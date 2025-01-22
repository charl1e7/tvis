use std::collections::HashMap;
use sysinfo::Pid;

/// Stores historical data for processes and their children
#[derive(Default)]
pub struct ProcessHistory {
    /// CPU usage history for each monitored process
    pub cpu_history: Vec<CircularBuffer>,
    /// Memory usage history for each monitored process
    pub memory_history: Vec<CircularBuffer>,
    /// CPU usage history for child processes, indexed by PID
    pub child_cpu_history: HashMap<Pid, CircularBuffer>,
    /// Memory usage history for child processes, indexed by PID
    pub child_memory_history: HashMap<Pid, CircularBuffer>,
    /// Maximum number of data points to store in history
    pub history_max_points: usize,
}

/// A fixed-size circular buffer for storing historical data
#[derive(Default)]
pub struct CircularBuffer {
    data: Vec<f32>,
    position: usize,
}

impl CircularBuffer {
    /// Creates a new circular buffer with the specified size
    fn new(size: usize) -> Self {
        Self {
            data: vec![0.0; size],
            position: 0,
        }
    }

    /// Adds a new value to the buffer
    fn push(&mut self, value: f32) {
        self.data[self.position] = value;
        self.position = (self.position + 1) % self.data.len();
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
        self.data.iter().copied().fold(0.0, f32::max)
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
            cpu_history: Vec::new(),
            memory_history: Vec::new(),
            child_cpu_history: HashMap::new(),
            child_memory_history: HashMap::new(),
            history_max_points: max_points,
        }
    }

    /// Updates CPU usage history for a monitored process
    pub fn update_process_cpu(&mut self, process_idx: usize, cpu_usage: f32) {
        if process_idx >= self.cpu_history.len() {
            self.cpu_history.resize_with(process_idx + 1, || CircularBuffer::new(self.history_max_points));
        }
        self.cpu_history[process_idx].push(cpu_usage);
    }

    /// Updates memory usage history for a monitored process
    pub fn update_memory(&mut self, process_idx: usize, memory_mb: f32) {
        if process_idx >= self.memory_history.len() {
            self.memory_history.resize_with(process_idx + 1, || CircularBuffer::new(self.history_max_points));
        }
        self.memory_history[process_idx].push(memory_mb);
    }

    /// Updates CPU usage history for a child process
    pub fn update_child_cpu(&mut self, pid: Pid, cpu_usage: f32) {
        self.child_cpu_history
            .entry(pid)
            .or_insert_with(|| CircularBuffer::new(self.history_max_points))
            .push(cpu_usage);
    }

    /// Updates memory usage history for a child process
    pub fn update_child_memory(&mut self, pid: Pid, memory_mb: f32) {
        self.child_memory_history
            .entry(pid)
            .or_insert_with(|| CircularBuffer::new(self.history_max_points))
            .push(memory_mb);
    }

    /// Gets CPU usage history for a monitored process
    pub fn get_process_cpu_history(&self, idx: usize) -> Option<Vec<f32>> {
        self.cpu_history.get(idx).map(|h| h.as_slice())
    }

    /// Gets CPU usage history for a child process
    pub fn get_child_cpu_history(&self, pid: &Pid) -> Option<Vec<f32>> {
        self.child_cpu_history.get(pid).map(|h| h.as_slice())
    }

    /// Gets memory usage history for a monitored process
    pub fn get_memory_history(&self, idx: usize) -> Option<Vec<f32>> {
        self.memory_history.get(idx).map(|h| h.as_slice())
    }

    /// Gets memory usage history for a child process
    pub fn get_child_memory_history(&self, pid: &Pid) -> Option<Vec<f32>> {
        self.child_memory_history.get(pid).map(|h| h.as_slice())
    }

    /// Gets the last CPU value for a monitored process
    pub fn get_last_cpu(&self, idx: usize) -> Option<f32> {
        self.cpu_history.get(idx).map(|h| h.last_value())
    }

    /// Gets the last memory value for a monitored process
    pub fn get_last_memory(&self, idx: usize) -> Option<f32> {
        self.memory_history.get(idx).map(|h| h.last_value())
    }

    /// Gets the peak CPU value for a monitored process
    pub fn get_peak_cpu(&self, idx: usize) -> Option<f32> {
        self.cpu_history.get(idx).map(|h| h.max_value())
    }

    /// Gets the peak memory value for a monitored process
    pub fn get_peak_memory(&self, idx: usize) -> Option<f32> {
        self.memory_history.get(idx).map(|h| h.max_value())
    }

    /// Removes history entries for child processes that are no longer active
    pub fn cleanup_child_histories(&mut self, active_pids: &[Pid]) {
        self.child_cpu_history.retain(|pid, _| active_pids.contains(pid));
        self.child_memory_history.retain(|pid, _| active_pids.contains(pid));
    }

    /// Removes history for a monitored process
    pub fn remove_process(&mut self, idx: usize) {
        if idx < self.cpu_history.len() {
            self.cpu_history.remove(idx);
            self.memory_history.remove(idx);
        }
    }
} 