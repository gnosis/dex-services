use env_logger::{self, Env};

/// The logging filter environment variable key.
const FILTER_KEY: &str = "DFUSION_LOG";
const WRITE_STYLE_KEY: &str = "DFUSION_LOG_STYLE";

/// The default log message filter to pass into the `env_logger` when none is
/// supplied by the environment.
const DEFAULT_FILTER: &str = "info,driver=debug";

/// Initialize driver logging.
pub fn init() {
    let env = Env::new()
        .filter_or(FILTER_KEY, DEFAULT_FILTER)
        .write_style(WRITE_STYLE_KEY);

    env_logger::init_from_env(env);
}
