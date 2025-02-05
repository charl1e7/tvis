use std::sync::{Arc, RwLock};

use sysinfo::Pid;

use crate::components::process_view::state::ProcessView;
use crate::components::settings::Settings;
use crate::metrics::process::{MetricType, ProcessData, ProcessIdentifier, SortType};
use crate::metrics::{Metrics, GENERAL_STATS_PID};
use crate::ProcessMonitorApp;

impl ProcessView {
    pub fn show_process(
        &mut self,
        ui: &mut egui::Ui,
        process_identifier: &ProcessIdentifier,
        process_data: &ProcessData,
        settings: &Settings,
    ) {
        ui.group(|ui| {
            ui.heading(process_identifier.to_string());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(format!(
                        "Total Processes: {}",
                        process_data.genereal.stats.process_count
                    ));
                    ui.label(format!(
                        "Total Threads: {}",
                        process_data.genereal.stats.thread_count
                    ));
                });
            });
            ui.add_space(8.0);
            // Metric toggle button
            ui.horizontal(|ui| {
                egui::Frame::none()
                    .rounding(5.0)
                    .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(self.current_metric == MetricType::Cpu, "CPU")
                                .clicked()
                            {
                                self.current_metric = MetricType::Cpu;
                            }
                            if ui
                                .selectable_label(
                                    self.current_metric == MetricType::Memory,
                                    "Memory",
                                )
                                .clicked()
                            {
                                self.current_metric = MetricType::Memory;
                            }
                        });
                    });
            });
            ui.add_space(3.0);
            // Plot based on general metric
            match self.current_metric {
                MetricType::Cpu => {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "CPU Usage: {:.1}%",
                            process_data.genereal.stats.current_cpu
                        ));
                        ui.label(" | ");
                        ui.label(format!(
                            "Peak: {:.1}%",
                            process_data.genereal.stats.peak_cpu
                        ));
                        ui.label(" | ");
                        ui.label(format!(
                            "AVG CPU: {:.1}%",
                            process_data.genereal.stats.avg_cpu
                        ));
                    });
                    ui.add_space(2.0);
                    plot_metric(
                        ui,
                        "cpu_plot_general_process",
                        100.0,
                        process_data
                            .genereal
                            .history
                            .get_cpu_history(&*GENERAL_STATS_PID)
                            .unwrap_or_default(),
                        process_data.genereal.history.history_len,
                        process_data.genereal.stats.peak_cpu * (1.0 + settings.graph_scale_margin),
                    );
                }
                MetricType::Memory => {
                    ui.horizontal(|ui| {
                        let (current_memory, unit) = settings
                            .memory_unit
                            .format_value(process_data.genereal.stats.current_memory as f32);
                        let (peak_memory, _) = settings
                            .memory_unit
                            .format_value(process_data.genereal.stats.peak_memory as f32);
                        let (avg_memory, _) = settings
                            .memory_unit
                            .format_value(process_data.genereal.stats.avg_memory as f32);

                        ui.label(format!("Memory Usage: {:.1} {}", current_memory, unit));
                        ui.label(" | ");
                        ui.label(format!("Peak: {:.1} {}", peak_memory, unit));
                        ui.label(" | ");
                        ui.label(format!("AVG memory: {:.1} {}", avg_memory, unit));
                    });
                    let history = process_data
                        .genereal
                        .history
                        .get_memory_history(&*GENERAL_STATS_PID)
                        .unwrap_or_default();
                    let history: Vec<f32> = history
                        .iter()
                        .map(|&x| settings.memory_unit.format_value(x as f32).0)
                        .collect();
                    let peak_memory = settings
                        .memory_unit
                        .format_value(process_data.genereal.stats.peak_memory as f32)
                        .0;
                    plot_metric(
                        ui,
                        "memory_plot_general_process",
                        100.0,
                        history,
                        process_data.genereal.history.history_len,
                        peak_memory * (1.0 + settings.graph_scale_margin),
                    );
                }
            }

            if !process_data.processes_stats.is_empty() {
                ui.collapsing("Processes", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Sort by:");
                        if ui
                            .selectable_label(self.sort_type == SortType::AvgCpu, "Average CPU")
                            .clicked()
                        {
                            self.sort_type = SortType::AvgCpu;
                        }
                        if ui
                            .selectable_label(self.sort_type == SortType::Memory, "Memory")
                            .clicked()
                        {
                            self.sort_type = SortType::Memory;
                        }
                    });

                    let mut processes = process_data.processes_stats.iter().collect::<Vec<_>>();

                    match self.sort_type {
                        SortType::AvgCpu => {
                            processes.sort_by(|&a, &b| {
                                let a_avg = process_data
                                    .history
                                    .get_cpu_history(&a.pid)
                                    .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                                    .unwrap_or(0.0);
                                let b_avg = process_data
                                    .history
                                    .get_cpu_history(&b.pid)
                                    .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                                    .unwrap_or(0.0);
                                b_avg
                                    .partial_cmp(&a_avg)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            });
                        }
                        SortType::Memory => {
                            processes.sort_by(|&a, &b| {
                                b.current_memory
                                    .partial_cmp(&a.current_memory)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            });
                        }
                    }

                    let scroll_area_id = ui.make_persistent_id("processes_scroll_area");
                    let scroll = egui::ScrollArea::vertical()
                        .max_height(500.0)
                        .id_salt(scroll_area_id);

                    scroll.show(ui, |ui| {
                        for process in processes {
                            let response = ui.group(|ui| {
                                if process.is_thread {
                                    ui.heading(&format!("{} (Thread)", process.name));
                                } else {
                                    ui.heading(&process.name);
                                }
                                ui.horizontal(|ui| {
                                    ui.label(format!("PID: {}", process.pid));
                                    ui.label(" | ");
                                    if let Some(parent_pid) = process.parent_pid {
                                        let parent_exists = process_data
                                            .processes_stats
                                            .iter()
                                            .any(|p| p.pid == parent_pid);

                                        if parent_exists {
                                            if ui.link(format!("Parent: {}", parent_pid)).clicked()
                                            {
                                                self.scroll_target =
                                                    Some(ProcessIdentifier::Pid(parent_pid));
                                            }
                                        } else {
                                            ui.label(format!("Parent: {}", parent_pid));
                                        }
                                    } else {
                                        ui.label("Parent: None");
                                    }
                                });

                                match self.current_metric {
                                    MetricType::Cpu => {
                                        ui.horizontal(|ui| {
                                            ui.label(format!(
                                                "Current CPU: {:.1}%",
                                                process.current_cpu
                                            ));
                                            ui.label(" | ");
                                            ui.label(format!("Peak: {:.1}%", process.peak_cpu));
                                            ui.label(" | ");
                                            ui.label(format!("Avg CPU: {:.1}%", process.avg_cpu));
                                        });
                                        ui.add_space(2.0);
                                        if let Some(cpu_history) =
                                            process_data.history.get_cpu_history(&process.pid)
                                        {
                                            let max_cpu =
                                                cpu_history.iter().copied().fold(0.0, f32::max);
                                            plot_metric(
                                                ui,
                                                format!("cpu_plot_{}", process.pid),
                                                80.0,
                                                cpu_history.clone(),
                                                process_data.history.history_len,
                                                max_cpu * (1.0 + settings.graph_scale_margin),
                                            );
                                        }
                                    }
                                    MetricType::Memory => {
                                        ui.horizontal(|ui| {
                                            let (current_memory, unit) = settings
                                                .memory_unit
                                                .format_value(process.current_memory as f32);
                                            let (peak_memory, _) = settings
                                                .memory_unit
                                                .format_value(process.peak_memory as f32);
                                            let (avg_memory, _) = settings
                                                .memory_unit
                                                .format_value(process.avg_memory as f32);

                                            ui.label(format!(
                                                "Memory Usage: {:.1} {}",
                                                current_memory, unit
                                            ));
                                            ui.label(" | ");
                                            ui.label(format!("Peak: {:.1} {}", peak_memory, unit));
                                            ui.label(" | ");
                                            ui.label(format!(
                                                "AVG memory: {:.1} {}",
                                                avg_memory, unit
                                            ));
                                        });
                                        ui.add_space(5.0);
                                        if let Some(memory_history) =
                                            process_data.history.get_memory_history(&process.pid)
                                        {
                                            let memory_history: Vec<f32> = memory_history
                                                .iter()
                                                .map(|&x| {
                                                    settings.memory_unit.format_value(x as f32).0
                                                })
                                                .collect();
                                            let max_memory =
                                                memory_history.iter().copied().fold(0.0, f32::max);
                                            plot_metric(
                                                ui,
                                                format!("child_memory_plot_{}", process.pid),
                                                80.0,
                                                memory_history,
                                                process_data.history.history_len,
                                                max_memory * (1.0 + settings.graph_scale_margin),
                                            );
                                        }
                                    }
                                }
                            });

                            // Check if we need to scroll to this process
                            if let Some(target_pid) = &self.scroll_target {
                                if process.pid == target_pid.to_pid().unwrap() {
                                    ui.scroll_to_rect(
                                        response.response.rect,
                                        Some(egui::Align::Center),
                                    );
                                    self.scroll_target = None;
                                }
                            }
                        }
                    });
                });
            }
        });
    }
}
fn plot_metric<T>(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    height: f32,
    history: Vec<T>,
    max_points: usize,
    max_value: T,
) where
    T: Into<f64> + Copy,
{
    let plot = egui_plot::Plot::new(id)
        .height(height)
        .show_axes(true)
        .set_margin_fraction(egui::Vec2::splat(0.005))
        .include_x(0.0)
        .include_x(max_points as f64)
        .include_y(0.0)
        .include_y(max_value.into())
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false);

    plot.show(ui, |plot_ui| {
        let start_x = (max_points - history.len()) as f64;
        let points: Vec<[f64; 2]> = history
            .iter()
            .enumerate()
            .map(|(i, &y)| [start_x + i as f64, y.into()])
            .collect();

        plot_ui.line(egui_plot::Line::new(points).width(2.0));
    });
}
