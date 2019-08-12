extern crate hex;
extern crate mongodb;
#[macro_use]
extern crate log;
extern crate rustc_hex;
extern crate serde_json;
extern crate web3;
extern crate dfusion_core;

use crate::contract::SnappContractImpl;
use crate::mongo_db::MongoDB;
use crate::deposit_driver::run_deposit_listener;
use crate::order_driver::run_order_listener;
use crate::price_finding::PriceFinding;
use crate::withdraw_driver::run_withdraw_listener;

pub mod contract;
pub mod mongo_db;
pub mod error;
pub mod price_finding;

mod deposit_driver;
mod order_driver;
mod withdraw_driver;
mod util;

pub fn run_driver_components(
    db: &MongoDB,
    contract: &SnappContractImpl,
    price_finder: &mut PriceFinding,
) {
    if let Err(e) = run_deposit_listener(db, contract) {
        error!("Deposit_driver error: {}", e);
    }
    if let Err(e) = run_withdraw_listener(db, contract) {
        error!("Withdraw_driver error: {}", e);
    }
    if let Err(e) = run_order_listener(db, contract, price_finder) {
        error!("Order_driver error: {}", e);
    }
    //...
}

