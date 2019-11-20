//! This crate initializes logging for the drivers for both Snapp and StableX.

use lazy_static::lazy_static;
use log::SetLoggerError;
use slog::Logger;
use slog_scope::GlobalLoggerGuard;

lazy_static! {
    /// A static instance of a global logger guard. This cannot be dropped or
    /// else the the slog scope will reset the global logger. Using a lazy
    /// static ensures that the logger only gets initialized once.
    static ref LOGGER_GUARD: Result<GlobalLoggerGuard, SetLoggerError> = init_global_logger();
}

fn init_global_logger() -> Result<GlobalLoggerGuard, SetLoggerError> {
    // use the graph's logging settings since they seem to be good.
    let logger = graph::log::logger(false);
    let guard = slog_scope::set_global_logger(logger);
    slog_stdlog::init()?;

    Ok(guard)
}

/// Initialize and set the global logger. Returns a handle to the global logger.
pub fn init() -> Result<Logger, &'static SetLoggerError> {
    LOGGER_GUARD.as_ref()?;
    Ok(slog_scope::logger())
}
