use slog::{o, Drain, Logger};
use slog_async::Async;
use slog_envlogger::LogBuilder;
use slog_scope::GlobalLoggerGuard;
use slog_term::{CompactFormat, TermDecorator};
use std::env;

/// The logging filter environment variable key.
const FILTER_KEY: &str = "DFUSION_LOG";

/// The default log message filter to pass into the `env_logger` when none is
/// supplied by the environment.
const DEFAULT_FILTER: &str = "info";

/// Initialize driver logging.
pub fn init() -> (Logger, GlobalLoggerGuard) {
    let filter = env::var(FILTER_KEY).unwrap_or_else(|_| DEFAULT_FILTER.to_owned());
    let format = CompactFormat::new(TermDecorator::new().stderr().build())
        .build()
        .fuse();
    let drain = Async::default(LogBuilder::new(format).parse(&filter).build());
    let logger = Logger::root(drain.fuse(), o!());

    let guard = slog_scope::set_global_logger(logger.clone());
    slog_stdlog::init().expect("failed to register logger");

    (logger, guard)
}
