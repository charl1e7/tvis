use std::collections::HashMap;
use sysinfo::Pid;

#[derive(Default)]
pub struct ProcessHistory {
    pub cpu_history: Vec<Vec<f32>>,
    pub memory_history: Vec<f32>,
    pub child_cpu_history: HashMap<Pid, Vec<f32>>,
    pub history_max_points: usize,
}

impl ProcessHistory {
    pub fn new(max_points: usize) -> Self {
        Self {
            cpu_history: Vec::new(),
            memory_history: Vec::new(),
            child_cpu_history: HashMap::new(),
            history_max_points: max_points,
        }
    }

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

    pub fn update_child_cpu(&mut self, pid: Pid, cpu_usage: f32) {
        let history = self.child_cpu_history.entry(pid).or_insert_with(Vec::new);
        history.push(cpu_usage);
        if history.len() > self.history_max_points {
            history.remove(0);
        }
    }

    pub fn update_memory(&mut self, memory_mb: f32) {
        self.memory_history.push(memory_mb);
        if self.memory_history.len() > self.history_max_points {
            self.memory_history.remove(0);
        }
    }

    pub fn get_process_cpu_history(&self, idx: usize) -> Option<&Vec<f32>> {
        self.cpu_history.get(idx)
    }

    pub fn get_child_cpu_history(&self, pid: &Pid) -> Option<&Vec<f32>> {
        self.child_cpu_history.get(pid)
    }

    pub fn cleanup_child_histories(&mut self, active_pids: &[Pid]) {
        self.child_cpu_history.retain(|pid, _| active_pids.contains(pid));
    }

    pub fn remove_process(&mut self, idx: usize) {
        if idx < self.cpu_history.len() {
            self.cpu_history.remove(idx);
        }
    }
} 