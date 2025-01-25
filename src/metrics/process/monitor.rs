use super::{ProcessHistory, ProcessIdentifier, ProcessInfo, ProcessStats};
use log::info;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use sysinfo::{Process, System};

#[derive(Debug)]
pub struct ProcessMonitor {
    pub system: System,
    last_update: Instant,
    update_interval: Duration,
}

impl Default for ProcessMonitor {
    fn default() -> Self {
        Self {
            system: System::new_all(),
            last_update: Instant::now(),
            update_interval: Duration::from_millis(1000),
        }
    }
}

impl ProcessMonitor {
    pub fn new(update_interval: Duration) -> Self {
        Self {
            system: System::new_all(),
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

    pub fn get_process_by_pid(&self, pid: sysinfo::Pid) -> Option<ProcessInfo> {
        self.system
            .processes()
            .get(&pid)
            .map(|p| self.collect_process_info(p))
    }

    pub fn get_all_processes_with_pid(&self) -> Vec<(String, sysinfo::Pid)> {
        let mut processes: Vec<_> = self
            .system
            .processes()
            .values()
            .map(|p| (p.name().to_string_lossy().into_owned(), p.pid()))
            .collect();
        processes.sort_by(|a, b| a.0.cmp(&b.0));
        processes
    }

    fn collect_processes(
        &self,
        identifier: &ProcessIdentifier,
    ) -> Option<(Vec<ProcessInfo>, usize)> {
        match identifier {
            ProcessIdentifier::Pid(pid) => {
                let mut all_processes = Vec::new();
                let mut seen_pids = HashSet::new();
                let mut thread_count = 0;
                if let Some(process) = self.system.processes().get(pid) {
                    seen_pids.insert(*pid);
                    let info = self.collect_process_info(process);
                    if info.is_thread {
                        thread_count += 1;
                    }
                    all_processes.push(info);

                    let child_pids = self.collect_child_pids(&[*pid], &mut seen_pids);
                    for child_pid in child_pids {
                        if let Some(process) = self.system.processes().get(&child_pid) {
                            let info = self.collect_process_info(process);
                            if info.is_thread {
                                thread_count += 1;
                            }
                            all_processes.push(info);
                        }
                    }

                    Some((all_processes, thread_count))
                } else {
                    None
                }
            }
            ProcessIdentifier::Name(name) => {
                let mut all_processes = Vec::new();
                let mut seen_pids = HashSet::new();
                let mut thread_count = 0;

                let matching_pids: Vec<_> = self
                    .system
                    .processes()
                    .iter()
                    .filter(|(_, p)| p.name().to_string_lossy() == *name)
                    .map(|(pid, _)| *pid)
                    .collect();

                if matching_pids.is_empty() {
                    return None;
                }

                for pid in &matching_pids {
                    if !seen_pids.contains(pid) {
                        if let Some(process) = self.system.processes().get(pid) {
                            seen_pids.insert(*pid);
                            let info = self.collect_process_info(process);
                            if info.is_thread {
                                thread_count += 1;
                            }
                            all_processes.push(info);

                            let child_pids = self.collect_child_pids(&[*pid], &mut seen_pids);
                            for child_pid in child_pids {
                                if let Some(process) = self.system.processes().get(&child_pid) {
                                    let info = self.collect_process_info(process);
                                    if info.is_thread {
                                        thread_count += 1;
                                    }
                                    all_processes.push(info);
                                }
                            }
                        }
                    }
                }

                Some((all_processes, thread_count))
            }
        }
    }

    fn collect_process_info(&self, process: &Process) -> ProcessInfo {
        ProcessInfo {
            name: process.name().to_string_lossy().into_owned(),
            pid: process.pid(),
            parent_pid: process.parent(),
            cpu_usage: process.cpu_usage(),
            memory_mb: process.memory() as f32 / 1024.0,
            is_thread: process.name().to_string_lossy().contains("Thread"),
        }
    }

    fn collect_child_pids(
        &self,
        parent_pids: &[sysinfo::Pid],
        seen_pids: &mut HashSet<sysinfo::Pid>,
    ) -> Vec<sysinfo::Pid> {
        let mut child_pids = Vec::new();
        for (pid, process) in self.system.processes() {
            if !seen_pids.contains(pid) {
                if let Some(parent) = process.parent() {
                    if parent_pids.contains(&parent) {
                        seen_pids.insert(*pid);
                        child_pids.push(*pid);
                        // Recursively collect children of this process
                        let grandchildren = self.collect_child_pids(&[*pid], seen_pids);
                        child_pids.extend(grandchildren);
                    }
                }
            }
        }
        child_pids
    }

    fn calculate_stats(processes: &[ProcessInfo]) -> (f32, f32) {
        let total_cpu: f32 = processes.iter().map(|p| p.cpu_usage).sum();
        let total_memory: f32 = processes.iter().map(|p| p.memory_mb).sum();
        (total_cpu, total_memory)
    }

    fn calculate_history_stats(history: &[f32]) -> (f32, f32) {
        if history.is_empty() {
            return (0.0, 0.0);
        }
        let avg = history.iter().sum::<f32>() / history.len() as f32;
        let peak = history.iter().copied().fold(0.0, f32::max);
        (avg, peak)
    }

    pub fn get_process_stats(
        &self,
        identifier: &ProcessIdentifier,
        history: &ProcessHistory,
        _process_idx: usize,
    ) -> Option<ProcessStats> {
        let (processes, thread_count) = self.collect_processes(identifier)?;
        let (current_cpu, memory_mb) = Self::calculate_stats(&processes);

        let mut peak_cpu = current_cpu;
        let mut peak_memory_mb = memory_mb;
        let mut avg_cpu = current_cpu;

        // Calculate historical stats
        for process in &processes {
            if let Some(cpu_history) = history.get_process_cpu_history(&process.pid) {
                let (avg, peak) = Self::calculate_history_stats(&cpu_history);
                avg_cpu = avg_cpu.max(avg);
                peak_cpu = peak_cpu.max(peak);
            }
            if let Some(memory_history) = history.get_memory_history(&process.pid) {
                let (_, peak) = Self::calculate_history_stats(&memory_history);
                peak_memory_mb = peak_memory_mb.max(peak);
            }
        }

        Some(ProcessStats {
            current_cpu,
            avg_cpu,
            peak_cpu,
            memory_mb,
            peak_memory_mb,
            processes,
            thread_count,
        })
    }

    pub fn process_exists(&self, identifier: &ProcessIdentifier) -> bool {
        match identifier {
            ProcessIdentifier::Pid(pid) => self.system.processes().contains_key(pid),
            ProcessIdentifier::Name(name) => self
                .system
                .processes()
                .values()
                .any(|p| p.name().to_string_lossy() == *name),
        }
    }

    pub fn get_basic_stats(&self, identifier: &ProcessIdentifier) -> Option<ProcessStats> {
        let (processes, thread_count) = self.collect_processes(identifier)?;
        let (current_cpu, memory_mb) = Self::calculate_stats(&processes);

        Some(ProcessStats {
            current_cpu,
            avg_cpu: current_cpu,
            peak_cpu: current_cpu,
            memory_mb,
            peak_memory_mb: memory_mb,
            processes,
            thread_count,
        })
    }
}
