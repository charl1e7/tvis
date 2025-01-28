#[derive(serde::Deserialize, serde::Serialize, Clone, Copy, PartialEq)]
pub enum MemoryUnit {
    Bytes,
    Kilobytes,
    Megabytes,
    Gigabytes,
}

impl MemoryUnit {
    pub fn format_value(&self, bytes: f32) -> (f32, &'static str) {
        match self {
            MemoryUnit::Bytes => (bytes, "B"),
            MemoryUnit::Kilobytes => (bytes / 1024.0, "KB"),
            MemoryUnit::Megabytes => (bytes / (1024.0 * 1024.0), "MB"),
            MemoryUnit::Gigabytes => (bytes / (1024.0 * 1024.0 * 1024.0), "GB"),
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Settings {
    pub scale: f32,
    pub font_size: f32,
    pub graph_scale_margin: f32,
    pub update_interval_ms: usize,
    pub history_length: usize,
    pub memory_unit: MemoryUnit,
    #[serde(skip)]
    show_window: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scale: 1.2,
            font_size: 15.0,
            graph_scale_margin: 0.35,
            update_interval_ms: 1000,
            history_length: 100,
            memory_unit: MemoryUnit::Megabytes,
            show_window: false,
        }
    }
}

impl Settings {
    pub fn show(&mut self) {
        self.show_window = true;
    }

    pub fn is_visible(&self) -> bool {
        self.show_window
    }

    pub fn hide(&mut self) {
        self.show_window = false;
    }

    pub fn apply(&self, ctx: &egui::Context) {
        ctx.set_pixels_per_point(self.scale);

        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (
                egui::TextStyle::Heading,
                egui::FontId::new(self.font_size + 4.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Body,
                egui::FontId::new(self.font_size, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Monospace,
                egui::FontId::new(self.font_size, egui::FontFamily::Monospace),
            ),
            (
                egui::TextStyle::Button,
                egui::FontId::new(self.font_size, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Small,
                egui::FontId::new(self.font_size - 2.0, egui::FontFamily::Proportional),
            ),
        ]
        .into();
        ctx.set_style(style);
    }

    pub fn toggle_theme(&self, ctx: &egui::Context) {
        let visuals = if ctx.style().visuals.dark_mode {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        ctx.set_visuals(visuals);
    }
}
