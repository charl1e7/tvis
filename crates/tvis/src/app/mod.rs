//! The graphical application: engine wiring, presenter and Slint glue.

pub mod chart;
pub mod presenter;
pub mod ui;
pub mod view;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use slint::ComponentHandle;
use tvis_core::Target;
use tvis_core::alert::AlertEvent;
use tvis_core::engine::{Command, Engine, EngineConfig, EngineView, Event, Observer};
use tvis_core::metric::MetricRegistry;
use tvis_core::source::{OpenFilesProbe, Probe, ProcessSource, SyntheticSource, SysinfoSource};
use tvis_core::units::{ByteBase, format_value};

use self::ui::{Ui, with_ui};
use self::view::{SharedRequest, ViewRequest, ViewState};
use crate::AppWindow;
use crate::cli::Cli;
use crate::settings::{Settings, SettingsStore};

/// Hands engine output to the UI thread. Only the newest [`ViewState`] is kept:
/// if the UI is busy, stale frames are dropped instead of queueing up.
struct UiObserver {
    window: slint::Weak<AppWindow>,
    request: SharedRequest,
    registry: MetricRegistry,
    latest: Arc<Mutex<Option<ViewState>>>,
    scheduled: Arc<AtomicBool>,
}

impl UiObserver {
    fn post(&self, f: impl FnOnce() + Send + 'static) {
        let _ = self.window.upgrade_in_event_loop(move |_| f());
    }

    fn describe(&self, n: &tvis_core::alert::AlertNotice, fired: bool) -> String {
        let base = self
            .request
            .lock()
            .map_or(ByteBase::Binary, |r| r.byte_base);
        let (name, unit) = self
            .registry
            .get(n.metric)
            .map_or((n.metric.as_str(), tvis_core::metric::Unit::Count), |d| {
                (d.name, d.unit)
            });
        let cmp = match n.comparison {
            tvis_core::alert::Comparison::Above => "above",
            tvis_core::alert::Comparison::Below => "below",
        };
        if fired {
            format!(
                "{} — {}: {name} is {} ({cmp} {})",
                n.rule_name,
                n.target_title,
                format_value(unit, n.value, base),
                format_value(unit, n.threshold, base)
            )
        } else {
            format!(
                "{} — {}: {name} back to normal",
                n.rule_name, n.target_title
            )
        }
    }
}

impl Observer for UiObserver {
    fn on_update(&mut self, view: &EngineView<'_>) {
        let request: ViewRequest = {
            let mut r = self.request.lock().unwrap();
            if r.selected.is_none() {
                r.selected = view.store.targets().first().map(|t| t.id());
            }
            r.clone()
        };
        let state = presenter::build(view, &request);
        *self.latest.lock().unwrap() = Some(state);
        if !self.scheduled.swap(true, Ordering::AcqRel) {
            let latest = self.latest.clone();
            let scheduled = self.scheduled.clone();
            self.post(move || {
                scheduled.store(false, Ordering::Release);
                if let Some(state) = latest.lock().unwrap().take() {
                    with_ui(|ui| ui.apply(state));
                }
            });
        }
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Alert(AlertEvent::Fired(n)) => {
                let text = self.describe(&n, true);
                self.post(move || with_ui(|ui| ui.alert_event(true, text)));
            }
            Event::Alert(AlertEvent::Resolved(n)) => {
                let text = self.describe(&n, false);
                self.post(move || with_ui(|ui| ui.alert_event(false, text)));
            }
            Event::ExportFinished { result, .. } => self.post(move || {
                with_ui(|ui| match result {
                    Ok(path) => ui.toast(1, &format!("Exported to {}", path.display())),
                    Err(e) => ui.toast(2, &format!("Export failed: {e}")),
                })
            }),
        }
    }
}

pub fn make_source(demo: bool) -> (Box<dyn ProcessSource>, Vec<Box<dyn Probe>>) {
    if demo {
        (Box::new(SyntheticSource::new(900)), vec![])
    } else {
        (
            Box::new(SysinfoSource::new()),
            vec![Box::new(OpenFilesProbe::new())],
        )
    }
}

pub fn run(cli: Cli) -> Result<()> {
    let store = match &cli.config_dir {
        Some(dir) => SettingsStore::at(dir),
        None => SettingsStore::default_location(),
    };
    let mut settings: Settings = store.load();
    if let Some(ms) = cli.interval {
        settings.interval_ms = ms;
    }

    let (source, probes) = make_source(cli.demo);
    let engine = Engine::new(
        source,
        probes,
        EngineConfig {
            interval: Duration::from_millis(settings.interval_ms),
            retention: Duration::from_secs(settings.retention_secs),
            paused: false,
        },
    );
    let registry = engine.registry().clone();
    let system = engine.system_info().clone();

    let window = AppWindow::new().context("could not create the window")?;
    let request: SharedRequest = Arc::new(Mutex::new(ViewRequest {
        window_secs: settings.window_secs,
        ..ViewRequest::default()
    }));

    let handle = engine.spawn(UiObserver {
        window: window.as_weak(),
        request: request.clone(),
        registry: registry.clone(),
        latest: Arc::default(),
        scheduled: Arc::default(),
    });

    let engine_info = format!(
        "{} · {} · {} metrics{}",
        system.os,
        if system.cpu_brand.is_empty() {
            format!("{} cores", system.cpu_cores)
        } else {
            system.cpu_brand.clone()
        },
        registry.len(),
        if cli.demo { " · demo data" } else { "" }
    );
    let ui = Ui::install(
        &window,
        request,
        handle.sender(),
        store,
        settings.clone(),
        registry,
        engine_info,
    );

    handle.send(Command::SetAlertRules(settings.alerts.clone()));
    let mut watch: Vec<Target> = settings.watchlist.clone();
    watch.extend(cli.watch.iter().filter_map(|s| Target::parse(s)));
    for target in watch {
        handle.send(Command::Watch(target));
    }
    if let Some(first) = cli.watch.first().and_then(|s| Target::parse(s)) {
        ui.select_target(first.id());
        ui.select_page(1);
    }

    if let Some(path) = cli.dev.snapshot.clone() {
        ui.select_page(cli.dev.snapshot_page);
        if cli.dev.snapshot_height > 0 {
            window.window().set_size(slint::LogicalSize::new(
                1360.0,
                cli.dev.snapshot_height as f32,
            ));
        }
        schedule_snapshot(&window, path, Duration::from_millis(cli.dev.snapshot_delay));
    }

    window.run().context("event loop failed")?;
    ui.flush();
    drop(handle);
    Ok(())
}

/// Saves a screenshot after `delay` and quits; used for docs and UI work.
fn schedule_snapshot(window: &AppWindow, path: std::path::PathBuf, delay: Duration) {
    let weak = window.as_weak();
    slint::Timer::single_shot(delay, move || {
        let result = (|| -> Result<()> {
            let w = weak.upgrade().context("window is gone")?;
            let img = w
                .window()
                .take_snapshot()
                .context("renderer cannot take snapshots")?;
            let file = std::fs::File::create(&path)?;
            let mut enc =
                png::Encoder::new(std::io::BufWriter::new(file), img.width(), img.height());
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()?.write_image_data(img.as_bytes())?;
            Ok(())
        })();
        if let Err(e) = result {
            eprintln!("snapshot failed: {e:#}");
        }
        let _ = slint::quit_event_loop();
    });
}
