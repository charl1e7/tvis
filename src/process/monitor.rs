use super::{ProcessInfo, ProcessStats, ProcessHistory};
use sysinfo::{System, Process, Pid};
use std::time::{Duration, Instant};

pub struct ProcessMonitor {
    system: System,
    last_update: Instant,
    update_interval: Duration,
}

impl ProcessMonitor {
    pub fn new(update_interval: Duration) -> Self {
        Self {
            system: System::new(),
            last_update: Instant::now(),
            update_interval,
        }
    }

    pub fn should_update(&self) -> bool {
        self.last_update.elapsed() >= self.update_interval
    }

    pub fn update(&mut self) {
        self.system.refresh_all();
        self.last_update = Instant::now();
    }

    pub fn get_all_processes(&self) -> Vec<String> {
        let mut processes: Vec<_> = self.system.processes()
            .values()
            .map(|p| p.name().to_string())
            .collect();
        processes.sort();
        processes.dedup();
        processes
    }

    pub fn get_process_stats(&self, process_name: &str, history: &ProcessHistory, process_idx: usize) -> Option<ProcessStats> {
        let processes: Vec<_> = self.system.processes()
            .values()
            .filter(|p| p.name() == process_name)
            .collect();

        if processes.is_empty() {
            return None;
        }

        let child_processes = self.get_child_processes(&processes);
        
        let current_cpu: f32 = processes.iter().map(|p| p.cpu_usage()).sum();
        let memory_mb = processes.iter().map(|p| p.memory()).sum::<u64>() as f32 / 1024.0 / 1024.0;
        
        let children_current_cpu: f32 = child_processes.iter().map(|p| p.cpu_usage).sum();
        let children_memory_mb: f32 = child_processes.iter().map(|p| p.memory_mb).sum();

        // Calculate true average CPU for main process using history
        let avg_cpu = history.get_process_cpu_history(process_idx)
            .map(|h| h.iter().sum::<f32>() / h.len() as f32)
            .unwrap_or(current_cpu);

        // Calculate true average CPU for child processes using history
        let children_avg_cpu: f32 = child_processes.iter()
            .map(|child| {
                history.get_child_cpu_history(&child.pid)
                    .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                    .unwrap_or(child.cpu_usage)
            })
            .sum();

        Some(ProcessStats {
            current_cpu,
            avg_cpu,
            memory_mb,
            child_processes,
            children_avg_cpu,
            children_current_cpu,
            children_memory_mb,
        })
    }

    fn get_child_processes(&self, parent_processes: &[&sysinfo::Process]) -> Vec<ProcessInfo> {
        let parent_pids: Vec<_> = parent_processes.iter().map(|p| p.pid()).collect();
        
        self.system.processes()
            .values()
            .filter(|p| {
                if let Some(parent_pid) = p.parent() {
                    parent_pids.contains(&parent_pid)
                } else {
                    false
                }
            })
            .map(|p| ProcessInfo {
                name: p.name().to_string(),
                pid: p.pid(),
                cpu_usage: p.cpu_usage(),
                memory_mb: p.memory() as f32 / 1024.0 / 1024.0,
            })
            .collect()
    }

    pub fn process_exists(&self, process_name: &str) -> bool {
        self.system.processes()
            .values()
            .any(|p| p.name() == process_name)
    }

    // Simple version that doesn't need history for basic stats
    pub fn get_basic_stats(&self, process_name: &str) -> Option<ProcessStats> {
        let processes: Vec<_> = self.system.processes()
            .values()
            .filter(|p| p.name() == process_name)
            .collect();

        if processes.is_empty() {
            return None;
        }

        let child_processes = self.get_child_processes(&processes);
        
        let current_cpu: f32 = processes.iter().map(|p| p.cpu_usage()).sum();
        let memory_mb = processes.iter().map(|p| p.memory()).sum::<u64>() as f32 / 1024.0 / 1024.0;
        
        let children_current_cpu: f32 = child_processes.iter().map(|p| p.cpu_usage).sum();
        let children_memory_mb: f32 = child_processes.iter().map(|p| p.memory_mb).sum();

        Some(ProcessStats {
            current_cpu,
            avg_cpu: current_cpu,
            memory_mb,
            child_processes,
            children_avg_cpu: children_current_cpu,
            children_current_cpu,
            children_memory_mb,
        })
    }
} 