use crate::process::ProcessStats;

pub fn show_process_stats(ui: &mut egui::Ui, stats: &ProcessStats) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(format!("Total Memory (with children): {:.1} MB", 
                stats.memory_mb + stats.children_memory_mb));
            ui.label(format!("Current CPU (with children): {:.1}%", 
                stats.current_cpu + stats.children_current_cpu));
            ui.label(format!("Average CPU: {:.1}% (main) + {:.1}% (children) = {:.1}%", 
                stats.avg_cpu, stats.children_avg_cpu, stats.avg_cpu + stats.children_avg_cpu));
            ui.label(format!("Child Processes: {}", stats.child_processes.len()));
        });
    });
    ui.add_space(8.0);
} 