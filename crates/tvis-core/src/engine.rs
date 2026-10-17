//! The sampling engine: owns the source, store and alerts on one thread and
//! publishes results to an [`Observer`].
//!
//! Nothing here knows about any UI. A front end sends [`Command`]s through an
//! [`EngineHandle`] and renders whatever the [`Observer`] is given.

use std::collections::HashSet;
use std::fs;
use std::io::BufWriter;
use std::path::PathBuf;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};

use crate::alert::{AlertEngine, AlertEvent, AlertRule};
use crate::export::{ExportFormat, write_target};
use crate::frame::{Frame, SystemInfo};
use crate::metric::MetricRegistry;
use crate::source::{Probe, ProbeContext, ProcessSource};
use crate::store::{Limits, Store};
use crate::target::{Target, TargetId};

/// Shortest and longest accepted sampling interval.
pub const MIN_INTERVAL: Duration = Duration::from_millis(100);
pub const MAX_INTERVAL: Duration = Duration::from_secs(60);
/// Delay before the first real sample (CPU usage needs two readings).
const WARMUP: Duration = Duration::from_millis(300);

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub interval: Duration,
    pub retention: Duration,
    pub paused: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            retention: Duration::from_secs(3600),
            paused: false,
        }
    }
}

/// Requests a front end can make.
#[derive(Debug)]
pub enum Command {
    Watch(Target),
    Unwatch(TargetId),
    /// Forget the recorded history of one target, or of all when `None`.
    ClearHistory(Option<TargetId>),
    SetInterval(Duration),
    SetRetention(Duration),
    SetPaused(bool),
    SetAlertRules(Vec<AlertRule>),
    Export {
        target: TargetId,
        format: ExportFormat,
        path: PathBuf,
    },
    /// Re-publish the current state (e.g. after the UI changed its selection).
    Refresh,
    Shutdown,
}

#[derive(Debug)]
pub enum Event {
    Alert(AlertEvent),
    ExportFinished {
        target: TargetId,
        result: Result<PathBuf, String>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpdateReason {
    Tick,
    Command,
}

/// Read-only view handed to the observer after every update.
pub struct EngineView<'a> {
    pub reason: UpdateReason,
    pub store: &'a Store,
    /// The most recent frame, `None` before the first sample.
    pub frame: Option<&'a Frame>,
    pub registry: &'a MetricRegistry,
    pub system: &'a SystemInfo,
    pub alerts: &'a AlertEngine,
    pub config: &'a EngineConfig,
    /// Seconds since the engine started (the clock of every series).
    pub now: f64,
}

pub trait Observer: Send + 'static {
    fn on_update(&mut self, view: &EngineView<'_>);
    fn on_event(&mut self, _event: Event) {}
}

pub struct Engine {
    source: Box<dyn ProcessSource>,
    probes: Vec<Box<dyn Probe>>,
    registry: MetricRegistry,
    system: SystemInfo,
    store: Store,
    alerts: AlertEngine,
    config: EngineConfig,
    started: Instant,
    start_wall_ms: u64,
    last_frame: Option<Frame>,
    /// Frames supplied by [`ProcessSource::backfill`]; replayed into targets added later.
    history: Vec<Frame>,
}

fn clamp_interval(d: Duration) -> Duration {
    d.clamp(MIN_INTERVAL, MAX_INTERVAL)
}

fn wall_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Engine {
    pub fn new(
        source: Box<dyn ProcessSource>,
        probes: Vec<Box<dyn Probe>>,
        mut config: EngineConfig,
    ) -> Self {
        config.interval = clamp_interval(config.interval);
        let mut registry = MetricRegistry::new();
        registry.extend(source.descriptors());
        for p in &probes {
            registry.extend(p.descriptors());
        }
        let system = source.info();
        let store = Store::new(Limits {
            retention_secs: config.retention.as_secs_f64(),
            interval_secs: config.interval.as_secs_f64(),
        });
        Self {
            source,
            probes,
            registry,
            system,
            store,
            alerts: AlertEngine::default(),
            config,
            started: Instant::now(),
            start_wall_ms: wall_now_ms(),
            last_frame: None,
            history: Vec::new(),
        }
    }

    pub fn registry(&self) -> &MetricRegistry {
        &self.registry
    }

    pub fn system_info(&self) -> &SystemInfo {
        &self.system
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Takes one sample and feeds store and alerts. Returns alert transitions.
    fn tick(&mut self) -> Vec<AlertEvent> {
        let mut frame = self.source.sample();
        frame.t = self.now();
        frame.wall_ms = self.start_wall_ms + (frame.t * 1000.0) as u64;

        let watched: Vec<_> = {
            let mut seen = HashSet::new();
            self.store
                .targets()
                .iter()
                .flat_map(|t| t.target.resolve(&frame))
                .filter(|p| seen.insert(*p))
                .collect()
        };
        let ctx = ProbeContext { watched: &watched };
        for probe in &mut self.probes {
            probe.probe(&ctx, &mut frame);
        }

        self.store.ingest(&frame, &self.registry);
        let events = self.alerts.evaluate(&self.store, &self.registry, frame.t);
        self.last_frame = Some(frame);
        events
    }

    fn limits(&self) -> Limits {
        Limits {
            retention_secs: self.config.retention.as_secs_f64(),
            interval_secs: self.config.interval.as_secs_f64(),
        }
    }

    /// Applies a command. Returns `false` when the engine should stop.
    fn apply(&mut self, cmd: Command, observer: &mut dyn Observer) -> bool {
        match cmd {
            Command::Watch(t) => {
                let id = t.id();
                // Show something immediately instead of waiting for the next tick.
                if self.store.watch(t) {
                    for frame in self
                        .history
                        .iter()
                        .chain(self.last_frame.iter().filter(|_| self.history.is_empty()))
                    {
                        self.store.ingest_target(&id, frame, &self.registry);
                    }
                }
            }
            Command::Unwatch(id) => {
                self.store.unwatch(&id);
            }
            Command::ClearHistory(Some(id)) => {
                if let Some(t) = self.store.target_mut(&id) {
                    t.clear();
                }
            }
            Command::ClearHistory(None) => {
                let ids: Vec<_> = self.store.targets().iter().map(|t| t.id()).collect();
                for id in ids {
                    if let Some(t) = self.store.target_mut(&id) {
                        t.clear();
                    }
                }
            }
            Command::SetInterval(d) => {
                self.config.interval = clamp_interval(d);
                self.store.set_limits(self.limits());
            }
            Command::SetRetention(d) => {
                self.config.retention = d.max(Duration::from_secs(10));
                self.store.set_limits(self.limits());
            }
            Command::SetPaused(p) => self.config.paused = p,
            Command::SetAlertRules(rules) => self.alerts.set_rules(rules),
            Command::Export {
                target,
                format,
                path,
            } => {
                let result = self.export(&target, format, &path);
                observer.on_event(Event::ExportFinished { target, result });
            }
            Command::Refresh => {}
            Command::Shutdown => return false,
        }
        true
    }

    fn export(
        &self,
        id: &TargetId,
        format: ExportFormat,
        path: &PathBuf,
    ) -> Result<PathBuf, String> {
        let data = self
            .store
            .target(id)
            .ok_or_else(|| format!("{id} is not watched"))?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let file = fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut w = BufWriter::new(file);
        write_target(&mut w, data, &self.registry, format, self.start_wall_ms)
            .and_then(|()| std::io::Write::flush(&mut w))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path.clone())
    }

    fn publish(&self, reason: UpdateReason, observer: &mut dyn Observer) {
        observer.on_update(&EngineView {
            reason,
            store: &self.store,
            frame: self.last_frame.as_ref(),
            registry: &self.registry,
            system: &self.system,
            alerts: &self.alerts,
            config: &self.config,
            now: self.now(),
        });
    }

    /// Runs the engine on a dedicated thread.
    pub fn spawn(self, observer: impl Observer) -> EngineHandle {
        let (tx, rx) = unbounded();
        let join = thread::Builder::new()
            .name("tvis-engine".into())
            .spawn(move || self.run(rx, Box::new(observer)))
            .expect("failed to spawn engine thread");
        EngineHandle {
            tx,
            join: Some(join),
        }
    }

    fn run(mut self, rx: Receiver<Command>, mut observer: Box<dyn Observer>) {
        // Replay history that predates the engine (demo / imported sources).
        for mut frame in self.source.backfill() {
            frame.wall_ms = self
                .start_wall_ms
                .saturating_add_signed((frame.t * 1000.0) as i64);
            self.store.ingest(&frame, &self.registry);
            self.history.push(frame);
        }
        self.last_frame = self.history.last().cloned();
        self.publish(UpdateReason::Command, observer.as_mut());

        let mut next_tick = Instant::now() + WARMUP;
        loop {
            let wait = next_tick.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(first) => {
                    let mut keep_going = self.apply(first, observer.as_mut());
                    while keep_going {
                        match rx.try_recv() {
                            Ok(cmd) => keep_going = self.apply(cmd, observer.as_mut()),
                            Err(_) => break,
                        }
                    }
                    if !keep_going {
                        return;
                    }
                    next_tick = next_tick.min(Instant::now() + self.config.interval);
                    self.publish(UpdateReason::Command, observer.as_mut());
                }
                Err(RecvTimeoutError::Timeout) => {
                    let now = Instant::now();
                    next_tick = (next_tick + self.config.interval).max(now);
                    if !self.config.paused {
                        for event in self.tick() {
                            observer.on_event(Event::Alert(event));
                        }
                        self.publish(UpdateReason::Tick, observer.as_mut());
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// Handle to a running engine; dropping it stops the thread.
pub struct EngineHandle {
    tx: Sender<Command>,
    join: Option<JoinHandle<()>>,
}

impl EngineHandle {
    /// Queues a command. Silently ignored once the engine has stopped.
    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    pub fn sender(&self) -> Sender<Command> {
        self.tx.clone()
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::metric::builtin;
    use crate::source::SyntheticSource;

    #[derive(Default, Clone)]
    struct Probe1 {
        updates: Arc<Mutex<Vec<(UpdateReason, usize, Option<f64>)>>>,
        events: Arc<Mutex<Vec<String>>>,
    }

    impl Observer for Probe1 {
        fn on_update(&mut self, view: &EngineView<'_>) {
            let t = view.store.targets().first();
            let n = t.map_or(0, |t| t.series(builtin::CPU).map_or(0, |s| s.len()));
            self.updates.lock().unwrap().push((
                view.reason,
                n,
                t.and_then(|t| t.current(builtin::CPU)),
            ));
        }

        fn on_event(&mut self, event: Event) {
            let s = match event {
                Event::Alert(_) => "alert".to_owned(),
                Event::ExportFinished { result, .. } => format!("export:{}", result.is_ok()),
            };
            self.events.lock().unwrap().push(s);
        }
    }

    fn fast_engine() -> Engine {
        Engine::new(
            Box::new(SyntheticSource::new(20)),
            vec![],
            EngineConfig {
                interval: MIN_INTERVAL,
                ..Default::default()
            },
        )
    }

    fn wait_for(mut cond: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !cond() {
            assert!(Instant::now() < end, "timed out");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn watch_backfills_samples_and_exports() {
        let probe = Probe1::default();
        let engine = fast_engine();
        let handle = engine.spawn(probe.clone());
        handle.send(Command::Watch(Target::Name("api-server".into())));
        wait_for(|| {
            probe
                .updates
                .lock()
                .unwrap()
                .iter()
                .any(|(r, n, _)| *r == UpdateReason::Tick && *n >= 21)
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/out.csv");
        handle.send(Command::Export {
            target: Target::Name("api-server".into()).id(),
            format: ExportFormat::Csv,
            path: path.clone(),
        });
        wait_for(|| {
            probe
                .events
                .lock()
                .unwrap()
                .iter()
                .any(|e| e == "export:true")
        });
        let csv = fs::read_to_string(&path).unwrap();
        assert!(csv.lines().count() >= 4);
        assert!(csv.starts_with("time_unix_ms,t_seconds,cpu"));
        drop(handle);
    }

    #[test]
    fn pause_stops_ticks_and_resume_continues() {
        let probe = Probe1::default();
        let handle = fast_engine().spawn(probe.clone());
        handle.send(Command::Watch(Target::Name("nginx".into())));
        wait_for(|| {
            probe
                .updates
                .lock()
                .unwrap()
                .iter()
                .any(|(r, ..)| *r == UpdateReason::Tick)
        });
        handle.send(Command::SetPaused(true));
        thread::sleep(Duration::from_millis(250));
        let paused_at = probe.updates.lock().unwrap().len();
        thread::sleep(Duration::from_millis(400));
        assert_eq!(probe.updates.lock().unwrap().len(), paused_at);
        handle.send(Command::SetPaused(false));
        wait_for(|| probe.updates.lock().unwrap().len() > paused_at + 2);
    }

    #[test]
    fn unwatch_removes_target_and_drop_joins_thread() {
        let probe = Probe1::default();
        let handle = fast_engine().spawn(probe.clone());
        handle.send(Command::Watch(Target::Name("nginx".into())));
        wait_for(|| probe.updates.lock().unwrap().iter().any(|(_, n, _)| *n > 0));
        handle.send(Command::Unwatch(Target::Name("nginx".into()).id()));
        wait_for(|| {
            probe
                .updates
                .lock()
                .unwrap()
                .last()
                .is_some_and(|(_, n, _)| *n == 0)
        });
        drop(handle);
    }
}
