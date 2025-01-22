use crate::process::{ProcessStats, ProcessHistory, SortType};
use crate::components::stats_view;

pub fn show_process(
    ui: &mut egui::Ui,
    name: &str,
    stats: &ProcessStats,
    history: &ProcessHistory,
    process_idx: usize,
    sort_type: &mut SortType,
) {
    ui.group(|ui| {
        ui.heading(name);
        
        stats_view::show_process_stats(ui, stats);

        // CPU Usage
        if let Some(cpu_history) = history.get_process_cpu_history(process_idx) {
            if !cpu_history.is_empty() {
                ui.label(format!("CPU Usage: {:.1}%", cpu_history.last().unwrap()));
                cpu_plot(ui, format!("cpu_plot_{}", process_idx), 100.0, cpu_history, history.history_max_points);
            }
        }

        // Memory Usage
        ui.label(format!("Memory Usage: {:.1} MB", stats.memory_mb));

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
                        child_processes.sort_by(|a, b| {
                            let a_avg = history.get_child_cpu_history(&a.pid)
                                .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                                .unwrap_or(0.0);
                            let b_avg = history.get_child_cpu_history(&b.pid)
                                .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                                .unwrap_or(0.0);
                            b_avg.partial_cmp(&a_avg).unwrap_or(std::cmp::Ordering::Equal)
                        });
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
                                ui.label(format!("Current CPU: {:.1}%", child.cpu_usage));

                                if let Some(cpu_history) = history.get_child_cpu_history(&child.pid) {
                                    cpu_plot(
                                        ui,
                                        format!("child_cpu_plot_{}_{}", process_idx, child.pid),
                                        80.0,
                                        cpu_history,
                                        history.history_max_points,
                                    );
                                }

                                ui.label(format!("Memory Usage: {:.1} MB", child.memory_mb));
                            });
                        }
                    });
            });
        }
    });
}

fn cpu_plot(ui: &mut egui::Ui, id: impl std::hash::Hash, height: f32, history: &[f32], max_points: usize) {
    let plot = egui_plot::Plot::new(id)
        .height(height)
        .show_axes(true)
        .set_margin_fraction(egui::Vec2::ZERO)
        .include_x(0.0)
        .include_x(max_points as f64)
        .include_y(0.0)
        .include_y(100.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false);

    plot.show(ui, |plot_ui| {
        let points: egui_plot::PlotPoints = (0..history.len())
            .map(|i| [i as f64, history[i] as f64])
            .collect();
        plot_ui.line(egui_plot::Line::new(points));
    });
} 