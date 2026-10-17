//! Threshold alerts evaluated against the [`Store`].

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::metric::{MetricId, MetricRegistry};
use crate::store::Store;
use crate::target::TargetId;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Comparison {
    Above,
    Below,
}

impl Comparison {
    fn holds(self, value: f64, threshold: f64) -> bool {
        match self {
            Self::Above => value > threshold,
            Self::Below => value < threshold,
        }
    }
}

/// "Fire when `metric` of `target` stays `comparison` `threshold` for `for_secs`."
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: u64,
    pub name: String,
    /// `None` applies the rule to every watched target.
    pub target: Option<String>,
    /// Textual [`MetricId`], resolved through the registry.
    pub metric: String,
    pub comparison: Comparison,
    /// In the metric's base unit (percent, bytes, bytes/s, count).
    pub threshold: f64,
    pub for_secs: f64,
    pub enabled: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub enum AlertEvent {
    Fired(AlertNotice),
    Resolved(AlertNotice),
}

#[derive(Clone, PartialEq, Debug)]
pub struct AlertNotice {
    pub rule_id: u64,
    pub rule_name: String,
    pub target: TargetId,
    pub target_title: String,
    pub metric: MetricId,
    pub value: f64,
    pub threshold: f64,
    pub comparison: Comparison,
}

#[derive(Default, Debug)]
struct State {
    since: Option<f64>,
    firing: bool,
}

#[derive(Default, Debug)]
pub struct AlertEngine {
    rules: Vec<AlertRule>,
    state: HashMap<(u64, TargetId), State>,
}

impl AlertEngine {
    pub fn set_rules(&mut self, rules: Vec<AlertRule>) {
        self.rules = rules;
        let ids: Vec<u64> = self.rules.iter().map(|r| r.id).collect();
        self.state.retain(|(id, _), _| ids.contains(id));
    }

    pub fn rules(&self) -> &[AlertRule] {
        &self.rules
    }

    /// `(rule id, target)` pairs that are firing right now.
    pub fn active(&self) -> Vec<(u64, TargetId)> {
        let mut v: Vec<_> = self
            .state
            .iter()
            .filter(|(_, s)| s.firing)
            .map(|(k, _)| k.clone())
            .collect();
        v.sort();
        v
    }

    /// Evaluates every rule at time `now` and returns state transitions.
    pub fn evaluate(&mut self, store: &Store, registry: &MetricRegistry, now: f64) -> Vec<AlertEvent> {
        let mut events = Vec::new();
        let mut seen: Vec<(u64, TargetId)> = Vec::new();
        for rule in self.rules.iter().filter(|r| r.enabled) {
            let Some(desc) = registry.find(&rule.metric) else { continue };
            for data in store.targets() {
                let id = data.id();
                if rule.target.as_deref().is_some_and(|t| t != id.as_str()) {
                    continue;
                }
                let key = (rule.id, id.clone());
                seen.push(key.clone());
                let state = self.state.entry(key).or_default();
                let notice = |value| AlertNotice {
                    rule_id: rule.id,
                    rule_name: rule.name.clone(),
                    target: id.clone(),
                    target_title: data.title(),
                    metric: desc.id,
                    value,
                    threshold: rule.threshold,
                    comparison: rule.comparison,
                };
                match data.current(desc.id) {
                    Some(value) if rule.comparison.holds(value, rule.threshold) => {
                        let since = *state.since.get_or_insert(now);
                        if !state.firing && now - since >= rule.for_secs {
                            state.firing = true;
                            events.push(AlertEvent::Fired(notice(value)));
                        }
                    }
                    other => {
                        state.since = None;
                        if state.firing {
                            state.firing = false;
                            events.push(AlertEvent::Resolved(notice(other.unwrap_or(0.0))));
                        }
                    }
                }
            }
        }
        // Forget state of removed targets and disabled rules.
        self.state.retain(|k, _| seen.contains(k));
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, ProcessSample, SystemSample};
    use crate::metric::{builtin, builtin_descriptors};
    use crate::store::Limits;
    use crate::target::Target;

    fn rule(for_secs: f64) -> AlertRule {
        AlertRule {
            id: 1,
            name: "hot".into(),
            target: None,
            metric: "cpu".into(),
            comparison: Comparison::Above,
            threshold: 80.0,
            for_secs,
            enabled: true,
        }
    }

    fn step(store: &mut Store, reg: &MetricRegistry, t: f64, cpu: f64) {
        let mut p = ProcessSample::new(1, None, "app");
        p.set(builtin::CPU, cpu);
        let mut f = Frame::new(SystemSample::default(), vec![p]);
        f.t = t;
        store.ingest(&f, reg);
    }

    #[test]
    fn fires_after_duration_and_resolves_once() {
        let mut reg = MetricRegistry::new();
        reg.extend(builtin_descriptors());
        let mut store = Store::new(Limits { retention_secs: 600.0, interval_secs: 1.0 });
        store.watch(Target::Name("app".into()));
        let mut alerts = AlertEngine::default();
        alerts.set_rules(vec![rule(3.0)]);

        let mut log = Vec::new();
        for (t, cpu) in [(0.0, 90.0), (1.0, 90.0), (2.0, 90.0), (3.0, 90.0), (4.0, 95.0), (5.0, 10.0), (6.0, 10.0)] {
            step(&mut store, &reg, t, cpu);
            log.extend(alerts.evaluate(&store, &reg, t));
        }
        assert_eq!(log.len(), 2, "{log:?}");
        assert!(matches!(&log[0], AlertEvent::Fired(n) if n.value == 90.0));
        assert!(matches!(log[1], AlertEvent::Resolved(_)));
        assert!(alerts.active().is_empty());
    }

    #[test]
    fn short_spike_does_not_fire_and_scope_filters() {
        let mut reg = MetricRegistry::new();
        reg.extend(builtin_descriptors());
        let mut store = Store::new(Limits { retention_secs: 600.0, interval_secs: 1.0 });
        store.watch(Target::Name("app".into()));
        let mut alerts = AlertEngine::default();
        let mut scoped = rule(0.0);
        scoped.target = Some("name:other".into());
        alerts.set_rules(vec![rule(5.0), scoped]);
        let mut fired = 0;
        for (t, cpu) in [(0.0, 99.0), (1.0, 99.0), (2.0, 5.0), (3.0, 99.0)] {
            step(&mut store, &reg, t, cpu);
            fired += alerts.evaluate(&store, &reg, t).len();
        }
        assert_eq!(fired, 0);
    }

    #[test]
    fn unknown_metric_is_ignored() {
        let mut reg = MetricRegistry::new();
        reg.extend(builtin_descriptors());
        let mut store = Store::new(Limits { retention_secs: 600.0, interval_secs: 1.0 });
        store.watch(Target::Name("app".into()));
        let mut alerts = AlertEngine::default();
        let mut r = rule(0.0);
        r.metric = "gpu".into();
        alerts.set_rules(vec![r]);
        step(&mut store, &reg, 0.0, 99.0);
        assert!(alerts.evaluate(&store, &reg, 0.0).is_empty());
    }
}
