use crate::process::ProcessStats;

pub fn show_process_stats(ui: &mut egui::Ui, stats: &ProcessStats) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(format!("Average CPU: {:.1}%", stats.avg_cpu));
            ui.label(format!("Total Memory: {:.1} MB", stats.memory_mb));
        });

        ui.add_space(32.0);

        ui.vertical(|ui| {
            let process_count = stats.processes.iter().filter(|p| !p.is_thread).count();
            ui.label(format!("Total Processes: {}", process_count));
            ui.label(format!("Total Threads: {}", stats.thread_count));
        });
    });
    ui.add_space(8.0);
}
