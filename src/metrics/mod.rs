use crate::process::{ProcessHistory, ProcessIdentifier, ProcessStats};
use log::info;
use monitor::ProcessMonitor;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};
mod monitor;

#[derive(Debug, Clone, Default)]
pub struct Metrics {
    monitored_processes: Vec<ProcessIdentifier>,
    history: ProcessHistory,
    update_interval: Duration,
    history_size: usize,
}

impl Metrics {
    pub fn new(update_interval_ms: u64, history_size: usize) -> Arc<RwLock<Self>> {
        let metrics = Arc::new(RwLock::new(Self {
            update_interval: Duration::from_millis(update_interval_ms),
            history_size,
            history: ProcessHistory::new(history_size),
            ..Default::default()    
        }));

        let metrics_clone = Arc::clone(&metrics);
        let monitor = ProcessMonitor::new(Duration::from_millis(update_interval_ms));
        thread::spawn(move || {
            loop {
                let mut metrics = metrics_clone.write().unwrap();
                metrics.update_metrics(&monitor);
                
                info!("Updated Metrics: {:#?}", metrics.history);
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
        let mut all_active_pids = Vec::with_capacity(
            self.monitored_processes
                .iter()
                .map(|identifier| {
                    monitor
                        .get_basic_stats(&identifier)
                        .map(|stats| stats.processes.len())
                        .unwrap_or(0)
                })
                .sum(),
        );
        for (i, process_identifier) in self.monitored_processes.iter().enumerate() {
            if let Some(stats) = monitor.get_basic_stats(&process_identifier) {
                self.history.update_process_cpu(i, stats.current_cpu);
                self.history.update_memory(i, stats.memory_mb);

                all_active_pids.extend(stats.processes.iter().map(|process| {
                    self.history
                        .update_child_cpu(i, process.pid, process.cpu_usage);
                    self.history
                        .update_child_memory(i, process.pid, process.memory_mb);
                    process.pid
                }));
            }
        }
    }
}
