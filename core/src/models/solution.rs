use ethcontract::Address;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutedOrder {
    pub account_id: Address,
    pub order_id: u16,
    pub sell_amount: u128,
    pub buy_amount: u128,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    /// token_id => price
    pub prices: HashMap<u16, u128>,
    pub executed_orders: Vec<ExecutedOrder>,
}

impl Solution {
    pub fn trivial() -> Self {
        Solution {
            prices: HashMap::new(),
            executed_orders: Vec::new(),
        }
    }

    /// Returns true if a solution is non-trivial and false otherwise
    pub fn is_non_trivial(&self) -> bool {
        self.executed_orders
            .iter()
            .any(|order| order.sell_amount > 0)
    }
}

#[cfg(test)]
pub mod test_util {
    use super::*;

    impl Solution {
        /// Returns the price for a token by ID or 0 if the token was not found.
        pub fn price(&self, token_id: u16) -> Option<u128> {
            self.prices.get(&token_id).copied()
        }

        /// Returns the maximum token id included in the solution's non-zero prices.
        pub fn max_token(&self) -> Option<u16> {
            self.prices.keys().max().copied()
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::util::test_util::map_from_slice;

    fn generic_non_trivial_solution() -> Solution {
        Solution {
            prices: map_from_slice(&[(0, 42), (2, 42)]),
            executed_orders: vec![
                ExecutedOrder {
                    account_id: Address::zero(),
                    order_id: 0,
                    sell_amount: 1,
                    buy_amount: 4,
                },
                ExecutedOrder {
                    account_id: Address::zero(),
                    order_id: 1,
                    sell_amount: 2,
                    buy_amount: 5,
                },
                ExecutedOrder {
                    account_id: Address::zero(),
                    order_id: 2,
                    sell_amount: 3,
                    buy_amount: 6,
                },
            ],
        }
    }

    #[test]
    fn test_is_non_trivial() {
        assert!(generic_non_trivial_solution().is_non_trivial());
        assert!(!Solution::trivial().is_non_trivial());
    }

    #[test]
    fn test_max_token() {
        assert_eq!(generic_non_trivial_solution().max_token().unwrap(), 2);
        assert_eq!(Solution::trivial().max_token(), None);
    }
}
