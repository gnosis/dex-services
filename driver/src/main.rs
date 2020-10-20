use services_core::contracts::{stablex_contract::StableXContractImpl, web3_provider, Web3};
use services_core::driver::{
    scheduler::{AuctionTimingConfiguration, SchedulerKind},
    stablex_driver::StableXDriverImpl,
};
use services_core::economic_viability::EconomicViabilityStrategy;
use services_core::gas_price::{self, GasPriceEstimating};
use services_core::health::{HealthReporting, HttpHealthEndpoint};
use services_core::http::HttpFactory;
use services_core::http_server::{DefaultRouter, RouilleServer, Serving};
use services_core::logging;
use services_core::metrics::{HttpMetrics, MetricsHandler, SolverMetrics, StableXMetrics};
use services_core::orderbook::{
    EventBasedOrderbook, FilteredOrderbookReader, OrderbookFilter, StableXOrderBookReading,
};
use services_core::price_estimation::PriceOracle;
use services_core::price_finding::{self, Fee, InternalOptimizer, SolverType};
use services_core::solution_submission::StableXSolutionSubmitter;
use services_core::token_info::hardcoded::TokenData;
use services_core::util::FutureWaitExt as _;

use ethcontract::PrivateKey;
use log::info;
use prometheus::Registry;
use std::num::ParseIntError;
use std::path::PathBuf;
use std::sync::Arc;
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
        env = "LOG_FILTER",
        default_value = "warn,driver=info,services_core=info"
    )]
    log_filter: String,

    /// The Ethereum node URL to connect to. Make sure that the node allows for
    /// queries without a gas limit to be able to fetch the orderbook.
    #[structopt(short, long, env = "NODE_URL")]
    node_url: Url,

    /// Which style of solver to use. Can be one of:
    /// 'NaiveSolver' for the naive solver;
    /// 'StandardSolver' for mixed integer programming solver;
    /// 'FallbackSolver' for a more conservative solver than the standard solver;
    /// 'BestRingSolver' for a solver searching only for the best ring;
    /// 'OpenSolver' for the open-source solver
    #[structopt(
        long,
        env = "SOLVER_TYPE",
        default_value = "NaiveSolver",
        possible_values = SolverType::variant_names(),
        case_insensitive = true,
    )]
    solver_type: SolverType,

    /// Which internal optimizer the solver should use. It is passed as
    /// `--solver` to the solver. Choices are "scip" and "gurobi".
    #[structopt(
        long,
        env = "SOLVER_INTERNAL_OPTIMIZER",
        default_value = "Scip",
        possible_values = InternalOptimizer::variant_names(),
        case_insensitive = true,
    )]
    solver_internal_optimizer: InternalOptimizer,

    /// JSON encoded backup token information to provide to the solver.
    ///
    /// For example: '{
    ///   "T0001": {
    ///     "address": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
    ///     "alias": "WETH",
    ///     "decimals": 18,
    ///     "externalPrice": 200000000000000000000,
    ///   },
    ///   "T0004": {
    ///     "address": "0x0000000000000000000000000000000000000000",
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

    /// The private key used by the driver to sign transactions.
    #[structopt(short = "k", long, env = "PRIVATE_KEY", hide_env_values = true)]
    private_key: PrivateKey,

    /// Specify the number of blocks to fetch events for at a time for
    /// constructing the orderbook for the solver.
    #[structopt(long, env = "AUCTION_DATA_PAGE_SIZE", default_value = "500")]
    auction_data_page_size: usize,

    /// The timeout in milliseconds of web3 JSON RPC calls, defaults to 10000ms
    #[structopt(
        long,
        env = "RPC_TIMEOUT",
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
    /// gas or native token price moves slightly between solution computation and submission.
    #[structopt(
        long,
        env = "ECONOMIC_VIABILITY_MIN_AVG_FEE_FACTOR",
        default_value = "1.1"
    )]
    economic_viability_min_avg_fee_factor: f64,

    /// The fallback minimum average fee per order. This is passed to the solver
    /// in case the computing its value fails. Its unit is [OWL]
    #[structopt(long, env = "FALLBACK_MIN_AVG_FEE_PER_ORDER", default_value = "0")]
    fallback_min_avg_fee_per_order: u128,

    /// The fallback maximum gas price. This is used when computing the maximum gas price based on
    /// ether price in owl fails.
    #[structopt(long, env = "FALLBACK_MAX_GAS_PRICE", default_value = "100000000000")]
    fallback_max_gas_price: u128,

    /// How to calculate the economic viability constraints.
    /// `Static`: Use fallback_min_avg_fee_per_order and fallback_max_gas_price.
    /// `Dynamic`: Use current native token price, gas price and subsidy factor. Use Static if cannot get prices.
    /// `DynamicBoundedByStatic`: Use Dynamic first. If it fails or the result is worse (larger min-avg-fee, lower max-gas-price) than Static, use Static instead.
    #[structopt(
        long,
        env = "ECONOMIC_VIABILITY_STRATEGY",
        default_value = "Dynamic",
        possible_values = EconomicViabilityStrategy::variant_names(),
        case_insensitive = true,
    )]
    economic_viability_strategy: EconomicViabilityStrategy,

    /// The kind of scheduler to use.
    #[structopt(
        long,
        env = "SCHEDULER",
        default_value = "System",
        possible_values = SchedulerKind::variant_names(),
        case_insensitive = true,
    )]
    scheduler: SchedulerKind,

    /// Time interval in seconds in which price sources should be updated.
    #[structopt(
        long,
        env = "PRICE_SOURCE_UPDATE_INTERVAL",
        default_value = "300",
        parse(try_from_str = duration_secs),
    )]
    price_source_update_interval: Duration,

    /// Use an orderbook file for persisting an event cache in order to speed up
    /// the startup time.
    #[structopt(long, env = "ORDERBOOK_FILE", parse(from_os_str))]
    orderbook_file: Option<PathBuf>,

    /// ID for the token which is used to pay network transaction fees on the
    /// target chain (e.g. WETH on mainnet, DAI on xDAI).
    #[structopt(long, env = "NATIVE_TOKEN_ID", default_value = "1")]
    native_token_id: u16,

    /// Whether to rely on external price sources (e.g. 1Inch, Kraken etc)
    /// when estimating token prices
    #[structopt(
        long,
        env = "USE_EXTERNAL_PRICE_SOURCE",
        parse(try_from_str),
        default_value = "true"
    )]
    use_external_price_source: bool,
}

fn main() {
    let options = Options::from_args();
    let (_, _guard) = logging::init(&options.log_filter);
    info!("Starting driver with runtime options: {:#?}", options);

    // Set up metrics and health monitoring and serve in separate thread.
    let (stablex_metrics, http_metrics, solver_metrics, health) = setup_monitoring();

    // Set up shared HTTP client and HTTP services.
    let http_factory = HttpFactory::new(options.http_timeout, http_metrics);
    let (web3, gas_station) = setup_http_services(&http_factory, &options).wait();

    // Set up connection to exchange contract
    let contract = Arc::new(
        StableXContractImpl::new(&web3, options.private_key.clone())
            .wait()
            .unwrap(),
    );
    info!("Using contract at {:?}", contract.address());
    info!("Using account {:?}", contract.account());

    info!("Orderbook filter: {:?}", options.orderbook_filter);
    let orderbook = Arc::new(FilteredOrderbookReader::new(
        Box::new(EventBasedOrderbook::new(
            contract.clone(),
            web3,
            options.auction_data_page_size,
            options.orderbook_file,
        )),
        options.orderbook_filter.clone(),
    ));

    let price_oracle = Arc::new(
        PriceOracle::new(
            &http_factory,
            orderbook.clone(),
            contract.clone(),
            options.token_data,
            options.price_source_update_interval,
            options.native_token_id.into(),
            options.use_external_price_source,
        )
        .unwrap(),
    );

    let economic_viability = options.economic_viability_strategy.from_arguments(
        options.economic_viability_subsidy_factor,
        options.economic_viability_min_avg_fee_factor,
        options.fallback_min_avg_fee_per_order,
        options.fallback_max_gas_price,
        price_oracle.clone(),
        gas_station.clone(),
    );

    // Setup price.
    let price_finder = price_finding::create_price_finder(
        Some(Fee::default()),
        options.solver_type,
        price_oracle,
        economic_viability.clone(),
        options.solver_internal_optimizer,
        solver_metrics,
        stablex_metrics.clone(),
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

    let mut scheduler =
        options
            .scheduler
            .create(contract, Arc::new(driver), scheduler_config, health);
    orderbook
        .initialize()
        .wait()
        .expect("primary orderbook initialization failed");
    scheduler.start();
}

fn setup_monitoring() -> (
    Arc<StableXMetrics>,
    HttpMetrics,
    SolverMetrics,
    Arc<dyn HealthReporting>,
) {
    let health = Arc::new(HttpHealthEndpoint::new());

    let prometheus_registry = Arc::new(Registry::new());
    let stablex_metrics = Arc::new(StableXMetrics::new(prometheus_registry.clone()));
    let http_metrics = HttpMetrics::new(&prometheus_registry).unwrap();
    let solver_metrics = SolverMetrics::new(prometheus_registry.clone());

    let metric_handler = MetricsHandler::new(prometheus_registry);
    RouilleServer::new(DefaultRouter {
        metrics: Arc::new(metric_handler),
        health_readiness: health.clone(),
    })
    .start_in_background();

    (stablex_metrics, http_metrics, solver_metrics, health)
}

async fn setup_http_services(
    http_factory: &HttpFactory,
    options: &Options,
) -> (Web3, Arc<dyn GasPriceEstimating + Send + Sync>) {
    let web3 = web3_provider(http_factory, options.node_url.as_str(), options.rpc_timeout).unwrap();
    let gas_station = gas_price::create_estimator(&http_factory, &web3)
        .await
        .unwrap();
    (web3, gas_station)
}

fn duration_millis(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_millis(s.parse()?))
}

fn duration_secs(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_secs(s.parse()?))
}
