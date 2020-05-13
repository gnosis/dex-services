#![recursion_limit = "256"]

#[macro_use]
mod macros;

mod contracts;
mod driver;
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

use crate::contracts::{stablex_contract::StableXContractImpl, web3_provider, Web3};
use crate::driver::{
    scheduler::{AuctionTimingConfiguration, SchedulerKind},
    stablex_driver::StableXDriverImpl,
};
use crate::gas_station::GnosisSafeGasStation;
use crate::http::HttpFactory;
use crate::metrics::{HttpMetrics, MetricsServer, StableXMetrics};
use crate::orderbook::{
    FilteredOrderbookReader, OnchainFilteredOrderBookReader, OrderbookFilter, OrderbookReaderKind,
    ShadowedOrderbookReader, StableXOrderBookReading,
};
use crate::price_estimation::{PriceOracle, TokenData};
use crate::price_finding::{Fee, InternalOptimizer, SolverType};
use crate::solution_submission::StableXSolutionSubmitter;

use ethcontract::PrivateKey;
use log::info;
use prometheus::Registry;
use std::num::ParseIntError;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use structopt::StructOpt;
use url::Url;

#[derive(Clone, Debug, StructOpt)]
#[structopt(
    name = "driver",
    about = "Gnosis Exchange protocol driver.",
    rename_all = "kebab"
)]
struct Options {
    /// The log filter to use.
    ///
    /// This follows the `slog-envlogger` syntax (e.g. 'info,driver=debug').
    #[structopt(long, env = "DFUSION_LOG", default_value = "info")]
    log_filter: String,

    /// The Ethereum node URL to connect to. Make sure that the node allows for
    /// queries without a gas limit to be able to fetch the orderbook.
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

    /// Which internal optimizer the solver should use. It is passed as
    /// `--solver` to the solver. Choices are "scip" and "gurobi".
    #[structopt(long, env = "SOLVER_INTERNAL_OPTIMIZER", default_value = "scip")]
    solver_internal_optimizer: InternalOptimizer,

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
    ///  }'
    /// More examples can be found in the tests of orderbook/filtered_orderboook.rs
    #[structopt(long, env = "ORDERBOOK_FILTER", default_value = "{}")]
    orderbook_filter: OrderbookFilter,

    /// Primary method for orderbook retrieval ("Paginated" or "OnchainFiltered")
    #[structopt(long, env = "PRIMARY_ORDERBOOK", default_value = "paginated")]
    primary_orderbook: OrderbookReaderKind,

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

    /// Solver parameter: minimal average fee per order
    /// Its unit is [OWL]
    #[structopt(long, env = "MIN_AVG_FEE_PER_ORDER", default_value = "0")]
    min_avg_fee_per_order: u128,

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

    /// Use a shadowed orderbook reader along side a primary reader so that the
    /// queried data can be compared and produce log errors in case they
    /// disagree.
    #[structopt(
        long,
        env = "USE_SHADOWED_ORDERBOOK",
        default_value = "false",
        parse(try_from_str)
    )]
    use_shadowed_orderbook: bool,

    #[structopt(long, env = "ORDERBOOK_FILE", parse(from_os_str))]
    orderbook_file: Option<PathBuf>,
}

fn main() {
    let options = Options::from_args();
    let (_, _guard) = logging::init(&options.log_filter);
    info!("Starting driver with runtime options: {:#?}", options);

    // Set up metrics and serve in separate thread.
    let (stablex_metrics, http_metrics) = setup_metrics();

    // Set up shared HTTP client and HTTP services.
    let http_factory = HttpFactory::new(options.http_timeout, http_metrics);
    let (web3, gas_station, price_oracle) = setup_http_services(http_factory, options.clone());

    // Set up connection to exchange contract
    let contract = Arc::new(
        StableXContractImpl::new(&web3, options.private_key.clone(), options.network_id).unwrap(),
    );
    info!("Using contract at {:?}", contract.address());
    info!("Using account {:?}", contract.account());

    // Setup price.
    let price_finder = price_finding::create_price_finder(
        Some(Fee::default()),
        options.solver_type,
        price_oracle,
        options.min_avg_fee_per_order,
        options.solver_internal_optimizer,
    );

    // Create the orderbook reader.
    let primary_orderbook = options.primary_orderbook.create(
        contract.clone(),
        options.auction_data_page_size,
        &options.orderbook_filter,
        web3,
        options.orderbook_file,
    );

    info!("Orderbook filter: {:?}", options.orderbook_filter);
    let filtered_orderbook =
        FilteredOrderbookReader::new(&*primary_orderbook, options.orderbook_filter.clone());

    // NOTE: Keep the shadowed orderbook around so it doesn't get dropped and we
    //   can pass a reference to the filtered orderbook reader.
    let orderbook: Box<dyn StableXOrderBookReading + Sync> = if options.use_shadowed_orderbook {
        let shadow_orderbook = OnchainFilteredOrderBookReader::new(
            contract.clone(),
            options.auction_data_page_size,
            &options.orderbook_filter,
        );
        let shadowed_orderbook =
            ShadowedOrderbookReader::new(&filtered_orderbook, shadow_orderbook);
        Box::new(shadowed_orderbook)
    } else {
        Box::new(filtered_orderbook)
    };

    // Set up solution submitter.
    let solution_submitter = StableXSolutionSubmitter::new(&*contract, &gas_station);

    // Set up the driver and start the run-loop.
    let driver = StableXDriverImpl::new(
        &*price_finder,
        &*orderbook,
        &solution_submitter,
        &stablex_metrics,
    );

    let scheduler_config =
        AuctionTimingConfiguration::new(options.target_start_solve_time, options.solver_time_limit);

    let mut scheduler = options
        .scheduler
        .create(&*contract, &driver, scheduler_config);
    orderbook
        .initialize()
        .expect("primary orderbook initialization failed");
    scheduler.start();
}

fn setup_metrics() -> (StableXMetrics, HttpMetrics) {
    let prometheus_registry = Arc::new(Registry::new());
    let stablex_metrics = StableXMetrics::new(prometheus_registry.clone());
    let http_metrics = HttpMetrics::new(&prometheus_registry).unwrap();
    let metric_server = MetricsServer::new(prometheus_registry);
    thread::spawn(move || {
        metric_server.serve(9586);
    });

    (stablex_metrics, http_metrics)
}

fn setup_http_services(
    http_factory: HttpFactory,
    options: Options,
) -> (Web3, GnosisSafeGasStation, PriceOracle) {
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
    (web3, gas_station, price_oracle)
}

fn duration_millis(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_millis(s.parse()?))
}

fn duration_secs(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_secs(s.parse()?))
}
