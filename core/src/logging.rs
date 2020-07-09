use chrono::Utc;
use slog::Level;
use slog::{o, Drain, Logger, OwnedKVList, Record};
use slog_async::{Async, OverflowStrategy};
use slog_envlogger::LogBuilder;
use slog_scope::GlobalLoggerGuard;
use slog_term::{Decorator, TermDecorator};
use std::{
    panic::{self, PanicInfo},
    thread,
};

/// Initialize driver logging.
pub fn init(filter: impl AsRef<str>) -> (Logger, GlobalLoggerGuard) {
    // Log errors to stderr and lower severities to stdout.
    let format = CustomFormatter::new(
        TermDecorator::new().stderr().build(),
        TermDecorator::new().stdout().build(),
    )
    .fuse();
    let drain = Async::new(LogBuilder::new(format).parse(filter.as_ref()).build())
        .overflow_strategy(OverflowStrategy::Block)
        .build();
    let logger = Logger::root(drain.fuse(), o!());

    let guard = slog_scope::set_global_logger(logger.clone());
    slog_stdlog::init().expect("failed to register logger");

    set_panic_hook();

    (logger, guard)
}

/// Sets a panic hook so panic information is written with the log facilities
/// instead of directly to STDERR.
fn set_panic_hook() {
    fn hook(info: &PanicInfo) {
        let thread = thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");

        log::error!("thread '{}' {}", thread_name, info);
    }

    panic::set_hook(Box::new(hook));
}

/// Uses one decorator for `Error` and `Critical` log messages and the other for
/// the rest.
pub struct CustomFormatter<ErrDecorator, RestDecorator> {
    err_decorator: ErrDecorator,
    rest_decorator: RestDecorator,
}

impl<ErrDecorator, RestDecorator> CustomFormatter<ErrDecorator, RestDecorator> {
    fn new(err_decorator: ErrDecorator, rest_decorator: RestDecorator) -> Self {
        Self {
            err_decorator,
            rest_decorator,
        }
    }
}

impl<ErrDecorator: Decorator, RestDecorator: Decorator> Drain
    for CustomFormatter<ErrDecorator, RestDecorator>
{
    type Ok = ();
    type Err = std::io::Error;
    fn log(
        &self,
        record: &Record,
        values: &OwnedKVList,
    ) -> std::result::Result<Self::Ok, Self::Err> {
        match record.level() {
            Level::Error | Level::Critical => log_to_decorator(&self.err_decorator, record, values),
            _ => log_to_decorator(&self.rest_decorator, record, values),
        }
    }
}

fn log_to_decorator(
    decorator: &impl Decorator,
    record: &Record,
    values: &OwnedKVList,
) -> std::result::Result<(), std::io::Error> {
    decorator.with_record(record, values, |decorator| {
        decorator.start_timestamp()?;
        write!(decorator, "{}", Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ"))?;

        decorator.start_whitespace()?;
        write!(decorator, " ")?;

        decorator.start_level()?;
        write!(decorator, "{}", record.level())?;

        decorator.start_whitespace()?;
        write!(decorator, " ")?;

        write!(decorator, "[{}]", record.module())?;

        decorator.start_whitespace()?;
        write!(decorator, " ")?;

        decorator.start_msg()?;
        writeln!(decorator, "{}", record.msg())?;
        decorator.flush()?;

        Ok(())
    })
}
