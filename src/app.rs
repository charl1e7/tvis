use std::time::Duration;
use crate::process::{ProcessMonitor, ProcessHistory};
use crate::ui::{ProcessSelector, process_view};
use crate::settings::{Settings, show_settings_window};

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct ProcessMonitorApp {
    #[serde(skip)]
    monitor: ProcessMonitor,
    #[serde(skip)]
    history: ProcessHistory,
    monitored_processes: Vec<String>,
    #[serde(skip)]
    process_selector: ProcessSelector,
    settings: Settings,
    active_process_idx: Option<usize>,
}

impl Default for ProcessMonitorApp {
    fn default() -> Self {
        Self {
            monitor: ProcessMonitor::new(Duration::from_millis(1000)),
            history: ProcessHistory::new(100),
            monitored_processes: Vec::new(),
            process_selector: ProcessSelector::default(),
            settings: Settings::default(),
            active_process_idx: None,
        }
    }
}

impl ProcessMonitorApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }

        Default::default()
    }

    fn update_metrics(&mut self) {
        if !self.monitor.should_update() {
            return;
        }

        self.monitor.update();

        // Update histories for monitored processes
        for (i, process_name) in self.monitored_processes.iter().enumerate() {
            if let Some(stats) = self.monitor.get_process_stats(process_name) {
                self.history.update_process_cpu(i, stats.current_cpu);
                
                // Update child process histories
                for child in &stats.child_processes {
                    self.history.update_child_cpu(child.pid, child.cpu_usage);
                }

                // Cleanup old child histories
                let active_pids: Vec<_> = stats.child_processes.iter().map(|p| p.pid).collect();
                self.history.cleanup_child_histories(&active_pids);
            }
        }
    }
}

impl eframe::App for ProcessMonitorApp {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.settings.apply(ctx);
        self.update_metrics();

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                
                ui.add_space(16.0);
                egui::widgets::global_theme_preference_buttons(ui);
                
                ui.add_space(16.0);
                if ui.button("⚙").clicked() {
                    self.settings.show();
                }
            });
        });

        show_settings_window(ctx, &mut self.settings);

        egui::SidePanel::left("process_list").show(ctx, |ui| {
            ui.heading("Monitored Processes");
            
            // Process selector
            if let Some(added_idx) = self.process_selector.show(ui, &self.monitor, &mut self.monitored_processes) {
                self.active_process_idx = Some(added_idx);
            }
            
            // Process list with remove buttons
            let mut to_remove = None;
            for (i, process) in self.monitored_processes.iter().enumerate() {
                ui.horizontal(|ui| {
                    let is_active = self.active_process_idx == Some(i);
                    if ui.selectable_label(is_active, process).clicked() {
                        self.active_process_idx = Some(i);
                    }
                    if ui.small_button("❌").clicked() {
                        to_remove = Some(i);
                        if self.active_process_idx == Some(i) {
                            self.active_process_idx = None;
                        }
                    }
                });
            }
            
            if let Some(idx) = to_remove {
                self.monitored_processes.remove(idx);
                self.history.remove_process(idx);
                // Adjust active_process_idx if needed
                if let Some(active_idx) = self.active_process_idx {
                    if active_idx > idx {
                        self.active_process_idx = Some(active_idx - 1);
                    } else if active_idx == idx {
                        self.active_process_idx = None;
                    }
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Process Monitor");

            // Display process information
            if let Some(idx) = self.active_process_idx {
                if let Some(process_name) = self.monitored_processes.get(idx) {
                    if let Some(stats) = self.monitor.get_process_stats(process_name) {
                        process_view::show_process(ui, process_name, &stats, &self.history, idx);
                    } else {
                        ui.group(|ui| {
                            ui.heading(process_name);
                            ui.label("Process not found");
                        });
                    }
                }
            } else if !self.monitored_processes.is_empty() {
                ui.label("Select a process from the list to view details");
            }
        });

        // Request repaint
        ctx.request_repaint();
    }
}

fn powered_by_egui_and_eframe(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label("Powered by ");
        ui.hyperlink_to("egui", "https://github.com/emilk/egui");
        ui.label(" and ");
        ui.hyperlink_to(
            "eframe",
            "https://github.com/emilk/egui/tree/master/crates/eframe",
        );
        ui.label(".");
    });
}
