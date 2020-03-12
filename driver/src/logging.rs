use slog::Level;
use slog::{o, Drain, Logger, OwnedKVList, Record};
use slog_async::Async;
use slog_envlogger::LogBuilder;
use slog_scope::GlobalLoggerGuard;
use slog_term::{Decorator, TermDecorator};

/// The channel size for async logging.
const BUFFER_SIZE: usize = 1024;

/// Initialize driver logging.
pub fn init(filter: impl AsRef<str>) -> (Logger, GlobalLoggerGuard) {
    // Log errors to stderr and lower severities to stdout.
    let format = CustomFormatter::new(
        TermDecorator::new().stderr().build(),
        TermDecorator::new().stdout().build(),
    )
    .fuse();
    let drain = Async::new(LogBuilder::new(format).parse(filter.as_ref()).build())
        .chan_size(BUFFER_SIZE)
        .build();
    let logger = Logger::root(drain.fuse(), o!());

    let guard = slog_scope::set_global_logger(logger.clone());
    slog_stdlog::init().expect("failed to register logger");

    (logger, guard)
}

/// Uses one decorator for `Error` and `Critical` log messages and the other for
/// the rest.
pub struct CustomFormatter<T0, T1> {
    err_decorator: T0,
    rest_decorator: T1,
}

impl<T0, T1> CustomFormatter<T0, T1> {
    fn new(err_decorator: T0, rest_decorator: T1) -> Self {
        Self {
            err_decorator,
            rest_decorator,
        }
    }
}

impl<T0: Decorator, T1: Decorator> Drain for CustomFormatter<T0, T1> {
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
    decorator.with_record(record, values, |mut decorator| {
        decorator.start_timestamp()?;
        slog_term::timestamp_utc(&mut decorator)?;

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
