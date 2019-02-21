use driver::run_driver_components;
use std::thread;
use std::time::Duration;

fn main() {
    loop {
        //start driver_componets
        if let Err(e) = run_driver_components() {
            println!("Deposit_driver error: {}", e);
        }
        thread::sleep(Duration::from_secs(5));
    }
}