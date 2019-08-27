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
use crate::order_driver::run_order_listener;
use crate::price_finding::PriceFinding;
use crate::withdraw_driver::run_withdraw_listener;

pub mod contract;
pub mod error;
pub mod price_finding;

mod deposit_driver;
mod order_driver;
mod util;
mod withdraw_driver;

pub fn run_driver_components(
    db: &GraphReader,
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
}
