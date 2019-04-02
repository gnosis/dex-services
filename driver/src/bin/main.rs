use driver::contract::SnappContractImpl;
use driver::db_interface::MongoDB;
use driver::run_driver_components;

use std::thread;
use std::time::Duration;
use std::env;


fn main() {
    let db_host = env::var("DB_HOST").expect("Specify DB_HOST env variable");
    let db_port = env::var("DB_PORT").expect("Specify DB_PORT env variable");;
    let db_instance = MongoDB::new(db_host, db_port).unwrap();
    let contract = SnappContractImpl::new().unwrap();
    loop {
        // Start driver_components
        run_driver_components(&db_instance, &contract);
        thread::sleep(Duration::from_secs(5));
    }
}