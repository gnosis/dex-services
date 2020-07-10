use super::AccountState;
use crate::util::CeiledDiv;
use ethcontract::{Address, U256};
use pricegraph::{Element, Price, TokenPair, Validity};

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct Order {
    pub id: u16,
    pub account_id: Address,
    pub buy_token: u16,
    pub sell_token: u16,
    // buy amount
    pub numerator: u128,
    // sell amount
    pub denominator: u128,
    pub remaining_sell_amount: u128,
    pub valid_from: u32,
    pub valid_until: u32,
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
            numerator: 1_000_000_000_000_000_000,
            denominator: 1_000_000_000_000_000_000,
            remaining_sell_amount: 1_000_000_000_000_000_000,
            valid_from: 0,
            valid_until: 0,
        }
    }

    pub fn compute_remaining_buy_sell_amounts(&self) -> (u128, u128) {
        compute_remaining_buy_sell_amounts(
            self.numerator,
            self.denominator,
            self.remaining_sell_amount,
        )
    }

    pub fn to_element(&self, balance: U256) -> Element {
        Element {
            user: self.account_id,
            balance,
            pair: TokenPair {
                buy: self.buy_token,
                sell: self.sell_token,
            },
            valid: Validity {
                from: self.valid_from,
                to: self.valid_until,
            },
            price: Price {
                numerator: self.numerator,
                denominator: self.denominator,
            },
            remaining_sell_amount: self.remaining_sell_amount,
            id: self.id,
        }
    }

    pub fn to_element_with_accounts(&self, accounts: &AccountState) -> Element {
        self.to_element(accounts.read_balance(self.sell_token, self.account_id))
    }
}

fn compute_remaining_buy_sell_amounts(
    numerator: u128,
    denominator: u128,
    remaining: u128,
) -> (u128, u128) {
    assert!(
        remaining <= denominator,
        "Smart contract should never allow this inequality"
    );
    // 0 on sellAmount (remaining <= denominator) is nonsense, but solver can handle it.
    // 0 on buyAmount (numerator) is a Market Sell Order.
    let buy_amount = if denominator > 0 {
        // up-casting to handle overflow
        let top = U256::from(remaining) * U256::from(numerator);
        (top).ceiled_div(U256::from(denominator)).as_u128()
    } else {
        0
    };
    (buy_amount, remaining)
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
            numerator: 5,
            denominator: 4,
            remaining_sell_amount: 4,
            valid_from: 0,
            valid_until: 0,
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

    #[test]
    fn computation_of_buy_sell_amounts() {
        let numerator = 19;
        let denominator = 14;
        let remaining = 5;
        let result = compute_remaining_buy_sell_amounts(numerator, denominator, remaining);
        assert_eq!(result, ((5 * 19 + 13) / 14, 5));
    }

    // tests for compute_buy_sell_amounts
    #[test]
    fn compute_buy_sell_tiny_numbers() {
        let numerator = 1u128;
        let denominator = 3u128;
        let remaining = 2u128;
        // Note that contract does not allow remaining > denominator!
        let (buy, sell) = compute_remaining_buy_sell_amounts(numerator, denominator, remaining);
        // Sell at most 2 for at least 1 (i.e. as long as the price at least 1:3)
        assert_eq!((buy, sell), (1, remaining));
    }

    #[test]
    fn compute_buy_sell_recoverable_overflow() {
        let numerator = 2u128;
        let denominator = u128::max_value();
        let remaining = u128::max_value();

        // Note that contract does not allow remaining > denominator!
        let (buy, sell) = compute_remaining_buy_sell_amounts(numerator, denominator, remaining);
        // Sell at most 3 for at least 2 (i.e. as long as the price at least 1:2)
        assert_eq!((buy, sell), (2, remaining));
    }

    #[test]
    fn generic_compute_buy_sell() {
        let (numerator, denominator) = (1_000u128, 1_500u128);
        let remaining = 1_486u128;
        let (buy, sell) = compute_remaining_buy_sell_amounts(numerator, denominator, remaining);
        assert_eq!((buy, sell), (991, remaining));
    }

    #[test]
    fn generic_compute_buy_sell_2() {
        let (numerator, denominator) = (1_000u128, 1_500u128);
        let remaining = 1_485u128;
        let (buy, sell) = compute_remaining_buy_sell_amounts(numerator, denominator, remaining);
        assert_eq!((buy, sell), (990, remaining));
    }
}
