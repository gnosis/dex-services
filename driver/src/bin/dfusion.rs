use dfusion_core::database::GraphReader;
use driver::price_finding::price_finder_interface::OptimizationModel;

use driver::contracts::snapp_contract::SnappContractImpl;
use driver::driver::order_driver::OrderProcessor;
use driver::logging;
use driver::run_driver_components;

use graph_node_reader::Store as GraphNodeReader;

use log::info;

use std::env;
use std::thread;
use std::time::Duration;

fn main() {
    let (logger, _guard) = logging::init();

    let ethereum_node_url =
        env::var("ETHEREUM_NODE_URL").expect("Specify ETHEREUM_NODE_URL variable");
    let network_id = env::var("NETWORK_ID")
        .map(|s| s.parse().expect("Cannot parse NETWORK_ID"))
        .expect("Specify NETWORK_ID variable");
    let postgres_url = env::var("POSTGRES_URL").expect("Specify POSTGRES_URL variable");

    let optimization_model_string: String =
        env::var("OPTIMIZATION_MODEL").unwrap_or_else(|_| String::from("NAIVE"));
    let optimization_model = OptimizationModel::from(optimization_model_string.as_str());

    let store_reader = GraphNodeReader::new(postgres_url, &logger);
    let db_instance = GraphReader::new(Box::new(store_reader));

    let snapp_contract = SnappContractImpl::new(ethereum_node_url, network_id).unwrap();
    info!("Using contract at {}", snapp_contract.address());
    info!("Using account {}", snapp_contract.account());

    let mut price_finder = driver::util::create_price_finder(None, optimization_model);
    let mut order_processor =
        OrderProcessor::new(&db_instance, &snapp_contract, &mut *price_finder);
    loop {
        // Start driver_components
        run_driver_components(&db_instance, &snapp_contract, &mut order_processor);
        thread::sleep(Duration::from_secs(5));
    }
}
