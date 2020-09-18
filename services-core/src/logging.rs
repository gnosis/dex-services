use chrono::Utc;
use slog::{b, o, record, Drain, Level, Logger, OwnedKVList, Record};
use slog_async::{Async, OverflowStrategy};
use slog_envlogger::LogBuilder;
use slog_scope::GlobalLoggerGuard;
use slog_term::{Decorator, PlainDecorator, TermDecorator};
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
/// in addition to the default panic printer.
fn set_panic_hook() {
    let default_hook = panic::take_hook();
    let hook = move |info: &PanicInfo| {
        let thread = thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");

        // It is not possible for our custom hook to print a full backtrace on stable rust. To not
        // lose this information we call the default panic handler which prints the full backtrace.
        // We print a fake log message prefix so that kibana can identify that this is supposed to
        // a single message.
        let decorator = PlainDecorator::new(std::io::stderr());
        let _ = log_prefix_to_decorator(
            &decorator,
            &record!(Level::Error, "", &format_args!(""), b!()),
        );
        default_hook(info);
        log::error!("thread '{}' {}", thread_name, info);
    };

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

fn formatted_current_time() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn log_prefix_to_decorator(
    decorator: &impl Decorator,
    record: &Record,
) -> std::result::Result<(), std::io::Error> {
    decorator.with_record(record, &o!().into(), |decorator| {
        decorator.start_timestamp()?;
        write!(decorator, "{}", formatted_current_time())?;

        decorator.start_whitespace()?;
        write!(decorator, " ")?;

        decorator.start_level()?;
        write!(decorator, "{}", record.level())?;

        decorator.start_whitespace()?;
        write!(decorator, " ")?;

        write!(decorator, "[{}]", record.module())?;

        decorator.start_whitespace()?;
        write!(decorator, " ")?;

        Ok(())
    })
}

fn log_to_decorator(
    decorator: &impl Decorator,
    record: &Record,
    values: &OwnedKVList,
) -> std::result::Result<(), std::io::Error> {
    log_prefix_to_decorator(decorator, record)?;
    decorator.with_record(record, values, |decorator| {
        decorator.start_msg()?;
        writeln!(decorator, "{}", record.msg())?;
        decorator.flush()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // `env RUST_BACKTRACE=1 cargo test -p core panic_is_printed -- --ignored --nocapture`
    // Should see the normal rust panic backtrace and an error log message.
    #[test]
    #[ignore]
    fn panic_is_printed() {
        let _log = init("info");
        let _ = std::thread::spawn(|| panic!()).join();
    }
}
