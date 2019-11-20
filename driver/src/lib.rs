// NOTE: The order in which these two crates get linked seems to matter (usure
//   why). And when we remove `extern crate` statements and let cargo decide the
//   order it leads to a linking error. So for now, until we figure out exactly
//   why this is happening lets keep this these two `extern crate` statements so
//   we successfully link.
extern crate ethereum_tx_sign;
extern crate web3;

use dfusion_core::database::DbInterface;

use log::error;

use crate::contracts::snapp_contract::SnappContract;
use crate::driver::deposit_driver::run_deposit_listener;
use crate::driver::order_driver::OrderProcessor;
use crate::driver::withdraw_driver::run_withdraw_listener;

pub mod contracts;
pub mod driver;
pub mod error;
pub mod price_finding;
pub mod snapp;
pub mod util;

pub fn run_driver_components(
    db: &dyn DbInterface,
    contract: &dyn SnappContract,
    order_processor: &mut OrderProcessor,
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
