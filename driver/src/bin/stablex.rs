use driver::contracts::stablex_contract::BatchExchange;
use driver::driver::stablex_driver::StableXDriver;
use driver::price_finding::Fee;

use log::{error, info};

use std::thread;
use std::time::Duration;

fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();

    let (contract, _event_loop) = BatchExchange::new().unwrap();
    info!("Using contract at {}", contract.address());
    info!("Using account {}", contract.account());

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
