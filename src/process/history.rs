use std::collections::HashMap;
use sysinfo::Pid;

/// Stores historical data for processes and their children
#[derive(Default)]
pub struct ProcessHistory {
    /// CPU usage history for each monitored process
    pub cpu_history: Vec<Vec<f32>>,
    /// Memory usage history for the main process
    pub memory_history: Vec<f32>,
    /// CPU usage history for child processes, indexed by PID
    pub child_cpu_history: HashMap<Pid, Vec<f32>>,
    /// Maximum number of data points to store in history
    pub history_max_points: usize,
}

impl ProcessHistory {
    /// Creates a new ProcessHistory with the specified maximum history size
    ///
    /// # Arguments
    /// * `max_points` - Maximum number of historical data points to store
    pub fn new(max_points: usize) -> Self {
        Self {
            cpu_history: Vec::new(),
            memory_history: Vec::new(),
            child_cpu_history: HashMap::new(),
            history_max_points: max_points,
        }
    }

    /// Updates CPU usage history for a monitored process
    ///
    /// # Arguments
    /// * `process_idx` - Index of the process in the monitoring list
    /// * `cpu_usage` - Current CPU usage percentage
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
    ///
    /// # Arguments
    /// * `pid` - Process ID of the child process
    /// * `cpu_usage` - Current CPU usage percentage
    pub fn update_child_cpu(&mut self, pid: Pid, cpu_usage: f32) {
        let history = self.child_cpu_history.entry(pid).or_insert_with(Vec::new);
        history.push(cpu_usage);
        if history.len() > self.history_max_points {
            history.remove(0);
        }
    }

    /// Updates memory usage history
    ///
    /// # Arguments
    /// * `memory_mb` - Current memory usage in megabytes
    pub fn update_memory(&mut self, memory_mb: f32) {
        self.memory_history.push(memory_mb);
        if self.memory_history.len() > self.history_max_points {
            self.memory_history.remove(0);
        }
    }

    /// Gets CPU usage history for a monitored process
    ///
    /// # Arguments
    /// * `idx` - Index of the process in the monitoring list
    ///
    /// # Returns
    /// * `Some(&Vec<f32>)` if the process exists
    /// * `None` if the process index is invalid
    pub fn get_process_cpu_history(&self, idx: usize) -> Option<&Vec<f32>> {
        self.cpu_history.get(idx)
    }

    /// Gets CPU usage history for a child process
    ///
    /// # Arguments
    /// * `pid` - Process ID of the child process
    ///
    /// # Returns
    /// * `Some(&Vec<f32>)` if history exists for the PID
    /// * `None` if no history exists
    pub fn get_child_cpu_history(&self, pid: &Pid) -> Option<&Vec<f32>> {
        self.child_cpu_history.get(pid)
    }

    /// Removes history entries for child processes that are no longer active
    ///
    /// # Arguments
    /// * `active_pids` - List of currently active process IDs
    pub fn cleanup_child_histories(&mut self, active_pids: &[Pid]) {
        self.child_cpu_history.retain(|pid, _| active_pids.contains(pid));
    }

    /// Removes history for a monitored process
    ///
    /// # Arguments
    /// * `idx` - Index of the process to remove
    pub fn remove_process(&mut self, idx: usize) {
        if idx < self.cpu_history.len() {
            self.cpu_history.remove(idx);
        }
    }
} 