use core::contracts::{stablex_contract::StableXContractImpl, web3_provider, Web3};
use core::driver::{
    scheduler::{AuctionTimingConfiguration, SchedulerKind},
    stablex_driver::StableXDriverImpl,
};
use core::economic_viability::{
    EconomicViabilityComputer, FixedEconomicViabilityComputer, PriorityEconomicViabilityComputer,
};
use core::gas_price::{self, GasPriceEstimating};
use core::http::HttpFactory;
use core::logging;
use core::metrics::{HttpMetrics, MetricsServer, StableXMetrics};
use core::orderbook::{
    FilteredOrderbookReader, OnchainFilteredOrderBookReader, OrderbookFilter, OrderbookReaderKind,
    ShadowedOrderbookReader, StableXOrderBookReading,
};
use core::price_estimation::PriceOracle;
use core::price_finding::{self, Fee, InternalOptimizer, SolverType};
use core::solution_submission::StableXSolutionSubmitter;
use core::token_info::hardcoded::TokenData;
use core::util::FutureWaitExt as _;

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

#[derive(Debug, StructOpt)]
#[structopt(
    name = "driver",
    about = "Gnosis Exchange protocol driver.",
    rename_all = "kebab"
)]
struct Options {
    /// The log filter to use.
    ///
    /// This follows the `slog-envlogger` syntax (e.g. 'info,driver=debug').
    #[structopt(
        long,
        env = "DFUSION_LOG",
        default_value = "warn,driver=info,core=info"
    )]
    log_filter: String,

    /// The Ethereum node URL to connect to. Make sure that the node allows for
    /// queries without a gas limit to be able to fetch the orderbook.
    #[structopt(short, long, env = "ETHEREUM_NODE_URL")]
    node_url: Url,

    /// The network ID used for signing transactions (e.g. 1 for mainnet, 4 for
    /// rinkeby, 5777 for ganache).
    #[structopt(short = "i", long, env = "NETWORK_ID")]
    network_id: u64,

    /// Which style of solver to use. Can be one of:
    /// 'naive-solver' for the naive solver;
    /// 'standard-solver' for mixed integer programming solver;
    /// 'fallback-solver' for a more conservative solver than the standard solver;
    /// 'best-ring-solver' for a solver searching only for the best ring;
    /// 'open-solver' for the open-source solver
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
    ///   },
    ///   "T0004": {
    ///     "alias": "USDC",
    ///     "decimals": 6,
    ///     "externalPrice": 1000000000000000000000000000000,
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

    /// Primary method for orderbook retrieval
    #[structopt(long, env = "PRIMARY_ORDERBOOK", default_value = "eventbased")]
    primary_orderbook: OrderbookReaderKind,

    /// The private key used by the driver to sign transactions.
    #[structopt(short = "k", long, env = "PRIVATE_KEY", hide_env_values = true)]
    private_key: PrivateKey,

    /// For storage based orderbook reading, the page size with which to read
    /// orders from the smart contract. For event based orderbook reading, the
    /// number of blocks to fetch events for at a time.
    #[structopt(long, env = "AUCTION_DATA_PAGE_SIZE", default_value = "500")]
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
        env = "LATEST_SOLUTION_SUBMIT_TIME",
        default_value = "210",
        parse(try_from_str = duration_secs),
    )]
    latest_solution_submit_time: Duration,

    /// The earliest offset from the start of a batch in seconds at which point we should submit the
    /// solution. This is useful when there are multiple solvers one of provides solutions more
    /// often but also worse solutions than the others. By submitting its solutions later we avoid
    /// its solution getting reverted by a better one which saves gas.
    #[structopt(
        long,
        env = "EARLIEST_SOLUTION_SUBMIT_TIME",
        default_value = "0",
        parse(try_from_str = duration_secs),
    )]
    earliest_solution_submit_time: Duration,

    /// Subsidy factor used to compute the minimum average fee per order in a
    /// solution as well as the gas cap for economically viable solution.
    #[structopt(
        long,
        env = "ECONOMIC_VIABILITY_SUBSIDY_FACTOR",
        default_value = "10.0"
    )]
    economic_viability_subsidy_factor: f64,

    /// We multiply the economically viable min average fee by this amount to ensure that if a
    /// solution has this minimum amount it will still be end up economically viable even when the
    /// gas or eth price moves slightly between solution computation and submission.
    #[structopt(
        long,
        env = "ECONOMIC_VIABILITY_MIN_AVG_FEE_FACTOR",
        default_value = "1.1"
    )]
    economic_viability_min_avg_fee_factor: f64,

    /// The default minimum average fee per order. This is passed to the solver
    /// in case the computing its value fails. Its unit is [OWL]
    #[structopt(long, env = "MIN_AVG_FEE_PER_ORDER", default_value = "0")]
    default_min_avg_fee_per_order: u128,

    /// The default maximum gas price. This is used when computing the maximum gas price based on
    /// ether price in owl fails.
    #[structopt(long, env = "DEFAULT_MAX_GAS_PRICE", default_value = "100000000000")]
    default_max_gas_price: u128,

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
    let stablex_metrics = Arc::new(stablex_metrics);

    // Set up shared HTTP client and HTTP services.
    let http_factory = HttpFactory::new(options.http_timeout, http_metrics);
    let (web3, gas_station) = setup_http_services(&http_factory, &options);

    // Set up connection to exchange contract
    let contract = Arc::new(
        StableXContractImpl::new(&web3, options.private_key.clone(), options.network_id)
            .wait()
            .unwrap(),
    );
    info!("Using contract at {:?}", contract.address());
    info!("Using account {:?}", contract.account());

    // Create the orderbook reader.
    let primary_orderbook = options.primary_orderbook.create(
        contract.clone(),
        options.auction_data_page_size,
        &options.orderbook_filter,
        web3,
        options.orderbook_file,
    );

    info!("Orderbook filter: {:?}", options.orderbook_filter);
    let filtered_orderbook = Box::new(FilteredOrderbookReader::new(
        primary_orderbook,
        options.orderbook_filter.clone(),
    ));

    // NOTE: Keep the shadowed orderbook around so it doesn't get dropped and we
    //   can pass a reference to the filtered orderbook reader.
    let orderbook: Arc<dyn StableXOrderBookReading> = if options.use_shadowed_orderbook {
        let shadow_orderbook = Box::new(OnchainFilteredOrderBookReader::new(
            contract.clone(),
            options.auction_data_page_size,
            &options.orderbook_filter,
        ));
        Arc::new(ShadowedOrderbookReader::new(
            filtered_orderbook,
            shadow_orderbook,
        ))
    } else {
        Arc::new(*filtered_orderbook)
    };

    let price_oracle = Arc::new(
        PriceOracle::new(
            &http_factory,
            orderbook.clone(),
            contract.clone(),
            options.token_data,
            options.price_source_update_interval,
        )
        .unwrap(),
    );

    let economic_viability = Arc::new(PriorityEconomicViabilityComputer::new(vec![
        Box::new(EconomicViabilityComputer::new(
            price_oracle.clone(),
            gas_station.clone(),
            options.economic_viability_subsidy_factor,
            options.economic_viability_min_avg_fee_factor,
        )),
        Box::new(FixedEconomicViabilityComputer::new(
            Some(options.default_min_avg_fee_per_order),
            Some(options.default_max_gas_price.into()),
        )),
    ]));

    // Setup price.
    let price_finder = price_finding::create_price_finder(
        Some(Fee::default()),
        options.solver_type,
        price_oracle,
        economic_viability.clone(),
        options.solver_internal_optimizer,
    );

    // Set up solution submitter.
    let solution_submitter = Arc::new(StableXSolutionSubmitter::new(contract.clone(), gas_station));

    // Set up the driver and start the run-loop.
    let driver = StableXDriverImpl::new(
        price_finder,
        orderbook.clone(),
        solution_submitter,
        economic_viability,
        stablex_metrics,
    );

    let scheduler_config = AuctionTimingConfiguration::new(
        options.target_start_solve_time,
        options.latest_solution_submit_time,
        options.earliest_solution_submit_time,
    );

    let mut scheduler = options
        .scheduler
        .create(contract, Arc::new(driver), scheduler_config);
    orderbook
        .initialize()
        .wait()
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
    http_factory: &HttpFactory,
    options: &Options,
) -> (Web3, Arc<dyn GasPriceEstimating + Send + Sync>) {
    let web3 = web3_provider(http_factory, options.node_url.as_str(), options.rpc_timeout).unwrap();
    let gas_station =
        gas_price::create_estimator(options.network_id, &http_factory, &web3).unwrap();
    (web3, gas_station)
}

fn duration_millis(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_millis(s.parse()?))
}

fn duration_secs(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_secs(s.parse()?))
}
