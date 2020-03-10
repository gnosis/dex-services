#[macro_use]
mod macros;

mod contracts;
mod driver;
mod eth_rpc;
mod gas_station;
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
use crate::driver::{scheduler::Scheduler, stablex_driver::StableXDriverImpl};
use crate::eth_rpc::Web3EthRpc;
use crate::gas_station::GnosisSafeGasStation;
use crate::metrics::{MetricsServer, StableXMetrics};
use crate::orderbook::{FilteredOrderbookReader, OrderbookFilter, PaginatedStableXOrderBookReader};
use crate::price_estimation::{PriceOracle, TokenData};
use crate::price_finding::{Fee, SolverConfig};
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
    solver_type: String,

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

    /// Number of seconds the solver should maximally use for the optimization process
    #[structopt(long, env = "SOLVER_TIME_LIMIT", default_value = "150")]
    solver_time_limit: u32,

    /// JSON encoded object of which tokens/orders to ignore.
    ///
    /// For example: '{
    ///   "tokens": [1, 2],
    ///   "users": {
    ///     "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0A": { "OrderIds": [0, 1] },
    ///     "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0B": "All"
    ///   }
    /// }'
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

    /// The timeout in milliseconds of gas station calls, defaults to 10000ms
    #[structopt(
        long,
        env = "GAS_STATION_TIMEOUT",
        default_value = "10000",
        parse(try_from_str = duration_millis),
    )]
    gas_station_timeout: Duration,

    /// The duration in seconds which should be waited at the start of a batch
    /// before beginning to solve it.
    #[structopt(
        long,
        env = "BATCH_WAIT_TIME",
        default_value = "30",
        parse(try_from_str = duration_secs),
    )]
    batch_wait_time: Duration,

    /// The duration in seconds which can have at most been elapsed since the
    /// the start of the current batch if we should still attempt to solve it.
    #[structopt(
        long,
        env = "MAX_BATCH_ELAPSED_TIME",
        default_value = "180",
        parse(try_from_str = duration_secs),
    )]
    max_batch_elapsed_time: Duration,
}

fn main() {
    let options = Options::from_args();
    let (_, _guard) = logging::init(&options.log_filter);
    info!("Starting driver with runtime options: {:#?}", options);
    let solver_config = SolverConfig::new(&options.solver_type, options.solver_time_limit).unwrap();
    let web3 = web3_provider(options.node_url.as_str(), options.rpc_timeout).unwrap();
    let gas_station =
        GnosisSafeGasStation::new(options.gas_station_timeout, gas_station::DEFAULT_URI).unwrap();
    let contract = StableXContractImpl::new(
        &web3,
        options.private_key.clone(),
        options.network_id,
        &gas_station,
    )
    .unwrap();
    info!("Using contract at {:?}", contract.address());
    info!("Using account {:?}", contract.account());

    // Set up metrics and serve in separate thread
    let prometheus_registry = Arc::new(Registry::new());
    let stablex_metrics = StableXMetrics::new(prometheus_registry.clone());
    let metric_server = MetricsServer::new(prometheus_registry);
    thread::spawn(move || {
        metric_server.serve(9586);
    });

    let fee = Some(Fee::default());
    let price_oracle = PriceOracle::new(options.token_data).unwrap();
    let mut price_finder = price_finding::create_price_finder(fee, solver_config, price_oracle);

    let orderbook = PaginatedStableXOrderBookReader::new(&contract, options.auction_data_page_size);
    info!("Orderbook filter: {:?}", options.orderbook_filter);
    let filtered_orderbook = FilteredOrderbookReader::new(&orderbook, options.orderbook_filter);

    let eth_rpc = Web3EthRpc::new(&web3);

    let solution_submitter = StableXSolutionSubmitter::new(&contract, &eth_rpc);
    let mut driver = StableXDriverImpl::new(
        &mut *price_finder,
        &filtered_orderbook,
        &solution_submitter,
        &stablex_metrics,
    );
    let mut scheduler = Scheduler::new(
        &mut driver,
        &filtered_orderbook,
        &stablex_metrics,
        options.batch_wait_time,
        options.max_batch_elapsed_time,
    );
    scheduler.run_forever();
}

fn duration_millis(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_millis(s.parse()?))
}

fn duration_secs(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_secs(s.parse()?))
}
