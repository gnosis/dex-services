mod amounts_at_price;
mod error;
mod filter;
mod infallible_price_source;
mod models;
mod orderbook;
mod solver_rounding_buffer;

use core::{
    contracts::{stablex_contract::StableXContractImpl, web3_provider},
    http::HttpFactory,
    logging,
    metrics::{HttpMetrics, MetricsServer},
    orderbook::EventBasedOrderbook,
    token_info::{cached::TokenInfoCache, hardcoded::TokenData},
    util::FutureWaitExt as _,
};
use ethcontract::PrivateKey;
use infallible_price_source::PriceCacheUpdater;
use orderbook::Orderbook;
use prometheus::Registry;
use std::{
    collections::HashMap, net::SocketAddr, num::ParseIntError, path::PathBuf, sync::Arc, thread,
    time::Duration,
};
use structopt::StructOpt;
use tokio::{runtime, time};
use url::Url;
use warp::Filter;

#[derive(Debug, StructOpt)]
#[structopt(name = "price estimator", rename_all = "kebab")]
struct Options {
    /// The log filter to use.
    ///
    /// This follows the `slog-envlogger` syntax (e.g. 'info,price_estimator=debug').
    #[structopt(
        long,
        env = "DFUSION_LOG",
        default_value = "warn,price_estimator=info,core=info,warp::filters::log=info"
    )]
    log_filter: String,

    #[structopt(long, env = "BIND_ADDRESS", default_value = "0.0.0.0:8080")]
    bind_address: SocketAddr,

    /// The Ethereum node URL to connect to. Make sure that the node allows for
    /// queries without a gas limit to be able to fetch the orderbook.
    #[structopt(long, env = "ETHEREUM_NODE_URL")]
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
        default_value = "10",
        parse(try_from_str = duration_secs),
    )]
    orderbook_update_interval: Duration,

    /// Time interval in seconds in which the external price sources should be updated.
    #[structopt(
        long,
        env = "PRICE_SOURCE_UPDATE_INTERVAL",
        default_value = "60",
        parse(try_from_str = duration_secs),
    )]
    price_source_update_interval: Duration,

    /// JSON encoded backup token information like in the driver. Used as an override to the ERC20
    /// information we fetch from the block chain in case that information is wrong or unavailable
    /// which can happen for example when tokens do not implement the standard properly.
    #[structopt(long, env = "TOKEN_DATA", default_value = "{}")]
    token_data: TokenData,

    /// The number of blocks to fetch events for at a time for constructing the
    /// orderbook.
    #[structopt(long, env = "AUCTION_DATA_PAGE_SIZE", default_value = "500")]
    auction_data_page_size: usize,

    /// An extra factor to multiply calculated rounding buffers with. Setting this to >1 protects
    /// makes prices mores conservative protecting against changes between the time a user requested
    /// an estimate and the solver submitting a solution.
    #[structopt(long, env = "EXTRA_ROUNDING_BUFFER_FACTOR", default_value = "2.0")]
    extra_rounding_buffer_factor: f64,
}

fn main() {
    let options = Options::from_args();
    let (_, _guard) = logging::init(&options.log_filter);
    log::info!(
        "Starting price estimator with runtime options: {:#?}",
        options
    );

    let driver_http_metrics = setup_driver_metrics();
    let http_factory = HttpFactory::new(options.timeout, driver_http_metrics);
    let web3 = web3_provider(&http_factory, options.node_url.as_str(), options.timeout).unwrap();
    // The private key is not actually used but StableXContractImpl requires it.
    let private_key = PrivateKey::from_raw([1u8; 32]).unwrap();
    let contract = Arc::new(
        StableXContractImpl::new(&web3, private_key, 0)
            .wait()
            .unwrap(),
    );

    let cache: HashMap<_, _> = options.token_data.clone().into();
    let token_info = TokenInfoCache::with_cache(contract.clone(), cache);
    token_info
        .cache_all(10)
        .wait()
        .expect("failed to cache token infos");
    let token_info = Arc::new(token_info);

    let orderbook = EventBasedOrderbook::new(
        contract,
        web3,
        options.auction_data_page_size,
        options.orderbook_file,
    );

    let external_price_sources = core::price_estimation::external_price_sources(
        &http_factory,
        token_info.clone(),
        options.price_source_update_interval,
    )
    .expect("failed to create external price sources");
    let infallible_price_source =
        PriceCacheUpdater::new(token_info.clone(), external_price_sources);

    let orderbook = Arc::new(Orderbook::new(
        Box::new(orderbook),
        infallible_price_source,
        options.extra_rounding_buffer_factor,
    ));
    let _ = orderbook.update().wait();
    log::info!("Orderbook initialized.");

    let mut runtime = runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()
        .unwrap();

    let orderbook_task = runtime.spawn(update_orderbook_forever(
        orderbook.clone(),
        options.orderbook_update_interval,
    ));

    let filter = filter::all(orderbook, token_info).with(warp::log("price_estimator"));
    let serve_task = runtime.spawn(warp::serve(filter).run(options.bind_address));

    log::info!("Server ready.");
    runtime.block_on(async move {
        tokio::select! {
            _ = orderbook_task => log::error!("Update task exited."),
            _ = serve_task => log::error!("Serve task exited."),
        }
    });
}

async fn update_orderbook_forever(orderbook: Arc<Orderbook>, update_interval: Duration) -> ! {
    loop {
        time::delay_for(update_interval).await;
        if let Err(err) = orderbook.update().await {
            log::error!("error updating orderbook: {:?}", err);
        }
    }
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
