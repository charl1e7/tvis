// Slint-generated global accessors carry an elided lifetime (`Backend<'_>`).
#![allow(elided_lifetimes_in_paths)]
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod cli;
mod record;
mod settings;

use anyhow::Result;
use clap::Parser;

slint::include_modules!();

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("TVIS_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = cli::Cli::parse();
    match &cli.command {
        Some(cli::Command::Record(args)) => record::run(&cli, args),
        None => app::run(cli),
    }
}
