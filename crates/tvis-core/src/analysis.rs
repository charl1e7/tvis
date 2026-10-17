//! Automatic insights derived from recorded history.

use crate::metric::{MetricId, MetricRegistry, Unit, builtin};
use crate::stats::{Summary, linear_fit};
use crate::store::TargetData;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum InsightKind {
    /// Steady upward drift of a metric that should plateau.
    Growth {
        /// Change per second, in the metric's base unit.
        per_sec: f64,
        /// Goodness of the linear fit in `[0, 1]`.
        confidence: f64,
    },
    /// CPU pinned near (or above) one full core.
    Saturation { mean: f64 },
    /// A single sample far above the typical level.
    Spike { peak: f64, typical: f64 },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Insight {
    pub metric: MetricId,
    pub kind: InsightKind,
    pub severity: Severity,
}

/// Minimum observed span before a trend is trusted.
const MIN_TREND_SPAN_SECS: f64 = 30.0;
const MIN_SAMPLES: usize = 10;

/// Examines the last `window_secs` of `data` and returns findings, most severe first.
pub fn analyze(data: &TargetData, registry: &MetricRegistry, window_secs: f64) -> Vec<Insight> {
    let mut out = Vec::new();
    for desc in registry.iter() {
        let Some(series) = data.series(desc.id) else {
            continue;
        };
        let Some(last) = series.last() else { continue };
        let pts: Vec<_> = series.since(last.t - window_secs).collect();
        if pts.len() < MIN_SAMPLES {
            continue;
        }
        let span = pts[pts.len() - 1].t - pts[0].t;
        let values: Vec<f64> = pts.iter().map(|p| p.v).collect();
        let Some(sum) = Summary::of(&values) else {
            continue;
        };

        if desc.leak_candidate
            && span >= MIN_TREND_SPAN_SECS
            && let Some(fit) = linear_fit(pts.iter().map(|p| (p.t, p.v)))
        {
            let growth = fit.slope * span;
            let floor: f64 = match desc.unit {
                Unit::Bytes => 512.0 * 1024.0,
                _ => 3.0,
            };
            let relative = growth / sum.mean.abs().max(f64::EPSILON);
            if fit.slope > 0.0 && fit.r2 >= 0.85 && growth >= floor.max(0.03 * sum.mean) {
                out.push(Insight {
                    metric: desc.id,
                    kind: InsightKind::Growth {
                        per_sec: fit.slope,
                        confidence: fit.r2,
                    },
                    severity: if relative >= 0.25 {
                        Severity::Critical
                    } else {
                        Severity::Warning
                    },
                });
            }
        }

        if desc.id == builtin::CPU && span >= 15.0 && sum.mean >= 90.0 {
            out.push(Insight {
                metric: desc.id,
                kind: InsightKind::Saturation { mean: sum.mean },
                severity: Severity::Warning,
            });
        }

        if desc.unit == Unit::Percent
            && pts.len() >= 20
            && sum.stddev > 0.0
            && sum.max > sum.mean + 4.0 * sum.stddev
            && sum.max > 1.5 * sum.p95.max(1.0)
        {
            out.push(Insight {
                metric: desc.id,
                kind: InsightKind::Spike {
                    peak: sum.max,
                    typical: sum.p50,
                },
                severity: Severity::Info,
            });
        }
    }
    out.sort_by_key(|i| std::cmp::Reverse(i.severity));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, ProcessSample, SystemSample};
    use crate::metric::builtin_descriptors;
    use crate::store::{Limits, Store};
    use crate::target::Target;

    fn run(f: impl Fn(usize) -> (f64, f64), n: usize) -> (Store, MetricRegistry) {
        let mut reg = MetricRegistry::new();
        reg.extend(builtin_descriptors());
        let mut store = Store::new(Limits {
            retention_secs: 3600.0,
            interval_secs: 1.0,
        });
        store.watch(Target::Name("app".into()));
        for i in 0..n {
            let (cpu, rss) = f(i);
            let mut p = ProcessSample::new(1, None, "app");
            p.set(builtin::CPU, cpu);
            p.set(builtin::MEM_RSS, rss);
            let mut frame = Frame::new(SystemSample::default(), vec![p]);
            frame.t = i as f64;
            store.ingest(&frame, &reg);
        }
        (store, reg)
    }

    #[test]
    fn detects_steady_memory_growth() {
        let (s, r) = run(|i| (10.0, 100e6 + i as f64 * 1e6), 120);
        let found = analyze(&s.targets()[0], &r, 600.0);
        let g = found
            .iter()
            .find(|i| i.metric == builtin::MEM_RSS)
            .expect("growth");
        match g.kind {
            InsightKind::Growth {
                per_sec,
                confidence,
            } => {
                assert!((per_sec - 1e6).abs() < 1e3);
                assert!(confidence > 0.99);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn noisy_flat_memory_is_not_a_leak() {
        let (s, r) = run(|i| (10.0, 100e6 + ((i * 7919) % 13) as f64 * 1e5), 120);
        assert!(
            analyze(&s.targets()[0], &r, 600.0)
                .iter()
                .all(|i| i.metric != builtin::MEM_RSS)
        );
    }

    #[test]
    fn too_little_data_yields_nothing() {
        let (s, r) = run(|i| (100.0, 1e6 * i as f64), 5);
        assert!(analyze(&s.targets()[0], &r, 600.0).is_empty());
    }

    #[test]
    fn detects_saturation_and_spike() {
        let (s, r) = run(|_| (99.0, 1e8), 60);
        assert!(
            analyze(&s.targets()[0], &r, 600.0)
                .iter()
                .any(|i| matches!(i.kind, InsightKind::Saturation { .. }))
        );
        let (s, r) = run(
            |i| (if i == 50 { 400.0 } else { 5.0 + (i % 3) as f64 }, 1e8),
            60,
        );
        assert!(
            analyze(&s.targets()[0], &r, 600.0)
                .iter()
                .any(|i| matches!(i.kind, InsightKind::Spike { .. }))
        );
    }
}
