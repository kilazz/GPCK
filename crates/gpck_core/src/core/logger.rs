// crates/gpck_core/src/core/logger.rs
//! # Thread-Safe Asynchronous Logging Utility
//!
//! Provides non-blocking background logging using `tracing` and `tracing-appender`
//! to prevent thread lock contention during high-throughput I/O operations.

use super::paths::GpckPaths;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::writer::MakeWriterExt;

/// Initializes the non-blocking file and stdout logger in the centralized log directory.
pub fn init_logger() -> WorkerGuard {
    let log_dir = GpckPaths::get_logs_dir();
    let file_appender = tracing_appender::rolling::never(&log_dir, "gpck.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    fmt()
        .with_writer(non_blocking.and(std::io::stdout))
        .with_thread_ids(true)
        .with_target(false)
        .init();

    tracing::info!(
        "Tracing Logger initialized. Writing session output to {:?}",
        log_dir.join("gpck.log")
    );
    guard
}

/// Logs an informational message.
pub fn log_info(msg: &str) {
    tracing::info!("{}", msg);
}

/// Logs a warning message.
pub fn log_warn(msg: &str) {
    tracing::warn!("{}", msg);
}

/// Logs an error message.
pub fn log_error(msg: &str) {
    tracing::error!("{}", msg);
}

/// Logs a debug message.
pub fn log_debug(msg: &str) {
    tracing::debug!("{}", msg);
}
