use crate::components::process_view::state::ProcessView;
use crate::components::settings::Settings;
use crate::components::stats_view;
use crate::process::{MetricType, SortType};

pub fn show_process(
    ui: &mut egui::Ui,
    name: &str,
    state: &mut ProcessView<'_>,
    settings: &Settings,
) {
    ui.group(|ui| {
        ui.heading(name);

        stats_view::show_process_stats(ui, &state.stats);

        // Metric toggle button
        ui.horizontal(|ui| {
            egui::Frame::none()
                .rounding(5.0)
                .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(*state.current_metric == MetricType::Cpu, "CPU")
                            .clicked()
                        {
                            *state.current_metric = MetricType::Cpu;
                        }
                        if ui
                            .selectable_label(*state.current_metric == MetricType::Memory, "Memory")
                            .clicked()
                        {
                            *state.current_metric = MetricType::Memory;
                        }
                    });
                });
        });
        ui.add_space(3.0);
        // Plot based on selected metric
        match *state.current_metric {
            MetricType::Cpu => {
                if let Some(cpu_history) = state.history.get_process_cpu_history(state.process_idx)
                {
                    if !cpu_history.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(format!("CPU Usage: {:.1}%", cpu_history.last().unwrap()));
                            ui.label(" | ");
                            ui.label(format!("Peak: {:.1}%", state.stats.peak_cpu));
                        });
                        ui.add_space(2.0);
                        plot_metric(
                            ui,
                            format!("cpu_plot_{}", state.process_idx),
                            100.0,
                            cpu_history,
                            state.history.history_max_points,
                            state.stats.peak_cpu * (1.0 + settings.graph_scale_margin),
                        );
                    }
                }
            }
            MetricType::Memory => {
                if let Some(memory_history) = state.history.get_memory_history(state.process_idx) {
                    if !memory_history.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "Memory Usage: {:.1} MB",
                                memory_history.last().unwrap()
                            ));
                            ui.label(" | ");
                            ui.label(format!("Peak: {:.1} MB", state.stats.peak_memory_mb));
                        });
                        plot_metric(
                            ui,
                            format!("memory_plot_{}", state.process_idx),
                            100.0,
                            memory_history,
                            state.history.history_max_points,
                            state.stats.peak_memory_mb * (1.0 + settings.graph_scale_margin),
                        );
                    }
                }
            }
        }

        if !state.stats.processes.is_empty() {
            ui.collapsing("Processes", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Sort by:");
                    if ui
                        .selectable_label(state.sort_type == SortType::AvgCpu, "Average CPU")
                        .clicked()
                    {
                        state.sort_type = SortType::AvgCpu;
                    }
                    if ui
                        .selectable_label(state.sort_type == SortType::Memory, "Memory")
                        .clicked()
                    {
                        state.sort_type = SortType::Memory;
                    }
                });

                let mut processes = state.stats.processes.iter().collect::<Vec<_>>();

                match state.sort_type {
                    SortType::AvgCpu => {
                        processes.sort_by(|&a, &b| {
                            let a_avg = state
                                .history
                                .get_child_cpu_history(state.process_idx, &a.pid)
                                .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                                .unwrap_or(0.0);
                            let b_avg = state
                                .history
                                .get_child_cpu_history(state.process_idx, &b.pid)
                                .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                                .unwrap_or(0.0);
                            b_avg
                                .partial_cmp(&a_avg)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                    SortType::Memory => {
                        processes.sort_by(|&a, &b| {
                            b.memory_mb
                                .partial_cmp(&a.memory_mb)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                }

                let scroll_area_id = ui.make_persistent_id("processes_scroll_area");
                let scroll = egui::ScrollArea::vertical()
                    .max_height(500.0)
                    .id_salt(scroll_area_id);

                scroll.show(ui, |ui| {
                    for process in &processes {
                        let response = ui.group(|ui| {
                            let avg_cpu = state
                                .history
                                .get_child_cpu_history(state.process_idx, &process.pid)
                                .map(|h| h.iter().sum::<f32>() / h.len() as f32)
                                .unwrap_or(0.0);

                            ui.heading(&process.name);
                            ui.horizontal(|ui| {
                                ui.label(format!("PID: {}", process.pid));
                                ui.label(" | ");
                                if let Some(parent_pid) = process.parent_pid {
                                    let parent_exists =
                                        state.stats.processes.iter().any(|p| p.pid == parent_pid);

                                    if parent_exists {
                                        if ui.link(format!("Parent: {}", parent_pid)).clicked()
                                        {
                                            *state.scroll_target = Some(parent_pid);
                                        }
                                    } else {
                                        ui.label(format!("Parent: {}", parent_pid));
                                    }
                                } else {
                                    ui.label("Parent: None");
                                }
                            });

                            match *state.current_metric {
                                MetricType::Cpu => {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("Current CPU: {:.1}%", process.cpu_usage));
                                        ui.label(" | ");
                                        if let Some(cpu_history) = state
                                            .history
                                            .get_child_cpu_history(state.process_idx, &process.pid)
                                        {
                                            ui.label(format!(
                                                "Peak: {:.1}%",
                                                cpu_history.iter().copied().fold(0.0, f32::max)
                                            ));
                                        }
                                        ui.label(" | ");
                                        ui.label(format!("Avg CPU: {:.1}%", avg_cpu));
                                    });
                                    ui.add_space(2.0);
                                    if let Some(cpu_history) = state
                                        .history
                                        .get_child_cpu_history(state.process_idx, &process.pid)
                                    {
                                        let max_cpu =
                                            cpu_history.iter().copied().fold(0.0, f32::max);
                                        plot_metric(
                                            ui,
                                            format!(
                                                "child_cpu_plot_{}_{}",
                                                state.process_idx, process.pid
                                            ),
                                            80.0,
                                            cpu_history,
                                            state.history.history_max_points,
                                            max_cpu * (1.0 + settings.graph_scale_margin),
                                        );
                                    }
                                }
                                MetricType::Memory => {
                                    ui.horizontal(|ui| {
                                        ui.label(format!(
                                            "Memory Usage: {:.1} MB",
                                            process.memory_mb
                                        ));
                                        ui.label(" | ");
                                        if let Some(memory_history) =
                                            state.history.get_child_memory_history(
                                                state.process_idx,
                                                &process.pid,
                                            )
                                        {
                                            ui.label(format!(
                                                "Peak: {:.1} MB",
                                                memory_history.iter().copied().fold(0.0, f32::max)
                                            ));
                                        }
                                    });
                                    ui.add_space(5.0);
                                    if let Some(memory_history) = state
                                        .history
                                        .get_child_memory_history(state.process_idx, &process.pid)
                                    {
                                        let max_memory =
                                            memory_history.iter().copied().fold(0.0, f32::max);
                                        plot_metric(
                                            ui,
                                            format!(
                                                "child_memory_plot_{}_{}",
                                                state.process_idx, process.pid
                                            ),
                                            80.0,
                                            memory_history,
                                            state.history.history_max_points,
                                            max_memory * (1.0 + settings.graph_scale_margin),
                                        );
                                    }
                                }
                            }
                        });

                        // Check if we need to scroll to this process
                        if let Some(target_pid) = *state.scroll_target {
                            if process.pid == target_pid {
                                ui.scroll_to_rect(
                                    response.response.rect,
                                    Some(egui::Align::Center),
                                );
                                *state.scroll_target = None;
                            }
                        }
                    }
                });
            });
        }
    });
}

fn plot_metric(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    height: f32,
    history: Vec<f32>,
    max_points: usize,
    max_value: f32,
) {
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
        let points: Vec<[f64; 2]> = history
            .iter()
            .enumerate()
            .map(|(i, &y)| [start_x + i as f64, y as f64])
            .collect();

        plot_ui.line(egui_plot::Line::new(points).width(2.0));
    });
}
