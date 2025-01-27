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

    pub fn should_update(&self) -> bool {
        self.last_update.elapsed() >= self.update_interval
    }

    pub fn update(&mut self) {
        self.system.refresh_all();
        self.last_update = Instant::now();
    }

    pub fn get_process_by_pid(&self, pid: &Pid) -> Option<&Process> {
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

    pub fn collect_process_info(&self, process: &Process, history: &ProcessHistory) -> ProcessInfo {
        let (avg_cpu, avg_memory, peak_cpu, peak_memory) = if let (Some(cpu_history), Some(mem_history)) = (
            history.get_cpu_history(&process.pid()),
            history.get_memory_history(&process.pid())
        ) {
            let mut sum_cpu = 0.0;
            let mut sum_memory = 0.0;
            let mut max_cpu = 0.0;
            let mut max_memory = 0.0;
            let len = cpu_history.len();

            for i in 0..len {
                let cpu_val = cpu_history[i];
                let mem_val = mem_history[i];
                
                sum_cpu += cpu_val;
                sum_memory += mem_val;
                max_cpu = f32::max(max_cpu, cpu_val);
                max_memory = f32::max(max_memory, mem_val);
            }

            (sum_cpu / len as f32, sum_memory / len as f32, max_cpu, max_memory)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
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
            avg_cpu,
            avg_memory,
            peak_cpu,
            peak_memory,
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

    pub fn process_exists(&self, identifier: &ProcessIdentifier) -> bool {
        match identifier {
            ProcessIdentifier::Pid(pid) => self.system.process(*pid).is_some(),
            ProcessIdentifier::Name(name) => self
                .system
                .processes()
                .values()
                .any(|p| p.name().to_string_lossy() == *name),
        }
    }
}
