
mod db_interface;
mod deposit_driver;

use std::thread;
use std::time::Duration;
use crate::deposit_driver::deposit_driver::run_deposit_listener; 

fn main() {
	loop {
		if let Err(e) = run_deposit_listener() {
			println!("Application error: {}", e);
			()
		}
		thread::sleep(Duration::from_secs(5));
	}
}
