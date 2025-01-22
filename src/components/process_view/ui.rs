use crate::process::{ProcessStats, ProcessHistory, SortType, MetricType};
use crate::components::stats_view;
use crate::components::settings::Settings;

pub fn show_process(
    ui: &mut egui::Ui,
    name: &str,
    stats: &ProcessStats,
    history: &ProcessHistory,
    process_idx: usize,
    sort_type: &mut SortType,
    settings: &Settings,
) {
    ui.group(|ui| {
        ui.heading(name);
        
        stats_view::show_process_stats(ui, stats);

        // Metric toggle button
        static mut CURRENT_METRIC: MetricType = MetricType::Cpu;
        ui.horizontal(|ui| {
            if ui.selectable_label(unsafe { CURRENT_METRIC == MetricType::Cpu }, "CPU").clicked() {
                unsafe { CURRENT_METRIC = MetricType::Cpu; }
            }
            if ui.selectable_label(unsafe { CURRENT_METRIC == MetricType::Memory }, "Memory").clicked() {
                unsafe { CURRENT_METRIC = MetricType::Memory; }
            }
        });

        // Plot based on selected metric
        match unsafe { CURRENT_METRIC } {
            MetricType::Cpu => {
                if let Some(cpu_history) = history.get_process_cpu_history(process_idx) {
                    if !cpu_history.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(format!("CPU Usage: {:.1}%", cpu_history.last().unwrap()));
                            ui.label(" | ");
                            ui.label(format!("Peak: {:.1}%", stats.peak_cpu));
                        });
                        plot_metric(ui, format!("cpu_plot_{}", process_idx), 100.0, cpu_history, history.history_max_points, stats.peak_cpu * (1.0 + settings.graph_scale_margin));
                    }
                }
            }
            MetricType::Memory => {
                if let Some(memory_history) = history.get_memory_history(process_idx) {
                    if !memory_history.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(format!("Memory Usage: {:.1} MB", memory_history.last().unwrap()));
                            ui.label(" | ");
                            ui.label(format!("Peak: {:.1} MB", stats.peak_memory_mb));
                        });
                        plot_metric(ui, format!("memory_plot_{}", process_idx), 100.0, memory_history, history.history_max_points, stats.peak_memory_mb * (1.0 + settings.graph_scale_margin));
                    }
                }
            }
        }

        // Child Processes
        if !stats.child_processes.is_empty() {
            ui.collapsing("Child Processes", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Sort by:");
                    if ui.selectable_label(*sort_type == SortType::AvgCpu, "Average CPU").clicked() {
                        *sort_type = SortType::AvgCpu;
                    }
                    if ui.selectable_label(*sort_type == SortType::Memory, "Memory").clicked() {
                        *sort_type = SortType::Memory;
                    }
                });

                let mut child_processes = stats.child_processes.clone();
                
                // Sort children based on selected criteria
                match sort_type {
                    SortType::AvgCpu => {
                        // Cache average CPU values
                        let mut processes_with_avg: Vec<_> = child_processes
                            .iter()
                            .map(|p| {
                                let avg = history.get_child_cpu_history(&p.pid)
                                    .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                                    .unwrap_or(0.0);
                                (p, avg)
                            })
                            .collect();
                        
                        // Sort using cached values
                        processes_with_avg.sort_by(|(_, a_avg), (_, b_avg)| {
                            b_avg.partial_cmp(a_avg).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        
                        // Update original vector
                        child_processes = processes_with_avg.into_iter()
                            .map(|(p, _)| p.clone())
                            .collect();
                    }
                    SortType::Memory => {
                        child_processes.sort_by(|a, b| {
                            b.memory_mb.partial_cmp(&a.memory_mb).unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                }

                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for child in child_processes {
                            ui.group(|ui| {
                                let avg_cpu = history.get_child_cpu_history(&child.pid)
                                    .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                                    .unwrap_or(0.0);
                                
                                ui.heading(&child.name);
                                ui.label(format!("PID: {} (Avg CPU: {:.1}%)", child.pid, avg_cpu));
                                
                                match unsafe { CURRENT_METRIC } {
                                    MetricType::Cpu => {
                                        ui.horizontal(|ui| {
                                            ui.label(format!("Current CPU: {:.1}%", child.cpu_usage));
                                            ui.label(" | ");
                                            if let Some(cpu_history) = history.get_child_cpu_history(&child.pid) {
                                                ui.label(format!("Peak: {:.1}%", cpu_history.iter().copied().fold(0.0, f32::max)));
                                            }
                                        });
                                        if let Some(cpu_history) = history.get_child_cpu_history(&child.pid) {
                                            let max_cpu = cpu_history.iter().copied().fold(0.0, f32::max);
                                            plot_metric(
                                                ui,
                                                format!("child_cpu_plot_{}_{}", process_idx, child.pid),
                                                80.0,
                                                cpu_history,
                                                history.history_max_points,
                                                max_cpu * (1.0 + settings.graph_scale_margin),
                                            );
                                        }
                                    }
                                    MetricType::Memory => {
                                        ui.horizontal(|ui| {
                                            ui.label(format!("Memory Usage: {:.1} MB", child.memory_mb));
                                            ui.label(" | ");
                                            if let Some(memory_history) = history.get_child_memory_history(&child.pid) {
                                                ui.label(format!("Peak: {:.1} MB", memory_history.iter().copied().fold(0.0, f32::max)));
                                            }
                                        });
                                        if let Some(memory_history) = history.get_child_memory_history(&child.pid) {
                                            let max_memory = memory_history.iter().copied().fold(0.0, f32::max);
                                            plot_metric(
                                                ui,
                                                format!("child_memory_plot_{}_{}", process_idx, child.pid),
                                                80.0,
                                                memory_history,
                                                history.history_max_points,
                                                max_memory * (1.0 + settings.graph_scale_margin),
                                            );
                                        }
                                    }
                                }
                            });
                        }
                    });
            });
        }
    });
}

fn plot_metric(ui: &mut egui::Ui, id: impl std::hash::Hash, height: f32, history: Vec<f32>, max_points: usize, max_value: f32) {
    let plot = egui_plot::Plot::new(id)
        .height(height)
        .show_axes(true)
        .set_margin_fraction(egui::Vec2::splat(0.005))
        .include_x(0.0)
        .include_x(max_points as f64)
        .include_y(0.0)
        .include_y(max_value as f64)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false);

    plot.show(ui, |plot_ui| {
        let start_x = (max_points - history.len()) as f64;
        let points: Vec<[f64; 2]> = history.iter().enumerate()
            .map(|(i, &y)| [start_x + i as f64, y as f64])
            .collect();
        
        plot_ui.line(egui_plot::Line::new(points).width(2.0));
    });
} 