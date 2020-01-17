use driver::contracts::stablex_contract::BatchExchange;
use driver::driver::stablex_driver::StableXDriver;
use driver::logging;
use driver::metrics::MetricsServer;
use driver::price_finding::Fee;

use log::{error, info};

use prometheus::Registry;

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
    let metric_server = MetricsServer::new(prometheus_registry);
    thread::spawn(move || {
        metric_server.serve(9586);
    });

    let fee = Some(Fee::default());
    let mut price_finder = driver::util::create_price_finder(fee);
    let mut driver = StableXDriver::new(&contract, &mut *price_finder);
    loop {
        if let Err(e) = driver.run() {
            error!("StableXDriver error: {}", e);
        }
        thread::sleep(Duration::from_secs(5));
    }
}
