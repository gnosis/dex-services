use dfusion_core::database::GraphReader;

use driver::contracts::snapp_contract::SnappContractImpl;
use driver::driver::order_driver::OrderProcessor;
use driver::logging;
use driver::price_finding::SnappNaiveSolver;
use driver::run_driver_components;

use graph_node_reader::Store as GraphNodeReader;

use std::env;
use std::thread;
use std::time::Duration;

fn main() {
    let logger = logging::init().unwrap();

    let postgres_url = env::var("POSTGRES_URL").expect("Specify POSTGRES_URL variable");
    let store_reader = GraphNodeReader::new(postgres_url, &logger);
    let db_instance = GraphReader::new(Box::new(store_reader));

    let snapp_contract = SnappContractImpl::new().unwrap();

    let mut price_finder = driver::util::create_price_finder(None, SnappNaiveSolver::new(None));
    let mut order_processor =
        OrderProcessor::new(&db_instance, &snapp_contract, &mut *price_finder);
    loop {
        // Start driver_components
        run_driver_components(&db_instance, &snapp_contract, &mut order_processor);
        thread::sleep(Duration::from_secs(5));
    }
}
