mod amounts_at_price;
mod error;
mod filter;
mod infallible_price_source;
mod metrics;
mod models;
mod orderbook;
mod solver_rounding_buffer;

use ethcontract::PrivateKey;
use infallible_price_source::PriceCacheUpdater;
use metrics::Metrics;
use orderbook::Orderbook;
use prometheus::Registry;
use services_core::{
    contracts::{stablex_contract::StableXContractImpl, web3_provider},
    economic_viability::EconomicViabilityStrategy,
    gas_price::{self, GasEstimatorType},
    health::{HealthReporting, HttpHealthEndpoint},
    http::HttpFactory,
    http_server::{DefaultRouter, RouilleServer, Serving},
    logging,
    metrics::{HttpMetrics, MetricsHandler},
    orderbook::EventBasedOrderbook,
    token_info::{cached::TokenInfoCache, hardcoded::TokenData},
    util::FutureWaitExt as _,
};
use std::{
    collections::HashMap, net::SocketAddr, num::ParseIntError, path::PathBuf, sync::Arc,
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
        env = "LOG_FILTER",
        default_value = "warn,price_estimator=info,services_core=info,warp::filters::log=info"
    )]
    log_filter: String,

    #[structopt(long, env = "BIND_ADDRESS", default_value = "0.0.0.0:8080")]
    bind_address: SocketAddr,

    /// The Ethereum node URL to connect to. Make sure that the node allows for
    /// queries without a gas limit to be able to fetch the orderbook.
    #[structopt(long, env = "NODE_URL")]
    node_url: Url,

    /// The timeout in seconds of web3 JSON RPC calls.
    #[structopt(
        long,
        env = "RPC_TIMEOUT",
        default_value = "10",
        parse(try_from_str = duration_secs),
    )]
    rpc_timeout: Duration,

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

    // These are copies of the same arguments used in the driver for economic viability.
    // It would be nice if we could avoid copy pasting these. Maybe structopt/clap has a feature
    // like serde's flatten that would allow us to do this.
    #[structopt(
        long,
        env = "ECONOMIC_VIABILITY_SUBSIDY_FACTOR",
        default_value = "10.0"
    )]
    economic_viability_subsidy_factor: f64,
    #[structopt(
        long,
        env = "ECONOMIC_VIABILITY_MIN_AVG_FEE_FACTOR",
        default_value = "1.1"
    )]
    economic_viability_min_avg_fee_factor: f64,
    #[structopt(long, env = "STATIC_MIN_AVG_FEE_PER_ORDER")]
    static_min_avg_fee_per_order: Option<u128>,
    #[structopt(long, env = "STATIC_MAX_GAS_PRICE")]
    static_max_gas_price: Option<u128>,
    #[structopt(
        long,
        env = "ECONOMIC_VIABILITY_STRATEGY",
        default_value = "Dynamic",
        possible_values = EconomicViabilityStrategy::variant_names(),
        case_insensitive = true,
    )]
    economic_viability_strategy: EconomicViabilityStrategy,

    /// ID for the token which is used to pay network transaction fees on the
    /// target chain (e.g. WETH on mainnet, DAI on xDAI).
    #[structopt(long, env = "NATIVE_TOKEN_ID", default_value = "1")]
    native_token_id: u16,

    #[structopt(
        long,
        env = "GAS_ESTIMATORS",
        default_value = "Web3",
        possible_values = GasEstimatorType::variant_names(),
        case_insensitive = true,
        use_delimiter = true
    )]
    gas_estimators: Vec<GasEstimatorType>,
}

fn main() {
    let options = Options::from_args();
    let (_, _guard) = logging::init(&options.log_filter);
    log::info!(
        "Starting price estimator with runtime options: {:#?}",
        options
    );

    let (metrics, driver_http_metrics, health) = setup_monitoring();
    let metrics = Arc::new(metrics);
    let http_factory = HttpFactory::new(options.rpc_timeout, driver_http_metrics);
    let web3 = web3_provider(
        &http_factory,
        options.node_url.as_str(),
        options.rpc_timeout,
    )
    .unwrap();
    // The private key is not actually used but StableXContractImpl requires it.
    let private_key = PrivateKey::from_raw([1u8; 32]).unwrap();
    let contract = Arc::new(StableXContractImpl::new(&web3, private_key).wait().unwrap());
    let gas_station =
        gas_price::create_priority_estimator(&http_factory, &web3, &options.gas_estimators)
            .wait()
            .unwrap();

    let cache: HashMap<_, _> = options.token_data.clone().into();
    let token_info = TokenInfoCache::with_cache(contract.clone(), cache);
    token_info
        .cache_all()
        .wait()
        .expect("failed to cache token infos");
    let token_info = Arc::new(token_info);

    let orderbook = EventBasedOrderbook::new(
        contract,
        web3,
        options.auction_data_page_size,
        options.orderbook_file,
    );

    let external_price_sources = services_core::price_estimation::external_price_sources(
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
        options.native_token_id.into(),
    ));
    let _ = orderbook.update().wait();
    log::info!("Orderbook initialized.");

    let economic_viability = options
        .economic_viability_strategy
        .from_arguments(
            options.economic_viability_subsidy_factor,
            options.economic_viability_min_avg_fee_factor,
            options.static_min_avg_fee_per_order,
            options.static_max_gas_price,
            orderbook.clone(),
            gas_station.clone(),
        )
        .unwrap();

    let mut runtime = runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()
        .unwrap();

    let orderbook_task = runtime.spawn(update_orderbook_forever(
        orderbook.clone(),
        options.orderbook_update_interval,
    ));

    // We add the allow origin header so that requests from the interactive openapi documentation
    // go through to locally running instance. This does mean we set the header for non openapi
    // requests too. This doesn't have security implications because this is a public,
    // unauthenticated api anyway.
    let filter = filter::all(orderbook, token_info, metrics.clone(), economic_viability)
        .with(warp::log::custom(move |info| metrics.handle_response(info)))
        .with(warp::log("price_estimator"))
        .with(warp::reply::with::header(
            "Access-Control-Allow-Origin",
            "*",
        ));
    let serve_task = runtime.spawn(warp::serve(filter).run(options.bind_address));

    log::info!("Server ready.");
    runtime.block_on(async move {
        health.notify_ready();
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

fn setup_monitoring() -> (Metrics, HttpMetrics, Arc<dyn HealthReporting>) {
    let health = Arc::new(HttpHealthEndpoint::new());
    let prometheus_registry = Arc::new(Registry::new());

    let metric_handler = MetricsHandler::new(prometheus_registry.clone());
    RouilleServer::new(DefaultRouter {
        metrics: Arc::new(metric_handler),
        health_readiness: health.clone(),
    })
    .start_in_background();

    let http_metrics = HttpMetrics::new(&prometheus_registry).unwrap();
    let metrics = Metrics::new(prometheus_registry.as_ref()).unwrap();

    (metrics, http_metrics, health)
}
