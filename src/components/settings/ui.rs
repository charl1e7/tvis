use crate::metrics::Metrics;
use std::sync::{Arc, RwLock};
use super::state::{MemoryUnit, Settings};

pub fn show_settings_window(ctx: &egui::Context, settings: &mut Settings, metrics: Arc<RwLock<Metrics>>) {
    if !settings.is_visible() {
        return;
    }

    egui::Window::new("⚙ Settings")
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("UI Scale:");
                ui.add(egui::Slider::new(&mut settings.scale, 0.5..=2.0).step_by(0.1));
            });

            ui.horizontal(|ui| {
                ui.label("Font Size:");
                ui.add(egui::Slider::new(&mut settings.font_size, 8.0..=32.0).step_by(1.0));
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Graph Scale Margin:");
                ui.add(
                    egui::Slider::new(&mut settings.graph_scale_margin, 0.0..=0.5)
                        .step_by(0.01)
                        .suffix("%")
                        .text("Extra margin above peak"),
                );
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Update Interval:");
                let response = ui.add(
                    egui::Slider::new(&mut settings.update_interval_ms, 200..=5000)
                        .step_by(100.0)
                        .suffix(" ms")
                        .text("Time between updates"),
                );
                if response.changed() {
                    if let Ok(mut metrics) = metrics.write() {
                        metrics.set_update_interval(settings.update_interval_ms as u64);
                    }
                }
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("History Length:");
                let response = ui.add(
                    egui::Slider::new(&mut settings.history_length, 10..=1000)
                        .step_by(10.0)
                        .suffix(" points")
                        .text("Number of data points in graphs"),
                );
                if response.changed() {
                    if let Ok(mut metrics) = metrics.write() {
                        metrics.history_len = settings.history_length;
                    }
                }
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Theme:");
                let dark_mode = ui.ctx().style().visuals.dark_mode;
                if ui
                    .button(if dark_mode { "🌞 Light" } else { "🌙 Dark" })
                    .clicked()
                {
                    settings.toggle_theme(ctx);
                }
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Memory Unit:");
                for unit in [MemoryUnit::Bytes, MemoryUnit::Kilobytes, MemoryUnit::Megabytes, MemoryUnit::Gigabytes] {
                    let label = match unit {
                        MemoryUnit::Bytes => "Bytes",
                        MemoryUnit::Kilobytes => "KB",
                        MemoryUnit::Megabytes => "MB",
                        MemoryUnit::Gigabytes => "GB",
                    };
                    if ui.selectable_label(settings.memory_unit == unit, label).clicked() {
                        settings.memory_unit = unit;
                    }
                }
            });

            ui.separator();

            if ui.button("Close").clicked() {
                settings.hide();
            }
        });
}
