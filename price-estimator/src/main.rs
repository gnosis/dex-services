mod error;
mod filter;
mod infallible_price_source;
mod models;
mod orderbook;
mod solver_rounding_buffer;

use core::{
    contracts::{stablex_contract::StableXContractImpl, web3_provider},
    http::HttpFactory,
    metrics::{HttpMetrics, MetricsServer},
    orderbook::EventBasedOrderbook,
    token_info::{cached::TokenInfoCache, hardcoded::TokenData},
    util::FutureWaitExt as _,
};
use ethcontract::PrivateKey;
use orderbook::Orderbook;
use prometheus::Registry;
use std::net::SocketAddr;
use std::{num::ParseIntError, path::PathBuf, sync::Arc, thread, time::Duration};
use structopt::StructOpt;
use tokio::{runtime, time};
use url::Url;

#[derive(Debug, StructOpt)]
#[structopt(name = "price estimator", rename_all = "kebab")]
struct Options {
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

    /// The safety margin to subtract from the estimated price, in order to make it more likely to
    /// be matched.
    #[structopt(long, env = "PRICE_ROUNDING_BUFFER", default_value = "0.001")]
    price_rounding_buffer: f64,

    /// JSON encoded backup token information like in the driver. Used as an override to the ERC20
    /// information we fetch from the block chain in case that information is wrong or unavailable
    /// which can happen for example when tokens do not implement the standard properly.
    #[structopt(long, env = "TOKEN_DATA", default_value = "{}")]
    token_data: TokenData,

    /// The number of blocks to fetch events for at a time for constructing the
    /// orderbook.
    #[structopt(long, env = "AUCTION_DATA_PAGE_SIZE", default_value = "500")]
    auction_data_page_size: usize,
}

fn main() {
    let options = Options::from_args();
    env_logger::init();
    log::info!(
        "Starting price estimator with runtime options: {:#?}",
        options
    );
    let price_rounding_buffer = options.price_rounding_buffer;
    assert!(price_rounding_buffer.is_finite());
    assert!(price_rounding_buffer >= 0.0 && price_rounding_buffer <= 1.0);

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

    let token_info = TokenInfoCache::with_cache(contract.clone(), options.token_data.into());
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
    let orderbook = Arc::new(Orderbook::new(Box::new(orderbook)));
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

    let filter = filter::all(orderbook, token_info, price_rounding_buffer);
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
            log::warn!("error updating orderbook: {:?}", err);
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
