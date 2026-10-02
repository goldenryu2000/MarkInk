// No console window behind the app on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

use clap::Parser;
use markink::cli::Cli;
use markink::{app, logging, paths};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let launch = match std::env::current_dir()
        .map_err(|e| e.to_string())
        .and_then(|cwd| cli.launch(&cwd))
    {
        Ok(launch) => launch,
        Err(err) => {
            eprintln!("markink: {err}");
            return ExitCode::FAILURE;
        }
    };
    logging::init(&paths::state_dir());
    tracing::info!(root = %launch.root.display(), "starting");
    match app::run(launch) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "fatal error");
            eprintln!("markink: {err}");
            ExitCode::FAILURE
        }
    }
}
