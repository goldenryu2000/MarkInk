//! Logging to a file in the state directory.

use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::Mutex;

use tracing_subscriber::EnvFilter;

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Logs to `<state_dir>/markink.log`, or stderr if that fails. Level from `MARKINK_LOG`.
pub fn init(state_dir: &Path) {
    let filter = EnvFilter::try_from_env("MARKINK_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let path = state_dir.join("markink.log");
    let too_big = fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES);
    let file = fs::create_dir_all(state_dir).and_then(|_| {
        OpenOptions::new()
            .create(true)
            .write(true)
            .append(!too_big)
            .truncate(too_big)
            .open(&path)
    });
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false);
    match file {
        Ok(file) => builder.with_writer(Mutex::new(file)).init(),
        Err(_) => builder.with_writer(std::io::stderr).init(),
    }
}
