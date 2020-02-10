use serde::Deserialize;
use web3::types::H160;

#[derive(Debug, Clone, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub id: u16,
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
            id: 0,
            account_id: H160::from_low_u64_be(1),
            buy_token: 3,
            sell_token: 2,
            buy_amount: 5,
            sell_amount: 4,
        }
    }
}
