use driver::contracts::stablex_contract::BatchExchange;
use driver::driver::stablex_driver::StableXDriver;
use driver::logging;
use driver::metrics::{MetricsServer, StableXMetrics};
use driver::orderbook::{FilteredOrderbookReader, StableXOrderBookReader};
use driver::price_finding::Fee;
use driver::solution_submission::StableXSolutionSubmitter;

use log::{error, info};

use prometheus::Registry;

use std::env;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() {
    let (_, _guard) = logging::init();

    let (contract, _event_loop) = BatchExchange::new().unwrap();
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
    let mut price_finder = driver::util::create_price_finder(fee);

    let orderbook = StableXOrderBookReader::new(&contract);
    let filter = env::var("ORDERBOOK_FILTER").unwrap_or_default();
    info!("Orderbook filter: {}", filter);
    let filtered_orderbook = FilteredOrderbookReader::new(
        &orderbook,
        serde_json::from_str(&filter).unwrap_or_default(),
    );

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
