use serde::Deserialize;
use web3::types::{H160, U256};

// TODO: refactor the struct to be an enum that can be either the stableX
// version (slot index) or the Snapp version (slot_index + slot).
#[derive(Debug, Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct BatchInformation {
    // only applies to the Snapp version
    pub slot: U256,
    /// the order index of a StableX order (the n-th order a user has ever
    /// placed in the system)
    pub slot_index: u16,
}

#[derive(Debug, Clone, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub batch_information: Option<BatchInformation>,
    pub account_id: H160,
    pub buy_token: u16,
    pub sell_token: u16,
    pub buy_amount: u128,
    pub sell_amount: u128,
}

pub mod test_util {
    use super::*;
    pub fn create_order_for_test() -> Order {
        Order {
            batch_information: Some(BatchInformation {
                slot: U256::zero(),
                slot_index: 0,
            }),
            account_id: H160::from_low_u64_be(1),
            buy_token: 3,
            sell_token: 2,
            buy_amount: 5,
            sell_amount: 4,
        }
    }
}
