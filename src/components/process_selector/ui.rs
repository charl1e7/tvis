use std::sync::{Arc, RwLock};

use crate::{
    metrics::{process::ProcessIdentifier, Metrics},
    ProcessMonitorApp,
};

use super::state::ProcessSelector;

impl ProcessSelector {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        metrics: Arc<RwLock<Metrics>>,
    ) -> Option<ProcessIdentifier> {
        if !self.show {
            if ui.button("Add Process").clicked() {
                self.show = true;
                self.search.clear();
            }
            return None;
        }

        let mut new_proc = None;

        egui::Window::new("Select Process")
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Search:");
                    let response = ui.text_edit_singleline(&mut self.search);
                    if ui.small_button("❌").clicked() {
                        self.show = false;
                    }
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.show = false;
                    }
                });

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.search_by_pid, false, "By Name");
                    ui.radio_value(&mut self.search_by_pid, true, "By PID");
                });

                ui.separator();

                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        let search_term = self.search.to_lowercase();
                        if self.search_by_pid {
                            // Search by PID
                            {
                                let monitor = &metrics.read().unwrap().monitor;
                                if !search_term.is_empty() {
                                    let proc = ProcessIdentifier::from(search_term.as_str());
                                    if let Some(pid) = proc.to_pid() {
                                        if let Some(process) = monitor.get_process_by_pid(pid) {
                                            let display_text =
                                                format!("{} (PID: {})", process.name, pid);
                                            if ui.button(&display_text).clicked() {
                                                new_proc = Some(proc);
                                                self.show = false;
                                            }
                                        }
                                    }
                                }

                                // Show all processes with PIDs
                                for (name, pid) in monitor.get_all_processes_with_pid() {
                                    let display_text = format!("{} (PID: {})", name, pid);
                                    if search_term.is_empty()
                                        || display_text.to_lowercase().contains(&search_term)
                                        || pid.to_string().contains(&search_term)
                                    {
                                        if ui.button(&display_text).clicked() {
                                            new_proc = Some(ProcessIdentifier::Pid(pid));
                                            self.show = false;
                                        }
                                    }
                                }
                            }
                        } else {
                            // Original search by name
                            let monitor = &metrics.read().unwrap().monitor;
                            let processes = monitor.get_all_processes();
                            for process_name in processes {
                                if search_term.is_empty()
                                    || process_name.to_lowercase().contains(&search_term)
                                {
                                    if ui.button(&process_name).clicked() {
                                        let identifier = ProcessIdentifier::Name(process_name);
                                        new_proc = Some(identifier);
                                        self.show = false;
                                    }
                                }
                            }
                        }
                    });
            });

        new_proc
    }
}
