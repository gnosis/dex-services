use core::{
    contracts::{stablex_contract::StableXContractImpl, web3_provider},
    http::HttpFactory,
    metrics::{HttpMetrics, MetricsServer},
    util::FutureWaitExt as _,
};
use ethcontract::PrivateKey;
use prometheus::Registry;
use std::{num::ParseIntError, path::PathBuf, sync::Arc, thread, time::Duration};
use structopt::StructOpt;
use tokio::runtime;
use url::Url;
use warp::Filter;

#[derive(Debug, StructOpt)]
#[structopt(name = "price estimator", rename_all = "kebab")]
struct Options {
    /// The log filter to use.
    ///
    /// This follows the envlogger syntax (e.g. 'info,driver=debug').
    #[structopt(
        long,
        env = "LOG_FILTER",
        default_value = "warn,driver=info,price_estimator=info"
    )]
    log_filter: String,

    /// The Ethereum node URL to connect to. Make sure that the node allows for
    /// queries without a gas limit to be able to fetch the orderbook.
    #[structopt(short, long, env = "NODE_URL")]
    node_url: Url,

    /// The timeout in seconds of web3 JSON RPC calls.
    #[structopt(
        long,
        env = "TIMEOUT",
        default_value = "10",
        parse(try_from_str = duration_secs),
    )]
    timeout: Duration,

    #[structopt(long, env = "ORDERBOOK_FILE", parse(from_os_str))]
    orderbook_file: Option<PathBuf>,

    #[structopt(
        long,
        env = "ORDERBOOK_UPDATE_INTERVAL",
        default_value = "100",
        parse(try_from_str = duration_secs),
    )]
    orderbook_update_interval: Duration,
}

fn main() {
    let options = Options::from_args();
    env_logger::init();
    log::info!(
        "Starting price estimator with runtime options: {:#?}",
        options
    );

    let driver_http_metrics = setup_driver_metrics();
    let http_factory = HttpFactory::new(options.timeout, driver_http_metrics);
    let web3 = web3_provider(&http_factory, options.node_url.as_str(), options.timeout).unwrap();
    // The private key is not actually used but StableXContractImpl requires it.
    let private_key = PrivateKey::from_raw([1u8; 32]).unwrap();
    let _contract = Arc::new(
        StableXContractImpl::new(&web3, private_key, 0)
            .wait()
            .unwrap(),
    );

    // TODO: create event based orderbook
    // TODO: handle http requests

    let mut runtime = runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(warp::serve(warp::any().map(|| "")).run(([127, 0, 0, 1], 8080)));
}

fn duration_secs(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_secs(s.parse()?))
}

fn setup_driver_metrics() -> HttpMetrics {
    let prometheus_registry = Arc::new(Registry::new());
    let metric_server = MetricsServer::new(prometheus_registry.clone());
    thread::spawn(move || {
        metric_server.serve(9586);
    });
    HttpMetrics::new(&prometheus_registry).unwrap()
}
