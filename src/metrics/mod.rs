use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{System, Pid};
use crate::process::{ProcessHistory, ProcessStats};
use log::info;

pub struct ProcessData {
    stats: ProcessStats,
    history: ProcessHistory,
    process_idx: usize,
}

impl ProcessData {
    pub fn new(history_size: usize, idx: usize) -> Self {
        Self {
            stats: ProcessStats {
                current_cpu: 0.0,
                avg_cpu: 0.0,
                peak_cpu: 0.0,
                memory_mb: 0.0,
                peak_memory_mb: 0.0,
                processes: Vec::new(),
                thread_count: 0,
            },
            history: ProcessHistory::new(history_size),
            process_idx: idx,
        }
    }
}

impl std::fmt::Debug for ProcessData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessData")
            .field("cpu", &self.stats.current_cpu)
            .field("avg_cpu", &self.stats.avg_cpu)
            .field("peak_cpu", &self.stats.peak_cpu)
            .field("memory_mb", &self.stats.memory_mb)
            .field("peak_memory_mb", &self.stats.peak_memory_mb)
            .field("thread_count", &self.stats.thread_count)
            .field("process_count", &self.stats.processes.len())
            .finish()
    }
}

pub struct Metrics {
    processes: HashMap<String, ProcessData>,
    selected_processes: Vec<String>,
    update_interval: Duration,
    history_size: usize,
}

impl std::fmt::Debug for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Metrics")
            .field("processes", &self.processes)
            .field("selected_processes", &self.selected_processes)
            .field("update_interval", &self.update_interval)
            .field("history_size", &self.history_size)
            .finish()
    }
}

impl Metrics {
    pub fn new(update_interval_ms: u64, history_size: usize) -> Arc<RwLock<Self>> {
        let metrics = Arc::new(RwLock::new(Self {
            processes: HashMap::new(),
            selected_processes: Vec::new(),
            update_interval: Duration::from_millis(update_interval_ms),
            history_size,
        }));

        let metrics_clone = Arc::clone(&metrics);
        thread::spawn(move || {
            let mut system = System::new();
            loop {
                system.refresh_all();
                info!("Refreshing system");
                // Get update interval and process list under a single read lock
                let (update_interval, processes_to_monitor, history_size) = {
                    let metrics = metrics_clone.read().unwrap();
                    (
                        metrics.update_interval,
                        metrics.selected_processes.clone(),
                        metrics.history_size,
                    )
                };

                // Prepare data updates without holding the write lock
                let mut updates = HashMap::new();
                for (idx, identifier) in processes_to_monitor.iter().enumerate() {
                    let processes: Vec<_> = if identifier.starts_with("pid:") {
                        if let Ok(pid_num) = identifier[4..].parse::<usize>() {
                            let pid = Pid::from(pid_num);
                            system.process(pid)
                                .into_iter()
                                .map(|p| (p, pid))
                                .collect()
                        } else {
                            Vec::new()
                        }
                    } else {
                        system.processes()
                            .iter()
                            .filter(|(_, p)| p.name().to_string_lossy().to_string() == *identifier)
                            .map(|(pid, p)| (p, *pid))
                            .collect()
                    };
                            
                    if !processes.is_empty() {
                        let total_cpu: f32 = processes.iter().map(|(p, _)| p.cpu_usage()).sum();
                        let total_memory: f32 = processes.iter()
                            .map(|(p, _)| p.memory() as f32 / 1024.0 / 1024.0)
                            .sum();

                        let mut process_infos = Vec::new();
                        let mut thread_count = 0;
                        for (process, pid) in &processes {
                            let is_thread = process.thread_kind().is_some();
                            if is_thread {
                                thread_count += 1;
                            }
                            
                            process_infos.push(crate::process::ProcessInfo {
                                name: process.name().to_string_lossy().into_owned(),
                                pid: *pid,
                                parent_pid: process.parent(),
                                cpu_usage: process.cpu_usage(),
                                memory_mb: if is_thread { 0.0 } else { process.memory() as f32 / 1024.0 / 1024.0 },
                                is_thread,
                            });
                        }

                        let stats = ProcessStats {
                            current_cpu: total_cpu,
                            avg_cpu: total_cpu, // Will be updated with history
                            peak_cpu: total_cpu, // Will be updated with history
                            memory_mb: total_memory,
                            peak_memory_mb: total_memory, // Will be updated with history
                            processes: process_infos,
                            thread_count,
                        };

                        updates.insert((identifier.clone(), idx), (stats, idx));
                    }
                }

                // Apply all updates at once under write lock
                {
                    let mut metrics = metrics_clone.write().unwrap();
                    for ((identifier, idx), (stats, _)) in updates {
                        let process_data = metrics.processes
                            .entry(identifier.clone())
                            .or_insert_with(|| ProcessData::new(history_size, idx));

                        // Update history
                        process_data.history.update_process_cpu(idx, stats.current_cpu);
                        process_data.history.update_memory(idx, stats.memory_mb);

                        // Update child processes
                        for child in &stats.processes {
                            process_data.history.update_child_cpu(
                                idx,
                                child.pid,
                                child.cpu_usage
                            );
                            process_data.history.update_child_memory(
                                idx,
                                child.pid,
                                child.memory_mb
                            );
                        }

                        // Update stats with history data
                        if let Some(cpu_history) = process_data.history.get_process_cpu_history(idx) {
                            let peak_cpu = cpu_history.iter().copied().fold(0.0, f32::max);
                            let avg_cpu = cpu_history.iter().sum::<f32>() / cpu_history.len() as f32;
                            process_data.stats.peak_cpu = peak_cpu;
                            process_data.stats.avg_cpu = avg_cpu;
                        }

                        if let Some(memory_history) = process_data.history.get_memory_history(idx) {
                            let peak_memory = memory_history.iter().copied().fold(0.0, f32::max);
                            process_data.stats.peak_memory_mb = peak_memory;
                        }

                        process_data.stats = stats;
                        process_data.process_idx = idx;
                    }

                    // Log updated metrics
                    info!("Updated Metrics: {:#?}", &*metrics);
                }

                thread::sleep(update_interval);
            }
        });

        metrics
    }

    pub fn add_selected_process(&mut self, identifier: String) {
        if !self.selected_processes.contains(&identifier) {
            self.selected_processes.push(identifier);
        }
    }

    pub fn remove_selected_process(&mut self, identifier: &str) {
        if let Some(pos) = self.selected_processes.iter().position(|x| x == identifier) {
            self.selected_processes.remove(pos);
            self.processes.remove(identifier);
        }
    }

    pub fn get_process_data(&self, identifier: &str) -> Option<&ProcessData> {
        self.processes.get(identifier)
    }

    pub fn get_selected_processes(&self) -> &[String] {
        &self.selected_processes
    }

    pub fn set_update_interval(&mut self, update_interval_ms: u64) {
        self.update_interval = Duration::from_millis(update_interval_ms);
    }
}
