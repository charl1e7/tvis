use super::{ProcessHistory, ProcessInfo, ProcessStats, ProcessIdentifier};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use sysinfo::{Process, System};

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

    pub fn get_process_by_pid(&self, pid: sysinfo::Pid) -> Option<ProcessInfo> {
        self.system.processes().get(&pid).map(|p| self.collect_process_info(p))
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

    fn collect_processes(&self, identifier: &ProcessIdentifier) -> Option<(Vec<ProcessInfo>, usize)> {
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
                    } else {
                        all_processes.push(info);
                    }

                    let child_pids = self.collect_child_pids(&[*pid], &mut seen_pids);
                    for child_pid in child_pids {
                        if let Some(process) = self.system.processes().get(&child_pid) {
                            let info = self.collect_process_info(process);
                            if info.is_thread {
                                thread_count += 1;
                            } else {
                                all_processes.push(info);
                            }
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

                // Collect parent processes
                let parent_pids: Vec<_> = self
                    .system
                    .processes()
                    .values()
                    .filter(|p| p.name().to_string_lossy() == name.as_str())
                    .map(|p| p.pid())
                    .collect();

                if parent_pids.is_empty() {
                    return None;
                }

                for pid in &parent_pids {
                    if let Some(process) = self.system.processes().get(pid) {
                        seen_pids.insert(*pid);
                        let info = self.collect_process_info(process);
                        if info.is_thread {
                            thread_count += 1;
                        } else {
                            all_processes.push(info);
                        }
                    }
                }

                let child_pids = self.collect_child_pids(&parent_pids, &mut seen_pids);
                for pid in child_pids {
                    if let Some(process) = self.system.processes().get(&pid) {
                        let info = self.collect_process_info(process);
                        if info.is_thread {
                            thread_count += 1;
                        } else {
                            all_processes.push(info);
                        }
                    }
                }

                Some((all_processes, thread_count))
            }
        }
    }

    fn collect_process_info(&self, process: &Process) -> ProcessInfo {
        let is_thread = process.thread_kind().is_some();
        
        let memory_mb = if is_thread {
            0.0
        } else {
            process.memory() as f32 / 1024.0 / 1024.0
        };

        let name = if is_thread {
            format!("{} (thread)", process.name().to_string_lossy())
        } else {
            process.name().to_string_lossy().into_owned()
        };

        ProcessInfo {
            name,
            pid: process.pid(),
            parent_pid: process.parent(),
            cpu_usage: process.cpu_usage(),
            memory_mb,
            is_thread,
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
        identifier: &ProcessIdentifier,
        history: &ProcessHistory,
        process_idx: usize,
    ) -> Option<ProcessStats> {
        let (processes, thread_count) = self.collect_processes(identifier)?;
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
                .any(|p| p.name().to_string_lossy() == name.as_str()),
        }
    }

    /// Gets basic statistics for a process without requiring history
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
