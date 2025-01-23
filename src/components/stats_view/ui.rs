use crate::process::ProcessStats;

pub fn show_process_stats(ui: &mut egui::Ui, stats: &ProcessStats) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(format!("Average CPU: {:.1}%", stats.avg_cpu));
            ui.label(format!("Total Memory: {:.1} MB", stats.memory_mb));
        });

        ui.add_space(32.0);

        ui.vertical(|ui| {
            ui.label(format!("Total Processes: {}", stats.processes.len()));
            ui.label(format!("Total Threads: {}", stats.thread_count));
        });
    });
    ui.add_space(8.0);
}
