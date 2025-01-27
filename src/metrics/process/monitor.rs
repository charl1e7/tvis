use super::{ProcessHistory, ProcessIdentifier, ProcessInfo};
use log::info;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};
use sysinfo::{Pid, Process, System};

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

    pub fn get_process(&self, pid: &Pid) -> Option<&Process> {
        self.system.process(*pid)
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

    pub fn collect_process_info(&self, process: &Process) -> ProcessInfo {
        let is_thread = process.thread_kind().is_some();
        let memory_mb = if is_thread {
            0.0
        } else {
            process.memory() as f32 / (1024.0 * 1024.0)
        };
        ProcessInfo {
            name: process.name().to_string_lossy().into_owned(),
            pid: process.pid(),
            parent_pid: process.parent(),
            cpu_usage: process.cpu_usage(),
            memory_mb,
            is_thread,
        }
    }

    pub fn find_all_relation(&self, identifier: &ProcessIdentifier) -> Option<Vec<Pid>> {
        let target_pids = match identifier {
            ProcessIdentifier::Pid(pid) => {
                vec![*pid]
            }
            ProcessIdentifier::Name(name) => self
                .system
                .processes()
                .iter()
                .filter(|(_, p)| p.name().to_string_lossy() == *name)
                .map(|(pid, _)| *pid)
                .collect(),
        };
        if target_pids.is_empty() {
            return None;
        }
        let mut parent_to_children: HashMap<Option<Pid>, Vec<Pid>> = HashMap::new();

        for (pid, process) in self.system.processes() {
            parent_to_children
                .entry(process.parent())
                .or_default()
                .push(*pid);
        }

        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut result = Vec::new();

        for pid in target_pids {
            if visited.insert(pid) {
                queue.push_back(pid);
            }
        }

        while let Some(current_pid) = queue.pop_front() {
            result.push(current_pid);

            if let Some(children) = parent_to_children.get(&Some(current_pid)) {
                for &child_pid in children {
                    if visited.insert(child_pid) {
                        queue.push_back(child_pid);
                    }
                }
            }
        }

        (!result.is_empty()).then_some(result)
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
}
