extern crate byteorder;
extern crate hex;
extern crate mongodb;
extern crate rustc_hex;
extern crate serde;
extern crate serde_derive;
extern crate serde_json;
extern crate sha2;
extern crate web3;

use crate::contract::SnappContractImpl;
use crate::db_interface::MongoDB;
use crate::deposit_driver::run_deposit_listener;
use crate::withdraw_driver::run_withdraw_listener;

pub mod contract;
pub mod db_interface;
pub mod error;
pub mod price_finding;

mod deposit_driver;
pub mod models;
mod withdraw_driver;

pub fn run_driver_components(db: &MongoDB, contract: &SnappContractImpl) -> () {
    if let Err(e) = run_deposit_listener(db, contract) {
        println!("Deposit_driver error: {}", e);
    }
    if let Err(e) = run_withdraw_listener(db, contract) {
        println!("Withdraw_driver error: {}", e);
    }
    //...
}

