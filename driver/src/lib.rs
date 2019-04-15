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
pub mod error;
pub mod price_finding;

mod deposit_driver;
mod order_driver;
pub mod models;
mod withdraw_driver;
mod util;

use crate::deposit_driver::run_deposit_listener;
use crate::withdraw_driver::run_withdraw_listener;
use crate::order_driver::run_order_listener;
use crate::db_interface::MongoDB;
use crate::contract::SnappContractImpl;
use crate::price_finding::linear_optimization_price_finder::LinearOptimisationPriceFinder;

pub fn run_driver_components(
    db: &MongoDB,
    contract: &SnappContractImpl, 
    price_finder: &mut LinearOptimisationPriceFinder,
) -> () {
    if let Err(e) = run_deposit_listener(db, contract) {
        println!("Deposit_driver error: {}", e);
    }
    if let Err(e) = run_withdraw_listener(db, contract) {
        println!("Withdraw_driver error: {}", e);
    }
    if let Err(e) = run_order_listener(db, contract, price_finder) {
         println!("Order_driver error: {}", e);
    }
    //...
}

