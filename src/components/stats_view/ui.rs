use crate::metrics::process::ProcessGeneralStats;

pub fn show_process_stats(ui: &mut egui::Ui, stats: &ProcessGeneralStats) {
    ui.horizontal(|ui| {
        // ui.vertical(|ui| {
        //     ui.label(format!("Average CPU: {:.1}%", stats.avg_cpu));
        //     ui.label(format!("Total Memory: {:.1} MB", stats.current_memory));
        // });

        // ui.add_space(32.0);

        ui.vertical(|ui| {
            ui.label(format!("Total Processes: {}", stats.process_count));
            ui.label(format!("Total Threads: {}", stats.thread_count));
        });
    });
    ui.add_space(8.0);
}
