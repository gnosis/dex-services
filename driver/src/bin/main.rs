use driver::run_driver_components;
use std::thread;
use std::time::Duration;

fn main() {
    loop {
        //start driver_componets
        run_driver_components();
        thread::sleep(Duration::from_secs(5));
    }
}