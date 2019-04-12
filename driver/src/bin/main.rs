use driver::contract::SnappContractImpl;
use driver::db_interface::MongoDB;
use driver::price_finding::linear_optimization_price_finder::LinearOptimisationPriceFinder;
use driver::run_driver_components;

use std::thread;
use std::time::Duration;
use std::env;


fn main() {
    let db_host = env::var("DB_HOST").expect("Specify DB_HOST env variable");
    let db_port = env::var("DB_PORT").expect("Specify DB_PORT env variable");
    let db_instance = MongoDB::new(db_host, db_port).unwrap();
    let contract = SnappContractImpl::new().unwrap();
    // TODO check env variable and use simple solver instead
    let mut price_finder = LinearOptimisationPriceFinder::new();
    loop {
        // Start driver_components
        run_driver_components(&db_instance, &contract, &mut price_finder);
        thread::sleep(Duration::from_secs(5));
    }
}