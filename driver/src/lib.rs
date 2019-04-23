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
use crate::order_driver::run_order_listener;
use crate::price_finding::naive_solver::NaiveSolver;
use crate::withdraw_driver::run_withdraw_listener;

pub mod contract;
pub mod db_interface;
pub mod error;
pub mod price_finding;

mod deposit_driver;
mod order_driver;
pub mod models;
mod withdraw_driver;
mod util;

pub fn run_driver_components(
    db: &MongoDB,
    contract: &SnappContractImpl, 
    price_finder: &mut NaiveSolver,
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

