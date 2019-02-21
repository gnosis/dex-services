extern crate byteorder;
extern crate mongodb;
extern crate rustc_hex;
extern crate web3;
extern crate serde_derive;
extern crate hex;
extern crate serde;
extern crate serde_json;
extern crate sha2;

mod db_interface;
mod deposit_driver;
mod models;
mod error;

use std::error::Error;

use crate::deposit_driver::run_deposit_listener;

pub fn run_driver_components() -> Result<(), Box<dyn Error>> {
    //start deposit_driver
    if let Err(e) = run_deposit_listener() {
        println!("Deposit_driver error: {}", e);
        ()
    }
    //start withdraw_driver
    //...
    Ok(())
}

