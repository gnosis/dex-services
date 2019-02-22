extern crate byteorder;
extern crate mongodb;
extern crate rustc_hex;
extern crate web3;
extern crate serde_derive;
extern crate hex;
extern crate serde;
extern crate serde_json;
extern crate sha2;

pub mod contract;
pub mod db_interface;

mod deposit_driver;
mod models;
mod withdraw_driver;

use crate::deposit_driver::run_deposit_listener;
use crate::withdraw_driver::run_withdraw_listener;
use crate::db_interface::MongoDB;
use crate::contract::SnappContractImpl;

pub fn run_driver_components(db: &MongoDB, contract: &SnappContractImpl) -> () {
    if let Err(e) = run_deposit_listener() {
        println!("Deposit_driver error: {}", e);
    }
    if let Err(e) = run_withdraw_listener(db, contract) {
        println!("Withdraw_driver error: {}", e);
    }
    //...
}

