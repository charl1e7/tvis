//! In-memory history of everything that is watched.

use std::collections::HashMap;

use crate::frame::{Frame, Pid, ProcessSample};
use crate::metric::{Aggregation, MetricId, MetricRegistry};
use crate::series::TimeSeries;
use crate::stats::Summary;
use crate::target::{Target, TargetId};

/// Hard ceiling on points per series so pathological settings cannot exhaust memory.
const MAX_POINTS: usize = 200_000;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub retention_secs: f64,
    pub interval_secs: f64,
}

impl Limits {
    fn max_points(&self) -> usize {
        ((self.retention_secs / self.interval_secs.max(0.05)) as usize + 8).clamp(16, MAX_POINTS)
    }

    fn series(&self) -> TimeSeries {
        TimeSeries::new(self.retention_secs, self.max_points())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetStatus {
    /// Matching processes were found in the latest frame.
    Running,
    /// Nothing matches right now; the watch stays armed.
    Waiting,
}

/// Recorded history and latest state of one target.
#[derive(Clone, Debug)]
pub struct TargetData {
    pub target: Target,
    pub status: TargetStatus,
    /// Members in the latest frame (roots first, threads included).
    pub members: Vec<ProcessSample>,
    series: HashMap<MetricId, TimeSeries>,
    proc_series: HashMap<(Pid, MetricId), TimeSeries>,
}

impl TargetData {
    fn new(target: Target) -> Self {
        Self {
            target,
            status: TargetStatus::Waiting,
            members: Vec::new(),
            series: HashMap::new(),
            proc_series: HashMap::new(),
        }
    }

    pub fn id(&self) -> TargetId {
        self.target.id()
    }

    /// Display name: the target itself, or the process name for a pid target.
    pub fn title(&self) -> String {
        match (&self.target, self.members.first()) {
            (Target::Pid(pid), Some(p)) => format!("{} · {pid}", p.name),
            _ => self.target.to_string(),
        }
    }

    pub fn series(&self, metric: MetricId) -> Option<&TimeSeries> {
        self.series.get(&metric)
    }

    pub fn process_series(&self, pid: Pid, metric: MetricId) -> Option<&TimeSeries> {
        self.proc_series.get(&(pid, metric))
    }

    /// Latest aggregated value, if the target is running and reports the metric.
    pub fn current(&self, metric: MetricId) -> Option<f64> {
        if self.status != TargetStatus::Running {
            return None;
        }
        self.series.get(&metric)?.last().map(|p| p.v)
    }

    pub fn summary(&self, metric: MetricId, window_secs: f64) -> Option<Summary> {
        Summary::of(&self.series.get(&metric)?.window_values(window_secs))
    }

    pub fn has_metric(&self, metric: MetricId) -> bool {
        self.series.get(&metric).is_some_and(|s| !s.is_empty())
    }

    pub fn process_count(&self) -> usize {
        self.members.iter().filter(|p| !p.is_thread()).count()
    }

    pub fn thread_count(&self) -> usize {
        self.members.iter().filter(|p| p.is_thread()).count()
    }

    pub fn clear(&mut self) {
        self.series.values_mut().for_each(TimeSeries::clear);
        self.proc_series.clear();
    }

    fn ingest(&mut self, frame: &Frame, registry: &MetricRegistry, limits: Limits) {
        let pids = self.target.resolve(frame);
        self.members = pids
            .iter()
            .filter_map(|p| frame.process(*p))
            .cloned()
            .collect();
        if self.members.is_empty() {
            self.status = TargetStatus::Waiting;
            self.proc_series.clear();
            return;
        }
        self.status = TargetStatus::Running;

        for desc in registry.iter() {
            let mut values = self
                .members
                .iter()
                .filter(|p| !p.is_thread())
                .filter_map(|p| p.get(desc.id));
            let Some(first) = values.next() else { continue };
            let (sum, max, n) = values.fold((first, first, 1usize), |(s, m, n), v| {
                (s + v, m.max(v), n + 1)
            });
            let value = match desc.aggregation {
                Aggregation::Sum => sum,
                Aggregation::Max => max,
                Aggregation::Mean => sum / n as f64,
            };
            self.series
                .entry(desc.id)
                .or_insert_with(|| limits.series())
                .push(frame.t, value);

            if desc.per_process_history {
                for p in self.members.iter().filter(|p| !p.is_thread()) {
                    if let Some(v) = p.get(desc.id) {
                        self.proc_series
                            .entry((p.pid, desc.id))
                            .or_insert_with(|| limits.series())
                            .push(frame.t, v);
                    }
                }
            }
        }
        let live: std::collections::HashSet<Pid> = self.members.iter().map(|p| p.pid).collect();
        self.proc_series.retain(|(pid, _), _| live.contains(pid));
    }

    fn apply_limits(&mut self, limits: Limits) {
        let (age, pts) = (limits.retention_secs, limits.max_points());
        self.series
            .values_mut()
            .chain(self.proc_series.values_mut())
            .for_each(|s| s.set_limits(age, pts));
    }
}

/// Machine-wide history shown on the dashboard.
#[derive(Clone, Debug)]
pub struct SystemHistory {
    pub cpu: TimeSeries,
    pub memory: TimeSeries,
    pub swap: TimeSeries,
    pub load: TimeSeries,
}

impl SystemHistory {
    fn new(limits: Limits) -> Self {
        Self {
            cpu: limits.series(),
            memory: limits.series(),
            swap: limits.series(),
            load: limits.series(),
        }
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = &mut TimeSeries> {
        [&mut self.cpu, &mut self.memory, &mut self.swap, &mut self.load].into_iter()
    }
}

#[derive(Clone, Debug)]
pub struct Store {
    limits: Limits,
    targets: Vec<TargetData>,
    pub system: SystemHistory,
}

impl Store {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            targets: Vec::new(),
            system: SystemHistory::new(limits),
        }
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
        self.targets.iter_mut().for_each(|t| t.apply_limits(limits));
        let (age, pts) = (limits.retention_secs, limits.max_points());
        self.system.iter_mut().for_each(|s| s.set_limits(age, pts));
    }

    /// Starts watching a target. Returns `false` if it is already watched.
    pub fn watch(&mut self, target: Target) -> bool {
        if self.targets.iter().any(|t| t.target.id() == target.id()) {
            return false;
        }
        self.targets.push(TargetData::new(target));
        true
    }

    pub fn unwatch(&mut self, id: &TargetId) -> bool {
        let before = self.targets.len();
        self.targets.retain(|t| &t.id() != id);
        self.targets.len() != before
    }

    pub fn targets(&self) -> &[TargetData] {
        &self.targets
    }

    pub fn target(&self, id: &TargetId) -> Option<&TargetData> {
        self.targets.iter().find(|t| &t.id() == id)
    }

    pub fn target_mut(&mut self, id: &TargetId) -> Option<&mut TargetData> {
        self.targets.iter_mut().find(|t| &t.id() == id)
    }

    /// Feeds a single target, e.g. one that was just added between ticks.
    pub fn ingest_target(&mut self, id: &TargetId, frame: &Frame, registry: &MetricRegistry) {
        let limits = self.limits;
        if let Some(t) = self.target_mut(id) {
            t.ingest(frame, registry, limits);
        }
    }

    pub fn ingest(&mut self, frame: &Frame, registry: &MetricRegistry) {
        let s = &frame.system;
        self.system.cpu.push(frame.t, s.cpu_total);
        self.system.memory.push(frame.t, s.mem_used as f64);
        self.system.swap.push(frame.t, s.swap_used as f64);
        self.system.load.push(frame.t, s.load_average[0]);
        let limits = self.limits;
        for t in &mut self.targets {
            t.ingest(frame, registry, limits);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{ProcessKind, SystemSample};
    use crate::metric::{builtin, builtin_descriptors};

    fn registry() -> MetricRegistry {
        let mut r = MetricRegistry::new();
        r.extend(builtin_descriptors());
        r
    }

    fn frame(t: f64, rows: &[(Pid, Option<Pid>, &str, f64, f64, bool)]) -> Frame {
        let procs = rows
            .iter()
            .map(|&(pid, parent, name, cpu, rss, thread)| {
                let mut p = ProcessSample::new(pid, parent, name);
                p.set(builtin::CPU, cpu);
                p.set(builtin::MEM_RSS, rss);
                if thread {
                    p.kind = ProcessKind::Thread;
                }
                p
            })
            .collect();
        let mut f = Frame::new(SystemSample::default(), procs);
        f.t = t;
        f
    }

    fn limits() -> Limits {
        Limits { retention_secs: 600.0, interval_secs: 1.0 }
    }

    #[test]
    fn sums_tree_but_skips_threads() {
        let mut store = Store::new(limits());
        assert!(store.watch(Target::Name("app".into())));
        assert!(!store.watch(Target::Name("APP".into())));
        let f = frame(
            1.0,
            &[
                (1, None, "app", 10.0, 100.0, false),
                (2, Some(1), "child", 5.0, 50.0, false),
                (3, Some(1), "t", 99.0, 999.0, true),
            ],
        );
        store.ingest(&f, &registry());
        let t = &store.targets()[0];
        assert_eq!(t.status, TargetStatus::Running);
        assert_eq!(t.current(builtin::CPU), Some(15.0));
        assert_eq!(t.current(builtin::MEM_RSS), Some(150.0));
        assert_eq!((t.process_count(), t.thread_count()), (2, 1));
        assert!(t.process_series(2, builtin::CPU).is_some());
        assert!(t.process_series(3, builtin::CPU).is_none());
        assert!(!t.has_metric(builtin::THREADS));
    }

    #[test]
    fn missing_target_waits_and_recovers() {
        let mut store = Store::new(limits());
        store.watch(Target::Name("app".into()));
        let reg = registry();
        store.ingest(&frame(1.0, &[(1, None, "app", 1.0, 1.0, false)]), &reg);
        store.ingest(&frame(2.0, &[(9, None, "x", 1.0, 1.0, false)]), &reg);
        let t = &store.targets()[0];
        assert_eq!(t.status, TargetStatus::Waiting);
        assert_eq!(t.current(builtin::CPU), None);
        assert_eq!(t.series(builtin::CPU).unwrap().len(), 1);
        store.ingest(&frame(3.0, &[(5, None, "app", 2.0, 1.0, false)]), &reg);
        assert_eq!(store.targets()[0].current(builtin::CPU), Some(2.0));
    }

    #[test]
    fn dead_process_series_are_pruned() {
        let mut store = Store::new(limits());
        store.watch(Target::Name("app".into()));
        let reg = registry();
        store.ingest(
            &frame(1.0, &[(1, None, "app", 1.0, 1.0, false), (2, Some(1), "c", 1.0, 1.0, false)]),
            &reg,
        );
        store.ingest(&frame(2.0, &[(1, None, "app", 1.0, 1.0, false)]), &reg);
        assert!(store.targets()[0].process_series(2, builtin::CPU).is_none());
        assert!(store.targets()[0].process_series(1, builtin::CPU).is_some());
    }

    #[test]
    fn unwatch_and_clear() {
        let mut store = Store::new(limits());
        let t = Target::Name("app".into());
        store.watch(t.clone());
        store.ingest(&frame(1.0, &[(1, None, "app", 1.0, 1.0, false)]), &registry());
        store.target_mut(&t.id()).unwrap().clear();
        assert!(store.target(&t.id()).unwrap().series(builtin::CPU).unwrap().is_empty());
        assert!(store.unwatch(&t.id()));
        assert!(!store.unwatch(&t.id()));
    }
}
