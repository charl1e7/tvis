//! Where [`Frame`]s come from.
//!
//! * [`ProcessSource`] produces the base frame (process list + built-in metrics).
//! * [`Probe`] enriches a frame with extra metrics, only for the processes
//!   that are actually being watched – the right place for expensive or
//!   platform specific measurements.

mod synthetic;
mod sysinfo_source;

use crate::frame::{Frame, Pid, SystemInfo};
use crate::metric::MetricDescriptor;

pub use synthetic::SyntheticSource;
pub use sysinfo_source::{OpenFilesProbe, SysinfoSource};

pub trait ProcessSource: Send {
    fn info(&self) -> SystemInfo;

    /// Metrics this source fills in on every [`ProcessSample`](crate::ProcessSample).
    fn descriptors(&self) -> Vec<MetricDescriptor>;

    /// Observes the machine now. May block briefly. The engine stamps
    /// [`Frame::t`] and [`Frame::wall_ms`] afterwards.
    fn sample(&mut self) -> Frame;

    /// Frames that happened *before* the source was created, oldest first,
    /// with `t` relative to now (negative). Used by replay and demo sources.
    fn backfill(&mut self) -> Vec<Frame> {
        Vec::new()
    }
}

/// What a probe may look at while enriching a frame.
pub struct ProbeContext<'a> {
    /// Every pid that belongs to a watched target in this frame.
    pub watched: &'a [Pid],
}

pub trait Probe: Send {
    fn descriptors(&self) -> Vec<MetricDescriptor>;

    /// Adds values to the processes in `ctx.watched`.
    fn probe(&mut self, ctx: &ProbeContext<'_>, frame: &mut Frame);
}
