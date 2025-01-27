use std::collections::HashMap;
use sysinfo::Pid;

/// Stores historical data for processes and their children
#[derive(Default, Debug, Clone)]
pub struct ProcessHistory {
    /// Groups process metrics with its children
    histories: HashMap<Pid, ProcessMetrics>,
    /// Maximum number of data points to store in history
    pub history_len: usize,
}

/// Stores CPU and memory metrics for a process
#[derive(Default, Debug, Clone)]
pub struct ProcessMetrics {
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

    pub fn get_cpu_history(&self) -> Vec<f32> {
        self.cpu.as_slice()
    }

    pub fn get_memory_history(&self) -> Vec<f32> {
        self.memory.as_slice()
    }
}

/// A fixed-size circular buffer for storing historical data
#[derive(Default, Debug, Clone)]
pub struct CircularBuffer {
    data: Vec<f32>,
    position: usize,
    peak_value: f32,
    sum: f32,   // Track sum for efficient average calculation
    len: usize, // Track actual number of values
}

impl CircularBuffer {
    /// Creates a new circular buffer with the specified size
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0.0; size],
            position: 0,
            peak_value: 0.0,
            sum: 0.0,
            len: 0,
        }
    }

    /// Adds a new value to the buffer
    pub fn push(&mut self, value: f32) {
        if self.len == self.data.len() {
            self.sum -= self.data[self.position];
        } else {
            self.len += 1;
        }
        self.sum += value;

        // Update the old value before checking if we need to recalculate peak
        let old_value = self.data[self.position];
        self.data[self.position] = value;
        self.position = (self.position + 1) % self.data.len();

        // Update peak value if:
        // 1. New value is higher than current peak
        // 2. We just overwrote the peak value
        // 3. Peak value is no longer in our window
        if value > self.peak_value {
            self.peak_value = value;
        } else if self.peak_value == old_value
            || self.peak_value > self.data.iter().copied().fold(0.0, f32::max)
        {
            self.peak_value = self.data[..self.len].iter().copied().fold(0.0, f32::max);
        }
    }

    /// Returns two slices representing the data in chronological order
    pub fn as_slices(&self) -> (&[f32], &[f32]) {
        let (first, second) = self.data.split_at(self.position);
        (second, first)
    }

    /// Returns a slice of the buffer's data in chronological order
    /// with newest values at the end
    pub fn as_slice(&self) -> Vec<f32> {
        let (first, second) = self.as_slices();
        let mut result = Vec::with_capacity(self.len);
        result.extend_from_slice(first);
        result.extend_from_slice(second);
        result
    }

    /// Returns the maximum value in the buffer
    pub fn max_value(&self) -> f32 {
        self.peak_value
    }

    /// Returns the last value in the buffer
    pub fn last_value(&self) -> f32 {
        if self.len == 0 {
            return 0.0;
        }
        let pos = if self.position == 0 {
            self.data.len() - 1
        } else {
            self.position - 1
        };
        self.data[pos]
    }
}

impl ProcessHistory {
    pub fn new(max_points: usize) -> Self {
        Self {
            histories: HashMap::new(),
            history_len: max_points,
        }
    }

    pub fn update_process_cpu(&mut self, _process_idx: usize, pid: Pid, cpu_usage: f32) {
        self.histories
            .entry(pid)
            .or_insert_with(|| ProcessMetrics::new(self.history_len))
            .update_cpu(cpu_usage);
    }

    pub fn update_memory(&mut self, _process_idx: usize, pid: Pid, memory_mb: f32) {
        self.histories
            .entry(pid)
            .or_insert_with(|| ProcessMetrics::new(self.history_len))
            .update_memory(memory_mb);
    }

    pub fn get_cpu_history(&self, pid: &Pid) -> Option<Vec<f32>> {
        self.histories
            .get(pid)
            .map(|metrics| metrics.get_cpu_history())
    }

    pub fn get_memory_history(&self, pid: &Pid) -> Option<Vec<f32>> {
        self.histories
            .get(pid)
            .map(|metrics| metrics.get_memory_history())
    }

    pub fn get_last_cpu(&self, _idx: usize, pid: &Pid) -> Option<f32> {
        self.histories
            .get(pid)
            .map(|metrics| metrics.cpu.last_value())
    }

    pub fn get_last_memory(&self, _idx: usize, pid: &Pid) -> Option<f32> {
        self.histories
            .get(pid)
            .map(|metrics| metrics.memory.last_value())
    }

    pub fn get_peak_cpu(&self, _idx: usize, pid: &Pid) -> Option<f32> {
        self.histories
            .get(pid)
            .map(|metrics| metrics.cpu.max_value())
    }

    pub fn get_peak_memory(&self, _idx: usize, pid: &Pid) -> Option<f32> {
        self.histories
            .get(pid)
            .map(|metrics| metrics.memory.max_value())
    }

    pub fn cleanup_histories(&mut self, active_pids: &[Pid]) {
        self.histories.retain(|pid, _| active_pids.contains(pid));
    }
}
