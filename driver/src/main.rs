#[macro_use]
mod macros;

mod contracts;
mod driver;
mod eth_rpc;
mod gas_station;
mod http;
mod logging;
mod metrics;
mod models;
mod orderbook;
mod price_estimation;
mod price_finding;
mod solution_submission;
mod transport;
mod util;

use crate::contracts::{stablex_contract::StableXContractImpl, web3_provider};
use crate::driver::{
    scheduler::{AuctionTimingConfiguration, SchedulerKind},
    stablex_driver::StableXDriverImpl,
};
use crate::eth_rpc::Web3EthRpc;
use crate::gas_station::GnosisSafeGasStation;
use crate::http::HttpFactory;
use crate::metrics::{HttpMetrics, MetricsServer, StableXMetrics};
use crate::orderbook::{FilteredOrderbookReader, OrderbookFilter, PaginatedStableXOrderBookReader};
use crate::price_estimation::{PriceOracle, TokenData};
use crate::price_finding::{Fee, SolverType};
use crate::solution_submission::StableXSolutionSubmitter;

use ethcontract::PrivateKey;
use log::info;
use prometheus::Registry;
use std::num::ParseIntError;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use structopt::StructOpt;
use url::Url;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "driver",
    about = "Gnosis Exchange protocol driver.",
    rename_all = "kebab"
)]
struct Options {
    /// The log fiter to use.
    ///
    /// This follows the `slog-envlogger` syntax (e.g. 'info,driver=debug').
    #[structopt(long, env = "DFUSION_LOG", default_value = "info")]
    log_filter: String,

    /// The Ethereum node URL to connect to. Make sure that the node allows for
    /// queries witout a gas limit to be able to fetch the orderbook.
    #[structopt(short, long, env = "ETHEREUM_NODE_URL")]
    node_url: Url,

    /// The network ID used for signing transactions (e.g. 1 for mainnet, 4 for
    /// rinkeby, 5777 for ganache).
    #[structopt(short = "i", long, env = "NETWORK_ID")]
    network_id: u64,

    /// Which style of solver to use. Can be one of: 'NAIVE' for the naive
    /// solver; 'MIP' for mixed integer programming solver; 'NLP' for non-linear
    /// programming solver.
    #[structopt(long, env = "SOLVER_TYPE", default_value = "naive-solver")]
    solver_type: SolverType,

    /// JSON encoded backup token information to provide to the solver.
    ///
    /// For example: '{
    ///   "T0001": {
    ///     "alias": "WETH",
    ///     "decimals": 18,
    ///     "externalPrice": 200000000000000000000,
    ///     "shouldEstimatePrice": false
    ///   },
    ///   "T0004": {
    ///     "alias": "USDC",
    ///     "decimals": 6,
    ///     "externalPrice": 1000000000000000000000000000000,
    ///     "shouldEstimatePrice": true
    ///   }
    /// }'
    #[structopt(long, env = "TOKEN_DATA", default_value = "{}")]
    token_data: TokenData,

    /// JSON encoded object of which tokens/orders to ignore.
    ///
    /// For example: '{
    ///   "tokens": {"Whitelist": [1, 2]},
    ///   "users": {
    ///     "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0A": { "OrderIds": [0, 1] },
    ///     "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0B": "All"
    ///   }
    #[structopt(long, env = "ORDERBOOK_FILTER", default_value = "{}")]
    orderbook_filter: OrderbookFilter,

    /// The private key used by the driver to sign transactions.
    #[structopt(short = "k", long, env = "PRIVATE_KEY", hide_env_values = true)]
    private_key: PrivateKey,

    /// The page size with which to read orders from the smart contract.
    #[structopt(long, env = "AUCTION_DATA_PAGE_SIZE", default_value = "100")]
    auction_data_page_size: u16,

    /// The timeout in milliseconds of web3 JSON RPC calls, defaults to 10000ms
    #[structopt(
        long,
        env = "WEB3_RPC_TIMEOUT",
        default_value = "10000",
        parse(try_from_str = duration_millis),
    )]
    rpc_timeout: Duration,

    /// The default timeout in milliseconds of HTTP requests to remote services
    /// such as the Gnosis Safe gas station and exchange REST APIs for fetching
    /// price estimates.
    #[structopt(
        long,
        env = "HTTP_TIMEOUT",
        default_value = "10000",
        parse(try_from_str = duration_millis),
    )]
    http_timeout: Duration,

    /// The offset from the start of a batch in seconds at which point we
    /// should start solving.
    #[structopt(
        long,
        env = "TARGET_START_SOLVE_TIME",
        default_value = "30",
        parse(try_from_str = duration_secs),
    )]
    target_start_solve_time: Duration,

    /// The offset from the start of the batch to cap the solver's execution
    /// time.
    #[structopt(
        long,
        env = "SOLVER_TIME_LIMIT",
        default_value = "210",
        parse(try_from_str = duration_secs),
    )]
    solver_time_limit: Duration,

    /// The kind of scheduler to use.
    #[structopt(long, env = "SCHEDULER", default_value = "system")]
    scheduler: SchedulerKind,

    /// Time interval in seconds in which price sources should be updated.
    #[structopt(
        long,
        env = "PRICE_SOURCE_UPDATE_INTERVAL",
        default_value = "300",
        parse(try_from_str = duration_secs),
    )]
    price_source_update_interval: Duration,
}

fn main() {
    let options = Options::from_args();
    let (_, _guard) = logging::init(&options.log_filter);
    info!("Starting driver with runtime options: {:#?}", options);

    // Set up metrics and serve in separate thread.
    let prometheus_registry = Arc::new(Registry::new());
    let stablex_metrics = StableXMetrics::new(prometheus_registry.clone());
    let http_metrics = HttpMetrics::new(&prometheus_registry).unwrap();
    let metric_server = MetricsServer::new(prometheus_registry);
    thread::spawn(move || {
        metric_server.serve(9586);
    });

    // Set up shared HTTP client and HTTP services.
    let http_factory = HttpFactory::new(options.http_timeout, http_metrics);
    let web3 = web3_provider(
        &http_factory,
        options.node_url.as_str(),
        options.rpc_timeout,
    )
    .unwrap();
    let gas_station = GnosisSafeGasStation::new(&http_factory, gas_station::DEFAULT_URI).unwrap();
    let price_oracle = PriceOracle::new(
        &http_factory,
        options.token_data,
        options.price_source_update_interval,
    )
    .unwrap();

    // Set up web3 and contract connection.
    let contract = StableXContractImpl::new(
        &web3,
        options.private_key.clone(),
        options.network_id,
        &gas_station,
    )
    .unwrap();
    info!("Using contract at {:?}", contract.address());
    info!("Using account {:?}", contract.account());

    // Set up solver.
    let fee = Some(Fee::default());
    let price_finder = price_finding::create_price_finder(fee, options.solver_type, price_oracle);

    // Create the orderbook reader.
    let orderbook = PaginatedStableXOrderBookReader::new(&contract, options.auction_data_page_size);
    info!("Blacklist Orderbook filter: {:?}", options.orderbook_filter);

    let filtered_orderbook = FilteredOrderbookReader::new(&orderbook, options.orderbook_filter);

    // Set up solution submitter.
    let eth_rpc = Web3EthRpc::new(&web3);
    let solution_submitter = StableXSolutionSubmitter::new(&contract, &eth_rpc);

    // Set up the driver and start the run-loop.
    let driver = StableXDriverImpl::new(
        &*price_finder,
        &filtered_orderbook,
        &solution_submitter,
        &stablex_metrics,
    );

    let scheduler_config =
        AuctionTimingConfiguration::new(options.target_start_solve_time, options.solver_time_limit);

    let mut scheduler = options
        .scheduler
        .create(&contract, &driver, scheduler_config);
    scheduler.start();
}

fn duration_millis(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_millis(s.parse()?))
}

fn duration_secs(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_secs(s.parse()?))
}
