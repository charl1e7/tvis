mod process_selector;
pub mod process_view;
mod stats_view;

pub use process_selector::*;
pub use process_view::*;
pub use stats_view::*;

use egui::Vec2;

pub fn cpu_plot(ui: &mut egui::Ui, id: impl std::hash::Hash, height: f32, history: &[f32], max_points: usize) {
    let plot = egui_plot::Plot::new(id)
        .height(height)
        .show_axes(true)
        .set_margin_fraction(Vec2::ZERO)
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