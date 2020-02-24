use ethcontract::Address;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub id: u16,
    pub account_id: Address,
    pub buy_token: u16,
    pub sell_token: u16,
    pub buy_amount: u128,
    pub sell_amount: u128,
}

impl Order {
    /// Creates a fake order in between a token pair for unit testing.
    #[cfg(test)]
    pub fn for_token_pair(buy_token: u16, sell_token: u16) -> Self {
        Order {
            id: 0,
            account_id: Address::repeat_byte(0x42),
            buy_token,
            sell_token,
            buy_amount: 1_000_000_000_000_000_000,
            sell_amount: 1_000_000_000_000_000_000,
        }
    }
}

#[cfg(test)]
pub mod test_util {
    use super::*;
    use crate::models::ExecutedOrder;

    pub fn create_order_for_test() -> Order {
        Order {
            id: 0,
            account_id: Address::from_low_u64_be(1),
            buy_token: 3,
            sell_token: 2,
            buy_amount: 5,
            sell_amount: 4,
        }
    }

    pub fn order_to_executed_order(
        order: &Order,
        sell_amount: u128,
        buy_amount: u128,
    ) -> ExecutedOrder {
        ExecutedOrder {
            account_id: order.account_id,
            order_id: order.id,
            sell_amount,
            buy_amount,
        }
    }
}
