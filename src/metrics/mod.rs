use log::info;
pub mod process;
use process::{ProcessHistory, ProcessIdentifier, ProcessInfo, ProcessMonitor};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};


#[derive(Debug, Clone, Default)]
pub struct Metrics {
    monitored_processes: Vec<ProcessIdentifier>,
    history: ProcessHistory,
    update_interval: Duration,
    history_len: usize,
}

impl Metrics {
    pub fn new(update_interval_ms: u64, history_len: usize) -> Arc<RwLock<Self>> {
        let metrics = Arc::new(RwLock::new(Self {
            update_interval: Duration::from_millis(update_interval_ms),
            history_len,
            history: ProcessHistory::new(history_len),
            ..Default::default()    
        }));

        let metrics_clone = Arc::clone(&metrics);
        thread::spawn(move || {
            loop {
                let monitor = ProcessMonitor::new(Duration::from_millis(update_interval_ms));
                let mut metrics = metrics_clone.write().unwrap();
                // Update history size if it changed
                if metrics.history.history_len != metrics.history_len {
                    metrics.history = ProcessHistory::new(metrics.history_len);
                }

                metrics.update_metrics(&monitor);
                
                info!("Updated Metrics: {:#?}", metrics);
                thread::sleep(metrics.update_interval);
            }
        });

        metrics
    }

    pub fn add_selected_process(&mut self, identifier: ProcessIdentifier) {
        if !self.monitored_processes.contains(&identifier) {
            self.monitored_processes.push(identifier);
        }
    }

    pub fn remove_selected_process(&mut self, identifier: &ProcessIdentifier) {
        if let Some(pos) = self
            .monitored_processes
            .iter()
            .position(|x| x == identifier)
        {
            self.monitored_processes.remove(pos);
        }
    }

    pub fn get_monitored_processes(&self) -> &[ProcessIdentifier] {
        &self.monitored_processes
    }

    pub fn set_update_interval(&mut self, update_interval_ms: u64) {
        self.update_interval = Duration::from_millis(update_interval_ms);
    }

    fn update_metrics(&mut self, monitor: &ProcessMonitor) {
        let mut all_active_pids = Vec::new();
        
        for (i, process_identifier) in self.monitored_processes.iter().enumerate() {
            if let Some(stats) = monitor.get_basic_stats(&process_identifier) {
                all_active_pids.extend(stats.processes.iter().map(|process| {
                    self.history.update_process_cpu(i, process.pid, process.cpu_usage);
                    self.history.update_memory(i, process.pid, process.memory_mb);
                    process.pid
                }));
                
                // Cleanup old processes that are no longer active
                self.history.cleanup_histories(i, &all_active_pids);
            }
        }
    }
}
