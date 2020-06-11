mod estimate_buy_amount;
mod filter;
mod models;
mod orderbook;

use core::{
    contracts::{stablex_contract::StableXContractImpl, web3_provider},
    http::HttpFactory,
    metrics::{HttpMetrics, MetricsServer},
    orderbook::EventBasedOrderbook,
    util::FutureWaitExt as _,
};
use ethcontract::PrivateKey;
use prometheus::Registry;
use std::{num::ParseIntError, path::PathBuf, sync::Arc, thread, time::Duration};
use structopt::StructOpt;
use tokio::{runtime, time};
use url::Url;

#[derive(Debug, StructOpt)]
#[structopt(name = "price estimator", rename_all = "kebab")]
struct Options {
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
        default_value = "10",
        parse(try_from_str = duration_secs),
    )]
    orderbook_update_interval: Duration,

    /// The safety margin to subtract from the estimated price, in order to make it more likely to
    /// be matched.
    #[structopt(long, env = "PRICE_ROUNDING_BUFFER", default_value = "0.001")]
    price_rounding_buffer: f64,
}

type Orderbook = orderbook::Orderbook<EventBasedOrderbook>;

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

    let orderbook = EventBasedOrderbook::new(contract, web3, options.orderbook_file);
    let orderbook = Arc::new(Orderbook::new(orderbook));
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

    let filter = filter::estimated_buy_amount(orderbook, price_rounding_buffer);
    let serve_task = runtime.spawn(warp::serve(filter).run(([127, 0, 0, 1], 8080)));

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
