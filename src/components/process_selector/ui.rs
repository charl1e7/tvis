use super::state::ProcessSelector;
use crate::process::ProcessMonitor;

impl ProcessSelector {
    pub fn show(&mut self, ui: &mut egui::Ui, monitor: &ProcessMonitor, monitored_processes: &mut Vec<String>) -> Option<usize> {
        if !self.show {
            if ui.button("Add Process").clicked() {
                self.show = true;
                self.search.clear();
            }
            return None;
        }

        let mut added_idx = None;

        egui::Window::new("Select Process")
            .collapsible(false)
            .resizable(true)
            .default_size([300.0, 400.0])
            .min_width(250.0)
            .max_height(500.0)
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
                ui.separator();

                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        let search_term = self.search.to_lowercase();
                        let processes = monitor.get_all_processes();

                        for process_name in processes {
                            if search_term.is_empty() || process_name.to_lowercase().contains(&search_term) {
                                if ui.button(&process_name).clicked() {
                                    if !monitored_processes.contains(&process_name) {
                                        monitored_processes.push(process_name);
                                        added_idx = Some(monitored_processes.len() - 1);
                                    }
                                    self.show = false;
                                }
                            }
                        }
                    });
            });

        added_idx
    }
} 