//! Headless `tvis record`: sample one target and write its history to a file.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use crossbeam_channel::{Sender, unbounded};
use tvis_core::Target;
use tvis_core::engine::{Command, Engine, EngineConfig, EngineView, Event, Observer};
use tvis_core::export::ExportFormat;

use crate::app::make_source;
use crate::cli::{Cli, Format, RecordArgs};

struct Done(Sender<Result<std::path::PathBuf, String>>);

impl Observer for Done {
    fn on_update(&mut self, _view: &EngineView<'_>) {}

    fn on_event(&mut self, event: Event) {
        if let Event::ExportFinished { result, .. } = event {
            let _ = self.0.send(result);
        }
    }
}

pub fn run(cli: &Cli, args: &RecordArgs) -> Result<()> {
    let Some(target) = Target::parse(&args.target) else {
        bail!("target must not be empty");
    };
    let interval = Duration::from_millis(cli.interval.unwrap_or(1000));
    let (source, probes) = make_source(cli.demo, !cli.no_gpu);
    let engine = Engine::new(
        source,
        probes,
        EngineConfig {
            interval,
            retention: Duration::from_secs(args.duration + 60),
            paused: false,
        },
    );
    let (tx, rx) = unbounded();
    let handle = engine.spawn(Done(tx));
    let id = target.id();
    handle.send(Command::Watch(target));

    eprintln!(
        "Recording {} for {} s every {} ms…",
        args.target,
        args.duration,
        interval.as_millis()
    );
    let end = Instant::now() + Duration::from_secs(args.duration);
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(200).min(end - Instant::now()));
    }
    handle.send(Command::Export {
        target: id,
        format: match args.format() {
            Format::Csv => ExportFormat::Csv,
            Format::Json => ExportFormat::Json,
        },
        path: args.output.clone(),
    });
    match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(Ok(path)) => {
            println!("{}", path.display());
            Ok(())
        }
        Ok(Err(e)) => bail!("export failed: {e}"),
        Err(_) => bail!("timed out waiting for the export"),
    }
}
