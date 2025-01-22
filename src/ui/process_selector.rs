use crate::process::ProcessMonitor;

pub struct ProcessSelector {
    pub show: bool,
    pub search: String,
}

impl Default for ProcessSelector {
    fn default() -> Self {
        Self {
            show: false,
            search: String::new(),
        }
    }
}

impl ProcessSelector {
    pub fn show(&mut self, ui: &mut egui::Ui, monitor: &ProcessMonitor, monitored_processes: &mut Vec<String>) {
        if !self.show {
            if ui.button("Add Process").clicked() {
                self.show = true;
                self.search.clear();
            }
            return;
        }

        egui::Window::new("Select Process")
            .collapsible(false)
            .resizable(true)
            .default_size([300.0, 400.0])
            .show(ui.ctx(), |ui| {
                // Search box
                ui.horizontal(|ui| {
                    ui.label("Search:");
                    ui.text_edit_singleline(&mut self.search);
                });
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let search_term = self.search.to_lowercase();
                    let processes = monitor.get_all_processes();

                    for process_name in processes {
                        if search_term.is_empty() || process_name.to_lowercase().contains(&search_term) {
                            if ui.button(&process_name).clicked() {
                                if !monitored_processes.contains(&process_name) {
                                    monitored_processes.push(process_name);
                                }
                                self.show = false;
                            }
                        }
                    }
                });

                if ui.button("Close").clicked() {
                    self.show = false;
                }
            });
    }
} 