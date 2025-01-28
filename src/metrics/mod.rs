use log::info;
pub mod process;
use process::{
    ProcessData, ProcessGeneral, ProcessGeneralStats, ProcessHistory, ProcessIdentifier,
    ProcessInfo, ProcessMonitor,
};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};

pub static GENERAL_STATS_PID: LazyLock<Pid> = LazyLock::new(|| Pid::from_u32(0));

#[derive(Debug, Default)]
pub struct Metrics {
    monitored_processes: Vec<ProcessIdentifier>,
    processes: HashMap<ProcessIdentifier, ProcessData>,
    pub monitor: ProcessMonitor,
    pub update_interval: Duration,
    pub history_len: usize,
}

impl Metrics {
    pub fn new(history_len: usize, update_interval_ms: usize) -> Arc<RwLock<Self>> {
        let metrics = Arc::new(RwLock::new(Self {
            update_interval: Duration::from_millis(update_interval_ms as u64),
            history_len,
            processes: HashMap::new(),
            ..Default::default()
        }));

        let metrics_clone = Arc::clone(&metrics);
        let mut update_interval = Duration::from_millis(3000);
        let mut metrics_thread = Metrics {
            monitor: ProcessMonitor::new(Duration::from_millis(update_interval_ms as u64)),
            update_interval: update_interval,
            history_len: 10,
            ..Default::default()
        };
        thread::sleep(update_interval);
        thread::spawn(move || loop {
            {
                let metrics_read = metrics_clone.read().unwrap();
                update_interval = metrics_read.update_interval;
                metrics_thread.update_interval = metrics_read.update_interval;
                metrics_thread.history_len = metrics_read.history_len;
                metrics_thread.monitored_processes = metrics_read.monitored_processes.clone();
            }
            {
                metrics_thread.update_metrics();
                let mut metrics_write = metrics_clone.write().unwrap();
                metrics_write.processes = metrics_thread.processes.clone();
            }
            metrics_thread.monitor =
                ProcessMonitor::new(Duration::from_millis(update_interval_ms as u64));
            thread::sleep(update_interval);
            metrics_thread.monitor.update();
        });

        metrics.clone()
    }

    pub fn add_selected_process(&mut self, identifier: ProcessIdentifier) {
        if !self.monitored_processes.contains(&identifier) {
            self.monitored_processes.push(identifier.clone());
        }
    }

    pub fn remove_selected_process(&mut self, identifier: &ProcessIdentifier) {
        if let Some(pos) = self
            .monitored_processes
            .iter()
            .position(|x| x == identifier)
        {
            self.monitored_processes.remove(pos);
            self.processes.remove(identifier);
        }
    }

    pub fn clear_process_data(&mut self, identifier: &ProcessIdentifier) {
        if let Some(process_data) = self.processes.get_mut(identifier) {
            process_data.history = ProcessHistory::new(self.history_len);
            process_data.processes_stats = vec![];
            process_data.genereal = ProcessGeneral::default();
        }
    }

    pub fn get_monitored_processes(&self) -> &[ProcessIdentifier] {
        &self.monitored_processes
    }

    pub fn get_process_data(&self, identifier: &ProcessIdentifier) -> Option<&ProcessData> {
        self.processes.get(identifier)
    }

    pub fn set_update_interval(&mut self, update_interval_ms: u64) {
        self.update_interval = Duration::from_millis(update_interval_ms);
    }

    fn update_metrics(&mut self) {
        for process_identifier in &self.monitored_processes {
            self.processes
                .entry(process_identifier.clone())
                .or_insert_with(|| ProcessData {
                    history: ProcessHistory::new(self.history_len),
                    genereal: ProcessGeneral {
                        history: ProcessHistory::new(self.history_len),
                        ..Default::default()
                    },
                    ..Default::default()
                });
            if let Some(processes) = self.monitor.find_all_relation(process_identifier) {
                // update history
                if let Some(process_data) = self.processes.get_mut(process_identifier) {
                    // Update history size if it changed
                    if process_data.history.history_len != self.history_len {
                        process_data.history = ProcessHistory::new(self.history_len);
                        process_data.genereal.history = ProcessHistory::new(self.history_len);
                    }
                    // Remove inactive processes from history
                    process_data.history.cleanup_histories(&processes);
                    let mut general_stats = ProcessGeneralStats::default();
                    let mut processes_stats = Vec::with_capacity(processes.len());
                    // Update process data
                    for process_pid in &processes {
                        if let Some(process) = self.monitor.get_process_by_pid(process_pid) {
                            // update history
                            process_data
                                .history
                                .update_cpu(process.pid(), process.cpu_usage());
                            process_data
                                .history
                                .update_memory(process.pid(), process.memory() as usize);
                            // collect process info
                            let process_info = self
                                .monitor
                                .collect_process_info(process, &process_data.history);
                            update_general_stats(&mut general_stats, &process_info);
                            processes_stats.push(process_info);
                        }
                    }
                    // update general history
                    process_data.processes_stats = processes_stats;
                    process_data
                        .genereal
                        .history
                        .update_cpu(*GENERAL_STATS_PID, general_stats.current_cpu);
                    process_data
                        .genereal
                        .history
                        .update_memory(*GENERAL_STATS_PID, general_stats.current_memory);
                    // get general stats
                    let (peak_cpu, peak_memory, avg_cpu, avg_memory) = process_data
                        .genereal
                        .history
                        .get_data_history(&*GENERAL_STATS_PID);
                    general_stats.peak_cpu = peak_cpu;
                    general_stats.peak_memory = peak_memory;
                    general_stats.avg_cpu = avg_cpu;
                    general_stats.avg_memory = avg_memory;
                    process_data.genereal.stats = general_stats;
                }
            } else {
                self.processes.remove(&process_identifier);
            }
        }
    }
}

fn update_general_stats(general_stats: &mut ProcessGeneralStats, process: &ProcessInfo) {
    if process.is_thread {
        general_stats.thread_count += 1;
    } else {
        general_stats.process_count += 1;
        general_stats.current_cpu += process.current_cpu;
        general_stats.current_memory += process.current_memory;
    }
}
