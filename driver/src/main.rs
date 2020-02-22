#[macro_use]
mod macros;

mod contracts;
mod driver;
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
use crate::driver::stablex_driver::StableXDriver;
use crate::gas_station::GnosisSafeGasStation;
use crate::metrics::{MetricsServer, StableXMetrics};
use crate::models::TokenData;
use crate::orderbook::{FilteredOrderbookReader, OrderbookFilter, PaginatedStableXOrderBookReader};
use crate::price_finding::{Fee, SolverConfig};
use crate::solution_submission::StableXSolutionSubmitter;

use ethcontract::PrivateKey;
use log::{error, info};
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
    #[structopt(long, env = "SOLVER_TYPE", default_value = "NaiveSolver")]
    solver_type: String,

    /// JSON encoded backup token information to provide to the solver.
    ///
    /// For example: '{
    ///   "T0001": {
    ///     "alias": "WETH",
    ///     "decimals": 18,
    ///     "external_price": 200000000000000000000
    ///   },
    ///   "T0004": {
    ///     "alias": "USDC",
    ///     "decimals": 6,
    ///     "external_price": 1000000000000000000000000000000,
    ///   }
    /// }'
    #[structopt(long, env = "PRICE_FEED_INFORMATION", default_value = "{}")]
    backup_token_data: TokenData,

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
}

fn main() {
    let options = Options::from_args();
    let (_, _guard) = logging::init(&options.log_filter);
    info!("Starting driver with runtime options: {:#?}", options);
    let solver_config = SolverConfig::new(&options.solver_type, options.solver_time_limit);
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
    let mut price_finder =
        price_finding::create_price_finder(fee, solver_config, options.backup_token_data);

    let orderbook = PaginatedStableXOrderBookReader::new(&contract, options.auction_data_page_size);
    info!("Orderbook filter: {:?}", options.orderbook_filter);
    let filtered_orderbook = FilteredOrderbookReader::new(&orderbook, options.orderbook_filter);

    let solution_submitter = StableXSolutionSubmitter::new(&contract);
    let mut driver = StableXDriver::new(
        &mut *price_finder,
        &filtered_orderbook,
        &solution_submitter,
        stablex_metrics,
    );
    loop {
        if let Err(e) = driver.run() {
            error!("StableXDriver error: {}", e);
        }
        thread::sleep(Duration::from_secs(5));
    }
}

fn duration_millis(s: &str) -> Result<Duration, ParseIntError> {
    Ok(Duration::from_millis(s.parse()?))
}
