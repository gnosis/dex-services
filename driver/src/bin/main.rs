extern crate env_logger;
extern crate graph;
extern crate simple_logger;

use dfusion_core::database::GraphReader;

use driver::contract::SnappContractImpl;
use driver::price_finding::NaiveSolver;
use driver::price_finding::LinearOptimisationPriceFinder;
use driver::price_finding::PriceFinding;
use driver::run_driver_components;

use graph::log::logger;
use graph_node_reader::Store as GraphNodeReader;

use std::thread;
use std::time::Duration;
use std::env;

fn main() {
    // driver logger
    simple_logger::init_with_level(log::Level::Info).unwrap();

    // graph logger
    let logger = logger(false);
    let postgres_url = env::var("POSTGRES_URL").expect("Specify POSTGRES_URL variable");
    let store_reader = GraphNodeReader::new(postgres_url, &logger);
    let db_instance = GraphReader::new(Box::new(store_reader));
    let contract = SnappContractImpl::new().unwrap();

    let solver_env_var = env::var("LINEAR_OPTIMIZATION_SOLVER")
        .unwrap_or_else(|_| "0".to_string());
    let mut price_finder: Box<dyn PriceFinding> = if solver_env_var == "1" {
        Box::new(LinearOptimisationPriceFinder::new())
    } else {
        Box::new(NaiveSolver {})
    };
    loop {
        // Start driver_components
        run_driver_components(&db_instance, &contract, &mut *price_finder);
        thread::sleep(Duration::from_secs(5));
    }
}