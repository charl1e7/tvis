#![warn(clippy::all, rust_2018_idioms)]

pub mod app;
pub mod components;
pub mod metrics;
pub mod process;
pub use app::ProcessMonitorApp;
