use eframe::wgpu::core::identity;
use log::info;
pub mod process;
use process::{ProcessHistory, ProcessIdentifier, ProcessInfo, ProcessMonitor, ProcessData, ProcessStats};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};


#[derive(Debug, Clone, Default)]
pub struct Metrics {
    monitored_processes: Vec<ProcessIdentifier>,
    processes: HashMap<ProcessIdentifier, ProcessData>,
    update_interval: Duration,
    history_len: usize,
}

impl Metrics {
    pub fn new(update_interval_ms: u64, history_len: usize) -> Arc<RwLock<Self>> {
        let metrics = Arc::new(RwLock::new(Self {
            update_interval: Duration::from_millis(update_interval_ms),
            history_len,
            processes: HashMap::new(),
            ..Default::default()    
        }));

        let metrics_clone = Arc::clone(&metrics);
        thread::spawn(move || {
            loop {
                let monitor = ProcessMonitor::new(Duration::from_millis(update_interval_ms));
                let mut metrics = metrics_clone.write().unwrap();

                metrics.update_metrics(&monitor);
                
                info!("Updated Metrics: {:#?}", metrics);
                thread::sleep(metrics.update_interval);
            }
        });

        metrics
    }

    pub fn add_selected_process(&mut self, identifier: ProcessIdentifier) {
        if !self.monitored_processes.contains(&identifier) {
            self.monitored_processes.push(identifier.clone());
            self.processes.entry(identifier).or_insert_with(|| ProcessData {
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

    pub fn get_monitored_processes(&self) -> &[ProcessIdentifier] {
        &self.monitored_processes
    }

    pub fn set_update_interval(&mut self, update_interval_ms: u64) {
        self.update_interval = Duration::from_millis(update_interval_ms);
    }

    fn update_metrics(&mut self, monitor: &ProcessMonitor) {
        for process_identifier in &self.monitored_processes {
            if let Some(stats) = monitor.get_basic_stats(&process_identifier) {
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
                        process_data.history.update_process_cpu(0, process.pid, process.cpu_usage);
                        process_data.history.update_memory(0, process.pid, process.memory_mb);
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
