//! Metric identifiers and descriptors.

use std::fmt;

/// Stable identifier of a metric, e.g. `"cpu"` or `"mem.rss"`.
///
/// Identifiers are `'static` strings so they are free to copy and compare;
/// third-party probes simply declare their own constants.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MetricId(&'static str);

impl MetricId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Debug for MetricId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MetricId({})", self.0)
    }
}

impl fmt::Display for MetricId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Physical unit of a metric; drives formatting and axis ticks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    /// CPU style percentage, 100 = one fully used core. May exceed 100.
    Percent,
    Bytes,
    BytesPerSec,
    Count,
}

/// How the values of all processes of a target combine into one number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Aggregation {
    Sum,
    Max,
    Mean,
}

/// Static description of a metric.
#[derive(Clone, Debug)]
pub struct MetricDescriptor {
    pub id: MetricId,
    pub name: &'static str,
    pub unit: Unit,
    pub aggregation: Aggregation,
    /// Keep a history per individual process (not only per target).
    pub per_process_history: bool,
    /// Raising values are suspicious (candidate for leak detection).
    pub leak_candidate: bool,
}

/// Ids of the metrics shipped with the crate.
pub mod builtin {
    use super::MetricId;

    pub const CPU: MetricId = MetricId::new("cpu");
    pub const MEM_RSS: MetricId = MetricId::new("mem.rss");
    pub const MEM_VIRT: MetricId = MetricId::new("mem.virt");
    pub const THREADS: MetricId = MetricId::new("threads");
    pub const IO_READ: MetricId = MetricId::new("io.read");
    pub const IO_WRITE: MetricId = MetricId::new("io.write");
    pub const OPEN_FILES: MetricId = MetricId::new("files");
    pub const GPU_UTIL: MetricId = MetricId::new("gpu.util");
    pub const GPU_MEM: MetricId = MetricId::new("gpu.mem");
}

/// The metrics provided by [`crate::source::SysinfoSource`].
pub fn builtin_descriptors() -> Vec<MetricDescriptor> {
    use Aggregation::Sum;
    use Unit::*;
    use builtin::*;
    let d = |id, name, unit, per_process_history, leak_candidate| MetricDescriptor {
        id,
        name,
        unit,
        aggregation: Sum,
        per_process_history,
        leak_candidate,
    };
    vec![
        d(CPU, "CPU", Percent, true, false),
        d(MEM_RSS, "Memory", Bytes, true, true),
        d(MEM_VIRT, "Virtual memory", Bytes, false, false),
        d(THREADS, "Threads", Count, false, true),
        d(IO_READ, "Disk read", BytesPerSec, false, false),
        d(IO_WRITE, "Disk write", BytesPerSec, false, false),
        d(OPEN_FILES, "Open files", Count, false, true),
    ]
}

/// Ordered collection of every metric known to an engine.
#[derive(Clone, Debug, Default)]
pub struct MetricRegistry {
    items: Vec<MetricDescriptor>,
}

impl MetricRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a descriptor; an existing descriptor with the same id is replaced.
    pub fn register(&mut self, descriptor: MetricDescriptor) {
        match self.items.iter_mut().find(|d| d.id == descriptor.id) {
            Some(slot) => *slot = descriptor,
            None => self.items.push(descriptor),
        }
    }

    pub fn extend(&mut self, descriptors: impl IntoIterator<Item = MetricDescriptor>) {
        descriptors.into_iter().for_each(|d| self.register(d));
    }

    pub fn get(&self, id: MetricId) -> Option<&MetricDescriptor> {
        self.items.iter().find(|d| d.id == id)
    }

    /// Resolves a textual id (as persisted in settings) to a registered metric.
    pub fn find(&self, id: &str) -> Option<&MetricDescriptor> {
        self.items.iter().find(|d| d.id.as_str() == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &MetricDescriptor> {
        self.items.iter()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_replaces_same_id() {
        let mut reg = MetricRegistry::new();
        reg.extend(builtin_descriptors());
        let n = reg.len();
        let mut cpu = reg.get(builtin::CPU).unwrap().clone();
        cpu.name = "Processor";
        reg.register(cpu);
        assert_eq!(reg.len(), n);
        assert_eq!(reg.find("cpu").unwrap().name, "Processor");
        assert!(reg.find("nope").is_none());
    }
}
