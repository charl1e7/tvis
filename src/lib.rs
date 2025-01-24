#![warn(clippy::all, rust_2018_idioms)]

pub mod app;
pub mod components;
pub mod process;
pub mod metrics;
pub use app::ProcessMonitorApp;
