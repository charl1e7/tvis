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

        if !stats.processes.is_empty() {
            ui.collapsing("Processes", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Sort by:");
                    if ui.selectable_label(*sort_type == SortType::AvgCpu, "Average CPU").clicked() {
                        *sort_type = SortType::AvgCpu;
                    }
                    if ui.selectable_label(*sort_type == SortType::Memory, "Memory").clicked() {
                        *sort_type = SortType::Memory;
                    }
                });

                let mut processes = stats.processes.clone();
                
                match sort_type {
                    SortType::AvgCpu => {
                        let mut processes_with_avg: Vec<_> = processes
                            .iter()
                            .map(|p| {
                                let avg = history.get_child_cpu_history(&p.pid)
                                    .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                                    .unwrap_or(0.0);
                                (p, avg)
                            })
                            .collect();
                        
                        processes_with_avg.sort_by(|(_, a_avg), (_, b_avg)| {
                            b_avg.partial_cmp(a_avg).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        
                        processes = processes_with_avg.into_iter()
                            .map(|(p, _)| p.clone())
                            .collect();
                    }
                    SortType::Memory => {
                        processes.sort_by(|a, b| {
                            b.memory_mb.partial_cmp(&a.memory_mb).unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                }

                static mut SCROLL_TO_PID: Option<sysinfo::Pid> = None;
                let scroll_area_id = ui.make_persistent_id("processes_scroll_area");
                let scroll = egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .id_source(scroll_area_id);

                scroll.show(ui, |ui| {
                    for process in &processes {
                        let process_id = ui.make_persistent_id(format!("process_{}", process.pid));
                        let response = ui.group(|ui| {
                            let avg_cpu = history.get_child_cpu_history(&process.pid)
                                .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                                .unwrap_or(0.0);
                            
                            ui.heading(&process.name);
                            ui.horizontal(|ui| {
                                ui.label(format!("PID: {}", process.pid));
                                ui.label(" | ");
                                if let Some(parent_pid) = process.parent_pid {
                                    if ui.link(format!("Parent PID: {}", parent_pid)).clicked() {
                                        unsafe { SCROLL_TO_PID = Some(parent_pid); }
                                    }
                                } else {
                                    ui.label("Parent PID: None");
                                }
                            });
                            ui.label(format!("Avg CPU: {:.1}%", avg_cpu));
                            
                            match unsafe { CURRENT_METRIC } {
                                MetricType::Cpu => {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("Current CPU: {:.1}%", process.cpu_usage));
                                        ui.label(" | ");
                                        if let Some(cpu_history) = history.get_child_cpu_history(&process.pid) {
                                            ui.label(format!("Peak: {:.1}%", cpu_history.iter().copied().fold(0.0, f32::max)));
                                        }
                                    });
                                    if let Some(cpu_history) = history.get_child_cpu_history(&process.pid) {
                                        let max_cpu = cpu_history.iter().copied().fold(0.0, f32::max);
                                        plot_metric(
                                            ui,
                                            format!("child_cpu_plot_{}_{}", process_idx, process.pid),
                                            80.0,
                                            cpu_history,
                                            history.history_max_points,
                                            max_cpu * (1.0 + settings.graph_scale_margin),
                                        );
                                    }
                                }
                                MetricType::Memory => {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("Memory Usage: {:.1} MB", process.memory_mb));
                                        ui.label(" | ");
                                        if let Some(memory_history) = history.get_child_memory_history(&process.pid) {
                                            ui.label(format!("Peak: {:.1} MB", memory_history.iter().copied().fold(0.0, f32::max)));
                                        }
                                    });
                                    if let Some(memory_history) = history.get_child_memory_history(&process.pid) {
                                        let max_memory = memory_history.iter().copied().fold(0.0, f32::max);
                                        plot_metric(
                                            ui,
                                            format!("child_memory_plot_{}_{}", process_idx, process.pid),
                                            80.0,
                                            memory_history,
                                            history.history_max_points,
                                            max_memory * (1.0 + settings.graph_scale_margin),
                                        );
                                    }
                                }
                            }
                        });

                        // Если этот процесс тот, к которому нужно прокрутить
                        unsafe {
                            if let Some(scroll_to_pid) = SCROLL_TO_PID {
                                if process.pid == scroll_to_pid {
                                    ui.scroll_to_rect(response.response.rect, Some(egui::Align::Center));
                                    SCROLL_TO_PID = None;
                                }
                            }
                        }
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