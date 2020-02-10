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
use crate::orderbook::{FilteredOrderbookReader, PaginatedStableXOrderBookReader};
use crate::price_finding::price_finder_interface::OptimizationModel;
use crate::price_finding::Fee;
use crate::solution_submission::StableXSolutionSubmitter;

use log::{error, info};

use prometheus::Registry;

use std::env;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn auction_data_batch_size() -> u64 {
    const KEY: &str = "AUCTION_DATA_BATCH_SIZE";
    const DEFAULT: u64 = 100;
    env::var(KEY)
        .map(|str| {
            str.parse()
                .map_err(|err| format!("couldn't parse {} environment variable: {}", KEY, err))
                .unwrap()
        })
        .unwrap_or(DEFAULT)
}

fn main() {
    let (_, _guard) = logging::init();

    // Environment variable parsing
    let filter = env::var("ORDERBOOK_FILTER").unwrap_or_else(|_| String::from("{}"));
    let ethereum_node_url =
        env::var("ETHEREUM_NODE_URL").expect("ETHEREUM_NODE_URL env var not set");
    let network_id = env::var("NETWORK_ID")
        .map(|s| s.parse().expect("Cannot parse NETWORK_ID"))
        .expect("NETWORK_ID env var not set");

    let optimization_model_string: String =
        env::var("OPTIMIZATION_MODEL").unwrap_or_else(|_| String::from("NAIVE"));
    let optimization_model = OptimizationModel::from(optimization_model_string.as_str());

    let (web3, _event_loop_handle) = web3_provider(&ethereum_node_url).unwrap();
    let contract = BatchExchange::new(&web3, network_id).unwrap();
    info!("Using contract at {}", contract.address());
    info!("Using account {}", contract.account());

    // Set up metrics and serve in separate thread
    let prometheus_registry = Arc::new(Registry::new());
    let stablex_metrics = StableXMetrics::new(prometheus_registry.clone());
    let metric_server = MetricsServer::new(prometheus_registry);
    thread::spawn(move || {
        metric_server.serve(9586);
    });

    let fee = Some(Fee::default());
    let mut price_finder = util::create_price_finder(fee, optimization_model);

    let orderbook =
        PaginatedStableXOrderBookReader::new(&contract, auction_data_batch_size(), &web3);
    let parsed_filter = serde_json::from_str(&filter)
        .map_err(|e| {
            error!("Error parsing orderbook filter: {}", &e);
            e
        })
        .unwrap_or_default();
    info!("Orderbook filter: {:?}", parsed_filter);
    let filtered_orderbook = FilteredOrderbookReader::new(&orderbook, parsed_filter);

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
