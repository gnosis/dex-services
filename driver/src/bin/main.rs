extern crate simple_logger;

use driver::contract::SnappContractImpl;
use driver::db_interface::MongoDB;
use driver::price_finding::NaiveSolver;
use driver::price_finding::LinearOptimisationPriceFinder;
use driver::price_finding::PriceFinding;
use driver::run_driver_components;

use std::thread;
use std::time::Duration;
use std::env;

fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let db_host = env::var("DB_HOST").expect("Specify DB_HOST env variable");
    let db_port = env::var("DB_PORT").expect("Specify DB_PORT env variable");
    let db_instance = MongoDB::new(db_host, db_port).unwrap();
    let contract = SnappContractImpl::new().unwrap();

    let solver_env_var = env::var("LINEAR_OPTIMIZATION_SOLVER")
        .unwrap_or_else(|_| "0".to_string());
    let mut price_finder : Box<dyn PriceFinding> = if solver_env_var == "1" {
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