extern crate hex;
extern crate mongodb;
#[macro_use]
extern crate log;
extern crate dfusion_core;
extern crate rustc_hex;
extern crate serde_json;
extern crate web3;

use crate::contract::SnappContractImpl;
use crate::deposit_driver::run_deposit_listener;
use crate::mongo_db::MongoDB;
use crate::order_driver::OrderProcessor;
use crate::withdraw_driver::run_withdraw_listener;

pub mod contract;
pub mod error;
pub mod mongo_db;
pub mod order_driver;
pub mod price_finding;

mod deposit_driver;
mod util;
mod withdraw_driver;

pub fn run_driver_components(
    db: &MongoDB,
    contract: &SnappContractImpl,
    order_processor: &mut OrderProcessor<MongoDB, SnappContractImpl>,
) {
    if let Err(e) = run_deposit_listener(db, contract) {
        error!("Deposit_driver error: {}", e);
    }
    if let Err(e) = run_withdraw_listener(db, contract) {
        error!("Withdraw_driver error: {}", e);
    }
    if let Err(e) = order_processor.run() {
        error!("Order_driver error: {}", e);
    }
    //...
}
