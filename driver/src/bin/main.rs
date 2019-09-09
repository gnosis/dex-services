extern crate graph;
extern crate simple_logger;

use dfusion_core::database::GraphReader;

use driver::contracts::snapp_contract::SnappContractImpl;
use driver::contracts::base_contract::BaseContract;
use driver::order_driver::OrderProcessor;
use driver::price_finding::LinearOptimisationPriceFinder;
use driver::price_finding::NaiveSolver;
use driver::price_finding::PriceFinding;
use driver::run_driver_components;

use graph::log::logger;
use graph_node_reader::Store as GraphNodeReader;

use std::env;
use std::fs;
use std::thread;
use std::time::Duration;

fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let graph_logger = logger(false);
    let postgres_url = env::var("POSTGRES_URL").expect("Specify POSTGRES_URL variable");
    let store_reader = GraphNodeReader::new(postgres_url, &graph_logger);
    let db_instance = GraphReader::new(Box::new(store_reader));

    let contract_json = fs::read_to_string("dex-contracts/build/contracts/SnappAuction.json").unwrap();
    let address = env::var("SNAPP_CONTRACT_ADDRESS").unwrap();
    let dfusion_contract = SnappContractImpl::new(
        BaseContract::new(address, contract_json).unwrap()
    );

    let solver_env_var = env::var("LINEAR_OPTIMIZATION_SOLVER").unwrap_or_else(|_| "0".to_string());
    let mut price_finder: Box<dyn PriceFinding> = if solver_env_var == "1" {
        Box::new(LinearOptimisationPriceFinder::new())
    } else {
        Box::new(NaiveSolver::new(None))
    };

    let mut order_processor = OrderProcessor::new(&db_instance, &dfusion_contract, &mut *price_finder);
    loop {
        // Start driver_components
        run_driver_components(&db_instance, &dfusion_contract, &mut order_processor);
        thread::sleep(Duration::from_secs(5));
    }
}
