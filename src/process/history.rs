use std::collections::HashMap;
use sysinfo::Pid;

/// Stores historical data for processes and their children
#[derive(Default)]
pub struct ProcessHistory {
    /// CPU usage history for each monitored process
    pub cpu_history: Vec<Vec<f32>>,
    /// Memory usage history for each monitored process
    pub memory_history: Vec<Vec<f32>>,
    /// CPU usage history for child processes, indexed by PID
    pub child_cpu_history: HashMap<Pid, Vec<f32>>,
    /// Memory usage history for child processes, indexed by PID
    pub child_memory_history: HashMap<Pid, Vec<f32>>,
    /// Maximum number of data points to store in history
    pub history_max_points: usize,
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
            self.cpu_history.push(Vec::new());
        }
        
        let history = &mut self.cpu_history[process_idx];
        history.push(cpu_usage);
        if history.len() > self.history_max_points {
            history.remove(0);
        }
    }

    /// Updates CPU usage history for a child process
    pub fn update_child_cpu(&mut self, pid: Pid, cpu_usage: f32) {
        let history = self.child_cpu_history.entry(pid).or_insert_with(Vec::new);
        history.push(cpu_usage);
        if history.len() > self.history_max_points {
            history.remove(0);
        }
    }

    /// Updates memory usage history for a monitored process
    pub fn update_memory(&mut self, process_idx: usize, memory_mb: f32) {
        if process_idx >= self.memory_history.len() {
            self.memory_history.push(Vec::new());
        }
        
        let history = &mut self.memory_history[process_idx];
        history.push(memory_mb);
        if history.len() > self.history_max_points {
            history.remove(0);
        }
    }

    /// Updates memory usage history for a child process
    pub fn update_child_memory(&mut self, pid: Pid, memory_mb: f32) {
        let history = self.child_memory_history.entry(pid).or_insert_with(Vec::new);
        history.push(memory_mb);
        if history.len() > self.history_max_points {
            history.remove(0);
        }
    }

    /// Gets CPU usage history for a monitored process
    pub fn get_process_cpu_history(&self, idx: usize) -> Option<&Vec<f32>> {
        self.cpu_history.get(idx)
    }

    /// Gets CPU usage history for a child process
    pub fn get_child_cpu_history(&self, pid: &Pid) -> Option<&Vec<f32>> {
        self.child_cpu_history.get(pid)
    }

    /// Gets memory usage history for a monitored process
    pub fn get_memory_history(&self, idx: usize) -> Option<&Vec<f32>> {
        self.memory_history.get(idx)
    }

    /// Gets memory usage history for a child process

    pub fn get_child_memory_history(&self, pid: &Pid) -> Option<&Vec<f32>> {
        self.child_memory_history.get(pid)
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