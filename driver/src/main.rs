mod contracts;
mod driver;
mod error;
mod logging;
mod metrics;
mod models;
mod orderbook;
mod price_finding;
mod solution_submission;
mod transport;
mod util;

use crate::contracts::{stablex_contract::BatchExchange, web3_provider};
use crate::driver::stablex_driver::StableXDriver;
use crate::metrics::{MetricsServer, StableXMetrics};
use crate::orderbook::{FilteredOrderbookReader, OrderbookFilter, PaginatedStableXOrderBookReader};
use crate::price_finding::price_finder_interface::OptimizationModel;
use crate::price_finding::Fee;
use crate::solution_submission::StableXSolutionSubmitter;

use ethcontract::PrivateKey;
use log::{error, info};
use prometheus::Registry;
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
    #[structopt(long, env = "OPTIMIZATION_MODEL", default_value = "NAIVE")]
    optimization_model: OptimizationModel,

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
}

fn main() {
    let options = Options::from_args();

    let (_, _guard) = logging::init(&options.log_filter);
    info!("using options: {:#?}", options);

    let (web3, _event_loop_handle) = web3_provider(options.node_url.as_str()).unwrap();
    let contract =
        BatchExchange::new(&web3, options.private_key.clone(), options.network_id).unwrap();
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
    let mut price_finder = util::create_price_finder(fee, options.optimization_model);

    let orderbook =
        PaginatedStableXOrderBookReader::new(&contract, options.auction_data_page_size, &web3);
    info!("Orderbook filter: {:?}", options.orderbook_filter);
    let filtered_orderbook =
        FilteredOrderbookReader::new(&orderbook, options.orderbook_filter);

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
