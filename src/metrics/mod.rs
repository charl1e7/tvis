use log::info;
pub mod process;
use process::{
    ProcessData, ProcessGeneralStats, ProcessHistory, ProcessIdentifier, ProcessInfo,
    ProcessMonitor,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};

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
            process_data.genereal = ProcessGeneralStats::default();
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
                    ..Default::default()
                });
            if let Some(processes) = self.monitor.find_all_relation(process_identifier) {
                // update history
                if let Some(process_data) = self.processes.get_mut(process_identifier) {
                    // Update history size if it changed
                    if process_data.history.history_len != self.history_len {
                        process_data.history = ProcessHistory::new(self.history_len);
                    }
                    // Remove inactive processes from history
                    process_data.history.cleanup_histories(&processes);
                    let mut processes_stats = Vec::with_capacity(processes.len());
                    // Update process data
                    for process_pid in &processes {
                        if let Some(process) = self.monitor.get_process(process_pid) {
                            let process_info = self.monitor.collect_process_info(process);
                            process_data.history.update_process_cpu(
                                0,
                                process_info.pid,
                                process_info.cpu_usage,
                            );
                            process_data.history.update_memory(
                                0,
                                process_info.pid,
                                process_info.memory_mb,
                            );
                            processes_stats.push(process_info);
                        }
                    }
                    process_data.processes_stats = processes_stats;
                }
            } else {
                self.processes.remove(&process_identifier);
            }
        }
    }
}
