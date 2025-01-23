use super::{ProcessHistory, ProcessInfo, ProcessStats};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use sysinfo::{Process, System};

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
        let mut processes: Vec<_> = self
            .system
            .processes()
            .values()
            .map(|p| p.name().to_string_lossy().into_owned())
            .collect();
        processes.sort();
        processes.dedup();
        processes
    }

    fn collect_process_info(&self, process: &Process) -> ProcessInfo {
        ProcessInfo {
            name: process.name().to_string_lossy().into_owned(),
            pid: process.pid(),
            parent_pid: process.parent(),
            cpu_usage: process.cpu_usage(),
            memory_mb: process.memory() as f32 / 1024.0 / 1024.0,
        }
    }

    fn collect_child_pids(
        &self,
        parent_pids: &[sysinfo::Pid],
        seen_pids: &mut HashSet<sysinfo::Pid>,
    ) -> Vec<sysinfo::Pid> {
        let mut child_pids = Vec::new();

        for process in self.system.processes().values() {
            if let Some(parent_pid) = process.parent() {
                if parent_pids.contains(&parent_pid) && !seen_pids.contains(&process.pid()) {
                    seen_pids.insert(process.pid());
                    child_pids.push(process.pid());
                    let grandchild_pids = self.collect_child_pids(&[process.pid()], seen_pids);
                    child_pids.extend(grandchild_pids);
                }
            }
        }

        child_pids
    }

    fn collect_processes(&self, process_name: &str) -> Option<Vec<ProcessInfo>> {
        let mut all_processes = Vec::new();
        let mut seen_pids = HashSet::new();

        // Collect parent processes
        let parent_pids: Vec<_> = self
            .system
            .processes()
            .values()
            .filter(|p| p.name().to_string_lossy() == process_name)
            .map(|p| p.pid())
            .collect();

        if parent_pids.is_empty() {
            return None;
        }

        // Add parent processes
        for pid in &parent_pids {
            if let Some(process) = self.system.processes().get(pid) {
                seen_pids.insert(*pid);
                all_processes.push(self.collect_process_info(process));
            }
        }

        // Collect and add child processes
        let child_pids = self.collect_child_pids(&parent_pids, &mut seen_pids);
        for pid in child_pids {
            if let Some(process) = self.system.processes().get(&pid) {
                all_processes.push(self.collect_process_info(process));
            }
        }

        Some(all_processes)
    }

    fn calculate_stats(processes: &[ProcessInfo]) -> (f32, f32) {
        processes.iter().fold((0.0, 0.0), |(cpu, mem), p| {
            (cpu + p.cpu_usage, mem + p.memory_mb)
        })
    }

    fn calculate_history_stats(history: &[f32]) -> (f32, f32) {
        let mut max = 0.0f32;
        let mut sum = 0.0f32;
        for &v in history {
            max = max.max(v);
            sum += v;
        }
        (max, sum / history.len() as f32)
    }

    pub fn get_process_stats(
        &self,
        process_name: &str,
        history: &ProcessHistory,
        process_idx: usize,
    ) -> Option<ProcessStats> {
        let processes = self.collect_processes(process_name)?;
        let (current_cpu, memory_mb) = Self::calculate_stats(&processes);

        let (peak_cpu, avg_cpu) = history
            .get_process_cpu_history(process_idx)
            .map(|h| Self::calculate_history_stats(h.as_slice()))
            .unwrap_or((current_cpu, current_cpu));

        let peak_memory = history
            .get_memory_history(process_idx)
            .map(|h| h.iter().copied().fold(0.0, f32::max))
            .unwrap_or(memory_mb);

        Some(ProcessStats {
            current_cpu,
            avg_cpu,
            peak_cpu,
            memory_mb,
            peak_memory_mb: peak_memory,
            processes,
        })
    }

    pub fn process_exists(&self, process_name: &str) -> bool {
        self.system
            .processes()
            .values()
            .any(|p| p.name().to_string_lossy() == process_name)
    }

    /// Gets basic statistics for a process without requiring history
    pub fn get_basic_stats(&self, process_name: &str) -> Option<ProcessStats> {
        let processes = self.collect_processes(process_name)?;
        let (current_cpu, memory_mb) = Self::calculate_stats(&processes);

        Some(ProcessStats {
            current_cpu,
            avg_cpu: current_cpu,
            peak_cpu: current_cpu,
            memory_mb,
            peak_memory_mb: memory_mb,
            processes,
        })
    }
}
