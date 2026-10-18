#![forbid(unsafe_code)]
//! # tvis-core
//!
//! UI-agnostic engine behind the TVIS process profiler.
//!
//! The crate is organised around a few small, independent pieces:
//!
//! | module       | responsibility                                                   |
//! |--------------|------------------------------------------------------------------|
//! | [`metric`]   | metric identifiers, descriptors and the [`metric::MetricRegistry`] |
//! | [`frame`]    | one immutable snapshot of the machine ([`frame::Frame`])         |
//! | [`gpu`]      | GPU samples and the per-process GPU metrics                      |
//! | [`source`]   | where frames come from ([`source::ProcessSource`], [`source::Probe`]) |
//! | [`target`]   | what the user watches and how it resolves to processes           |
//! | [`series`]   | time-series ring buffers and decimation                          |
//! | [`stats`]    | descriptive statistics and regression                            |
//! | [`analysis`] | automatic insights (leaks, saturation, spikes)                   |
//! | [`store`]    | the in-memory history of everything that is watched              |
//! | [`alert`]    | threshold rules evaluated against the store                      |
//! | [`engine`]   | sampling thread, command channel and observer hook               |
//! | [`export`]   | CSV / JSON serialisation of recorded history                     |
//! | [`units`]    | human readable formatting                                        |
//!
//! Adding a metric means implementing [`source::Probe`] (or extending a
//! [`source::ProcessSource`]) and registering its [`metric::MetricDescriptor`];
//! the store, alerts, analysis and export pick it up without any change.

pub mod alert;
pub mod analysis;
pub mod engine;
pub mod export;
pub mod frame;
pub mod gpu;
pub mod metric;
pub mod series;
pub mod source;
pub mod stats;
pub mod store;
pub mod target;
pub mod units;

pub use engine::{
    Command, Engine, EngineConfig, EngineHandle, EngineView, Event, Observer, TickTimings,
};
pub use frame::{Frame, Pid, ProcessKind, ProcessSample, SystemInfo, SystemSample};
pub use metric::{MetricDescriptor, MetricId, MetricRegistry};
pub use target::{Target, TargetId};
