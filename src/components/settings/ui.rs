use super::state::Settings;

pub fn show_settings_window(ctx: &egui::Context, settings: &mut Settings) {
    if !settings.is_visible() {
        return;
    }

    egui::Window::new("⚙ Settings")
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("UI Scale:");
                ui.add(
                    egui::Slider::new(&mut settings.scale, 0.5..=2.0)
                        .step_by(0.1)
                );
            });

            ui.horizontal(|ui| {
                ui.label("Font Size:");
                ui.add(
                    egui::Slider::new(&mut settings.font_size, 8.0..=32.0)
                        .step_by(1.0)
                );
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Theme:");
                let dark_mode = ui.ctx().style().visuals.dark_mode;
                if ui.button(if dark_mode { "🌞 Light" } else { "🌙 Dark" }).clicked() {
                    settings.toggle_theme(ctx);
                }
            });

            ui.separator();

            if ui.button("Close").clicked() {
                settings.hide();
            }
        });
} 