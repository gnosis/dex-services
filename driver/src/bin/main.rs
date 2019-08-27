extern crate graph;
extern crate simple_logger;

use dfusion_core::database::GraphReader;

use driver::contract::SnappContractImpl;
use driver::price_finding::{
    NaiveSolver,
    LinearOptimisationPriceFinder,
    PriceFinding
};
use driver::run_driver_components;

use graph::log::logger;
use graph_node_reader::Store as GraphNodeReader;

use std::env;
use std::thread;
use std::time::Duration;

fn main() {
    // driver logger
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let graph_logger = logger(false);
    
    let postgres_url = env::var("POSTGRES_URL").expect("Specify POSTGRES_URL variable");
    let store_reader = GraphNodeReader::new(postgres_url, &graph_logger);
    let db_instance = GraphReader::new(Box::new(store_reader));
    let contract = SnappContractImpl::new().unwrap();

    let solver_env_var = env::var("LINEAR_OPTIMIZATION_SOLVER").unwrap_or_else(|_| "0".to_string());
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
