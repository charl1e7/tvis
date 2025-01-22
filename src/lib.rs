#![warn(clippy::all, rust_2018_idioms)]

pub mod app;
pub mod process;
pub mod ui;
pub mod settings;

pub use app::ProcessMonitorApp;
