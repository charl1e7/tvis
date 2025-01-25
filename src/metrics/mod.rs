use log::info;
pub mod process;
use process::{
    ProcessData, ProcessHistory, ProcessIdentifier, ProcessInfo, ProcessMonitor, ProcessStats,
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
        thread::spawn(move || loop {
            let mut update_interval = Duration::default();
            {
                let mut metrics_read = metrics_clone.read().unwrap();
                update_interval = metrics_read.update_interval;
                let mut metrics = Metrics {
                    monitored_processes: metrics_read.monitored_processes.clone(),
                    processes: metrics_read.processes.clone(),
                    monitor: ProcessMonitor::new(Duration::from_millis(update_interval_ms as u64)),
                    update_interval: update_interval,
                    history_len: metrics_read.history_len,
                };
                drop(metrics_read);
                metrics.update_metrics();
                let mut metrics_write = metrics_clone.write().unwrap();
                metrics_write.processes = metrics.processes;
                metrics_write.monitor = metrics.monitor;
                drop(metrics_write);
            }
            thread::sleep(update_interval);
        });

        metrics
    }

    pub fn add_selected_process(&mut self, identifier: ProcessIdentifier) {
        if !self.monitored_processes.contains(&identifier) {
            self.monitored_processes.push(identifier.clone());
            self.processes
                .entry(identifier)
                .or_insert_with(|| ProcessData {
                    history: ProcessHistory::new(self.history_len),
                    stats: ProcessStats::default(),
                });
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
            process_data.stats = ProcessStats::default();
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
            if let Some(stats) = self.monitor.get_basic_stats(&process_identifier) {
                info!("stats: {:#?}", stats);
                if let Some(process_data) = self.processes.get_mut(process_identifier) {
                    // Update history size if it changed
                    if process_data.history.history_len != self.history_len {
                        process_data.history = ProcessHistory::new(self.history_len);
                    }

                    // Collect active PIDs
                    let active_pids: Vec<_> = stats.processes.iter().map(|p| p.pid).collect();

                    // Update process data
                    process_data.stats = stats.clone();
                    for process in &stats.processes {
                        process_data
                            .history
                            .update_process_cpu(0, process.pid, process.cpu_usage);
                        process_data
                            .history
                            .update_memory(0, process.pid, process.memory_mb);
                    }

                    // Remove inactive processes from history
                    process_data.history.cleanup_histories(0, &active_pids);
                }
            } else {
                self.processes.remove(process_identifier);
            }
        }
    }
}
