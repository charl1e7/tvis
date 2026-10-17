//! Command line interface.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "tvis",
    version,
    about = "Professional process profiler: CPU, memory, threads, I/O and open files over time",
    propagate_version = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Start watching a process by name or PID (repeatable)
    #[arg(short, long = "watch", value_name = "NAME|PID", global = true)]
    pub watch: Vec<String>,

    /// Run against a synthetic machine instead of the real one
    #[arg(long, global = true)]
    pub demo: bool,

    /// Sampling interval in milliseconds (overrides the saved setting)
    #[arg(short, long, value_name = "MS", global = true)]
    pub interval: Option<u64>,

    /// Use a custom configuration directory
    #[arg(long, value_name = "DIR", env = "TVIS_CONFIG_DIR", global = true)]
    pub config_dir: Option<PathBuf>,

    #[command(flatten)]
    pub dev: DevOptions,
}

/// Options used for documentation screenshots and UI development.
#[derive(Args, Debug, Default)]
pub struct DevOptions {
    /// Render the window to a PNG after a delay, then exit
    #[arg(long, value_name = "PNG", hide = true)]
    pub snapshot: Option<PathBuf>,
    /// Page to show for `--snapshot` (0 dashboard … 4 settings)
    #[arg(long, value_name = "N", default_value_t = 0, hide = true)]
    pub snapshot_page: i32,
    /// Milliseconds to wait before taking the snapshot
    #[arg(long, value_name = "MS", default_value_t = 2500, hide = true)]
    pub snapshot_delay: u64,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Record a target without opening a window and write the history to a file
    Record(RecordArgs),
}

#[derive(Args, Debug)]
pub struct RecordArgs {
    /// Process name or PID to record
    pub target: String,

    /// How long to record, in seconds
    #[arg(short, long, default_value_t = 60)]
    pub duration: u64,

    /// Output file (extension decides the format unless --format is given)
    #[arg(short, long, default_value = "tvis-recording.csv")]
    pub output: PathBuf,

    #[arg(short, long, value_enum)]
    pub format: Option<Format>,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Csv,
    Json,
}

impl RecordArgs {
    pub fn format(&self) -> Format {
        self.format.unwrap_or_else(|| {
            match self.output.extension().and_then(|e| e.to_str()) {
                Some("json") => Format::Json,
                _ => Format::Csv,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gui_and_record_forms() {
        let cli = Cli::try_parse_from(["tvis", "-w", "java", "-w", "42", "--demo"]).unwrap();
        assert_eq!(cli.watch, ["java", "42"]);
        assert!(cli.demo && cli.command.is_none());

        let cli = Cli::try_parse_from(["tvis", "record", "nginx", "-d", "5", "-o", "x.json", "--demo"]).unwrap();
        let Some(Command::Record(r)) = cli.command else { panic!() };
        assert_eq!((r.duration, r.format()), (5, Format::Json));
        assert!(cli.demo);
    }

    #[test]
    fn clap_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
