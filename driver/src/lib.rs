extern crate dfusion_core;
extern crate hex;
#[macro_use]
extern crate log;
extern crate rustc_hex;
extern crate serde_json;
extern crate web3;

use dfusion_core::database::GraphReader;

use crate::contract::SnappContractImpl;
use crate::deposit_driver::run_deposit_listener;
use crate::order_driver::OrderProcessor;
use crate::withdraw_driver::run_withdraw_listener;

pub mod contract;
pub mod error;
pub mod order_driver;
pub mod price_finding;

mod deposit_driver;
mod util;
mod withdraw_driver;

pub fn run_driver_components(
    db: &GraphReader,
    contract: &SnappContractImpl,
    order_processor: &mut OrderProcessor<GraphReader, SnappContractImpl>,
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
}
