use slog::{o, Drain, Logger, OwnedKVList, Record};
use slog_async::Async;
use slog_envlogger::LogBuilder;
use slog_scope::GlobalLoggerGuard;
use slog_term::{Decorator, TermDecorator};

/// The channel size for async logging.
const BUFFER_SIZE: usize = 1024;

/// Initialize driver logging.
pub fn init(filter: impl AsRef<str>) -> (Logger, GlobalLoggerGuard) {
    let format = CustomFormatter::new(TermDecorator::new().stderr().build()).fuse();
    let drain = Async::new(LogBuilder::new(format).parse(filter.as_ref()).build())
        .chan_size(BUFFER_SIZE)
        .build();
    let logger = Logger::root(drain.fuse(), o!());

    let guard = slog_scope::set_global_logger(logger.clone());
    slog_stdlog::init().expect("failed to register logger");

    (logger, guard)
}

pub struct CustomFormatter<D: Decorator> {
    decorator: D,
}

impl<D: Decorator> CustomFormatter<D> {
    fn new(decorator: D) -> Self {
        Self { decorator }
    }
}

impl<D: Decorator> Drain for CustomFormatter<D> {
    type Ok = ();
    type Err = std::io::Error;
    fn log(
        &self,
        record: &Record,
        values: &OwnedKVList,
    ) -> std::result::Result<Self::Ok, Self::Err> {
        self.decorator.with_record(record, values, |mut decorator| {
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
}
