use crate::components::process_selector::ProcessSelector;
use crate::components::process_view::{self, state::ProcessView};
use crate::components::settings::{show_settings_window, Settings};
use crate::metrics::process::{MetricType, ProcessIdentifier, SortType};
use crate::metrics::{self, Metrics};
use log::info;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use sysinfo::Pid;

#[derive(serde::Deserialize, serde::Serialize, Default)]
#[serde(default)]
pub struct ProcessMonitorApp {
    #[serde(skip)]
    pub metrics: Arc<RwLock<Metrics>>,
    pub monitored_processes: Vec<ProcessIdentifier>,
    #[serde(skip)]
    pub process_selector: ProcessSelector,
    pub process_view: ProcessView,
    settings: Settings,
    pub active_process: Option<ProcessIdentifier>,
    sort_type: SortType,
    #[serde(skip)]
    scroll_target: Option<Pid>,
    current_metric: MetricType,
}

impl ProcessMonitorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(storage) = cc.storage {
            let mut app: Self = eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
            let metrics =
                Metrics::new(app.settings.history_length, app.settings.update_interval_ms);
            {
                app.metrics = metrics;
                for process in app.monitored_processes.clone() {
                    app.metrics.write().unwrap().add_selected_process(process);
                }
            }
            app
        } else {
            ProcessMonitorApp {
                metrics: Metrics::new(100, 10000),
                ..Default::default()
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

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Menu", |ui| {
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.add_space(16.0);
                if ui.button("⚙").clicked() {
                    self.settings.show();
                }
                ui.add_space(4.0);
                if ui
                    .button("⟲")
                    .on_hover_text("Clear current process data")
                    .clicked()
                {
                    if let Some(identifier) = &self.active_process {
                        let mut metrics = self.metrics.write().unwrap();
                        metrics.clear_process_data(identifier);
                    }
                }
            });
        });

        show_settings_window(ctx, &mut self.settings);

        // let mut to_remove = None;
        egui::SidePanel::left("process_list")
            .resizable(true)
            .min_width(150.0)
            .max_width(800.0)
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.heading("Monitored Processes");
                ui.add_space(4.0);

                // Process selector
                if let Some(proc) = self.process_selector.show(ui, self.metrics.clone()) {
                    self.add_monitored_proc(proc);
                };

                // Process list with remove buttons
                for (i, process) in self.monitored_processes.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let is_active = self.active_process.as_ref() == Some(process);

                        let response = ui.selectable_label(is_active, process.to_string());
                        if response.clicked() {
                            self.active_process = Some(process.clone());
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("❌").clicked() {
                                if self.active_process.as_ref() == Some(process) {
                                    self.active_process = None;
                                }
                                let mut metrics = self.metrics.write().unwrap();
                                metrics.remove_selected_process(process);
                            }
                        });
                    });
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Process Monitor");

            // Display process information
            if let Some(identifier) = &self.active_process {
                let monitored_processes = {
                    self.metrics
                        .read()
                        .unwrap()
                        .get_process_data(identifier)
                        .cloned()
                };
                if let Some(process_data) = monitored_processes {
                    &self
                        .process_view
                        .show_process(ui, &identifier,&process_data, &self.settings);
                } else {
                    ui.group(|ui| {
                        ui.heading(identifier.to_string());
                        ui.label("Process not found");
                    });
                }
            } else if !self.monitored_processes.is_empty() {
                ui.label("Select a process from the list to view details");
            }
        });

        // Change mode rendering
        ctx.request_repaint();
    }
}

impl ProcessMonitorApp {
    pub fn add_monitored_proc(&mut self, proc: ProcessIdentifier) {
        if !self.monitored_processes.contains(&proc) {
            self.monitored_processes.push(proc.clone());
            self.active_process = Some(proc.clone());
            self.metrics.write().unwrap().add_selected_process(proc);
        }
    }
}
