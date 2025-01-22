use super::{ProcessInfo, ProcessStats, ProcessHistory};
use sysinfo::{System, Process};
use std::time::{Duration, Instant};

/// Monitors system processes and provides real-time statistics
pub struct ProcessMonitor {
    system: System,
    last_update: Instant,
    update_interval: Duration,
}

impl ProcessMonitor {
    /// Creates a new ProcessMonitor with the specified update interval
    pub fn new(update_interval: Duration) -> Self {
        Self {
            system: System::new(),
            last_update: Instant::now(),
            update_interval,
        }
    }

    /// Gets the current update interval
    pub fn update_interval(&self) -> Duration {
        self.update_interval
    }

    /// Sets a new update interval
    pub fn set_update_interval(&mut self, interval: Duration) {
        self.update_interval = interval;
    }

    /// Checks if enough time has passed for the next update
    pub fn should_update(&self) -> bool {
        self.last_update.elapsed() >= self.update_interval
    }

    /// Refreshes system information
    pub fn update(&mut self) {
        self.system.refresh_all();
        self.last_update = Instant::now();
    }

    /// Returns a sorted list of all process names in the system
    pub fn get_all_processes(&self) -> Vec<String> {
        let mut processes: Vec<_> = self.system.processes()
            .values()
            .map(|p| p.name().to_string_lossy().into_owned())
            .collect();
        processes.sort();
        processes.dedup();
        processes
    }

    /// Gets detailed statistics for a process and its children
    pub fn get_process_stats(&self, process_name: &str, history: &ProcessHistory, process_idx: usize) -> Option<ProcessStats> {
        // Collect all processes with this name
        let processes: Vec<_> = self.system.processes()
            .values()
            .filter(|p| p.name().to_string_lossy() == process_name)
            .collect();

        if processes.is_empty() {
            return None;
        }

        // Get child processes once and cache results
        let child_processes = self.get_child_processes(&processes);
        
        // Calculate main process stats in one pass
        let (current_cpu, memory_mb): (f32, f32) = processes.iter()
            .fold((0.0, 0.0), |(cpu, mem), p| {
                (
                    cpu + p.cpu_usage(),
                    mem + (p.memory() as f32 / 1024.0 / 1024.0)
                )
            });
        
        // Calculate child process stats in one pass
        let (children_current_cpu, children_memory_mb): (f32, f32) = child_processes.iter()
            .fold((0.0, 0.0), |(cpu, mem), p| {
                (cpu + p.cpu_usage, mem + p.memory_mb)
            });

        // Get history values efficiently
        let (peak_cpu, avg_cpu) = history.get_process_cpu_history(process_idx)
            .map(|h| {
                let mut max = 0.0f32;
                let mut sum = 0.0f32;
                for &v in h.iter() {
                    max = max.max(v);
                    sum += v;
                }
                (max, sum / h.len() as f32)
            })
            .unwrap_or((current_cpu, current_cpu));

        let peak_memory = history.get_memory_history(process_idx)
            .map(|h| h.iter().copied().fold(0.0, f32::max))
            .unwrap_or(memory_mb);

        // Calculate children stats efficiently
        let (children_peak_cpu, children_avg_cpu) = child_processes.iter()
            .fold((0.0, 0.0), |(peak, avg), child| {
                let (p, a) = history.get_child_cpu_history(&child.pid)
                    .map(|h| {
                        let mut max = 0.0f32;
                        let mut sum = 0.0f32;
                        for &v in h.iter() {
                            max = max.max(v);
                            sum += v;
                        }
                        (max, sum / h.len() as f32)
                    })
                    .unwrap_or((child.cpu_usage, child.cpu_usage));
                (peak + p, avg + a)
            });

        let children_peak_memory = child_processes.iter()
            .map(|child| {
                history.get_child_memory_history(&child.pid)
                    .map(|h| h.iter().copied().fold(0.0, f32::max))
                    .unwrap_or(child.memory_mb)
            })
            .sum();

        Some(ProcessStats {
            current_cpu,
            avg_cpu,
            peak_cpu,
            memory_mb,
            peak_memory_mb: peak_memory,
            child_processes,
            children_avg_cpu,
            children_current_cpu,
            children_peak_cpu,
            children_memory_mb,
            children_peak_memory_mb: children_peak_memory,
        })
    }

    /// Gets child processes for the given parent processes
    fn get_child_processes(&self, parent_processes: &[&Process]) -> Vec<ProcessInfo> {
        let parent_pids: Vec<_> = parent_processes.iter()
            .map(|p| p.pid())
            .collect();
        
        self.system.processes()
            .values()
            .filter(|p| {
                p.parent()
                    .map(|parent_pid| parent_pids.contains(&parent_pid))
                    .unwrap_or(false)
            })
            .map(|p| ProcessInfo {
                name: p.name().to_string_lossy().into_owned(),
                pid: p.pid(),
                cpu_usage: p.cpu_usage(),
                memory_mb: p.memory() as f32 / 1024.0 / 1024.0,
            })
            .collect()
    }

    /// Checks if a process exists in the system
    pub fn process_exists(&self, process_name: &str) -> bool {
        self.system.processes()
            .values()
            .any(|p| p.name().to_string_lossy() == process_name)
    }

    /// Gets basic statistics for a process without requiring history
    pub fn get_basic_stats(&self, process_name: &str) -> Option<ProcessStats> {
        let processes: Vec<_> = self.system.processes()
            .values()
            .filter(|p| p.name().to_string_lossy() == process_name)
            .collect();

        if processes.is_empty() {
            return None;
        }

        let child_processes = self.get_child_processes(&processes);
        
        let current_cpu: f32 = processes.iter()
            .map(|p| p.cpu_usage())
            .sum();
            
        let memory_mb = processes.iter()
            .map(|p| p.memory())
            .sum::<u64>() as f32 / 1024.0 / 1024.0;
        
        let children_current_cpu: f32 = child_processes.iter()
            .map(|p| p.cpu_usage)
            .sum();
            
        let children_memory_mb: f32 = child_processes.iter()
            .map(|p| p.memory_mb)
            .sum();

        Some(ProcessStats {
            current_cpu,
            avg_cpu: current_cpu, // No history available, use current
            peak_cpu: current_cpu, // No history available, use current
            memory_mb,
            peak_memory_mb: memory_mb, // No history available, use current
            child_processes,
            children_avg_cpu: children_current_cpu, // No history available, use current
            children_current_cpu,
            children_peak_cpu: children_current_cpu, // No history available, use current
            children_memory_mb,
            children_peak_memory_mb: children_memory_mb, // No history available, use current
        })
    }
} 