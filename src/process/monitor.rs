use super::{ProcessInfo, ProcessStats, ProcessHistory};
use sysinfo::{System, Process};
use std::time::{Duration, Instant};
use std::collections::HashSet;

/// Monitors system processes and provides real-time statistics
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

    pub fn update_interval(&self) -> Duration {
        self.update_interval
    }

    pub fn set_update_interval(&mut self, interval: Duration) {
        self.update_interval = interval;
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
            .map(|p| p.name().to_string_lossy().into_owned())
            .collect();
        processes.sort();
        processes.dedup();
        processes
    }

    pub fn get_process_stats(&self, process_name: &str, history: &ProcessHistory, process_idx: usize) -> Option<ProcessStats> {
        let mut all_processes = Vec::new();
        let mut seen_pids = HashSet::new();

        let parent_pids: Vec<_> = self.system.processes()
            .values()
            .filter(|p| p.name().to_string_lossy() == process_name)
            .map(|p| p.pid())
            .collect();

        if parent_pids.is_empty() {
            return None;
        }

        self.system.processes()
            .values()
            .filter(|p| {
                p.name().to_string_lossy() == process_name || 
                p.parent().map(|parent_pid| parent_pids.contains(&parent_pid)).unwrap_or(false)
            })
            .for_each(|p| {
                if !seen_pids.contains(&p.pid()) {
                    seen_pids.insert(p.pid());
                    all_processes.push(ProcessInfo {
                        name: p.name().to_string_lossy().into_owned(),
                        pid: p.pid(),
                        cpu_usage: p.cpu_usage(),
                        memory_mb: p.memory() as f32 / 1024.0 / 1024.0,
                    });
                }
            });

        let (current_cpu, memory_mb): (f32, f32) = all_processes.iter()
            .fold((0.0, 0.0), |(cpu, mem), p| {
                (cpu + p.cpu_usage, mem + p.memory_mb)
            });

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

        Some(ProcessStats {
            current_cpu,
            avg_cpu,
            peak_cpu,
            memory_mb,
            peak_memory_mb: peak_memory,
            child_processes: all_processes,
            children_avg_cpu: 0.0,
            children_current_cpu: 0.0,
            children_peak_cpu: 0.0,
            children_memory_mb: 0.0,
            children_peak_memory_mb: 0.0,
        })
    }

    pub fn process_exists(&self, process_name: &str) -> bool {
        self.system.processes()
            .values()
            .any(|p| p.name().to_string_lossy() == process_name)
    }

    /// Gets basic statistics for a process without requiring history
    pub fn get_basic_stats(&self, process_name: &str) -> Option<ProcessStats> {
        let mut all_processes = Vec::new();
        let mut seen_pids = HashSet::new();

        let parent_pids: Vec<_> = self.system.processes()
            .values()
            .filter(|p| p.name().to_string_lossy() == process_name)
            .map(|p| p.pid())
            .collect();

        if parent_pids.is_empty() {
            return None;
        }

        self.system.processes()
            .values()
            .filter(|p| {
                p.name().to_string_lossy() == process_name || 
                p.parent().map(|parent_pid| parent_pids.contains(&parent_pid)).unwrap_or(false)
            })
            .for_each(|p| {
                if !seen_pids.contains(&p.pid()) {
                    seen_pids.insert(p.pid());
                    all_processes.push(ProcessInfo {
                        name: p.name().to_string_lossy().into_owned(),
                        pid: p.pid(),
                        cpu_usage: p.cpu_usage(),
                        memory_mb: p.memory() as f32 / 1024.0 / 1024.0,
                    });
                }
            });

        let (current_cpu, memory_mb): (f32, f32) = all_processes.iter()
            .fold((0.0, 0.0), |(cpu, mem), p| {
                (cpu + p.cpu_usage, mem + p.memory_mb)
            });

        Some(ProcessStats {
            current_cpu,
            avg_cpu: current_cpu,
            peak_cpu: current_cpu,
            memory_mb,
            peak_memory_mb: memory_mb,
            child_processes: all_processes,
            children_avg_cpu: 0.0,
            children_current_cpu: 0.0,
            children_peak_cpu: 0.0,
            children_memory_mb: 0.0,
            children_peak_memory_mb: 0.0,
        })
    }
} 