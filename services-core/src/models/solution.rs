use crate::bigint_u256;
use ethcontract::{Address, U256};
use num::{BigInt, Zero as _};
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

#[derive(Debug, Copy, Clone)]
pub struct EconomicViabilityInfo {
    pub num_executed_orders: usize,
    pub earned_fee: U256,
}

/// The approximate amount of gas used to per trade in a solution or solution reversion.
pub const GAS_PER_TRADE: u32 = 100_000;

pub fn gas_use(
    number_of_trades_in_solution: usize,
    include_reversion_of_previous_solution_with_same_number_of_trades: bool,
) -> U256 {
    let mut result = U256::from(number_of_trades_in_solution) * U256::from(GAS_PER_TRADE);
    if include_reversion_of_previous_solution_with_same_number_of_trades {
        result *= 2;
    }
    result
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

    pub fn economic_viability_info(&self) -> EconomicViabilityInfo {
        EconomicViabilityInfo {
            num_executed_orders: self.executed_orders.len(),
            earned_fee: self.earned_fee(),
        }
    }

    pub fn earned_fee(&self) -> U256 {
        // We expect that only the fee token has an imbalance so by calculating the total imbalance
        // this must be equal to the fee token imbalance. This allows us to calculate the burnt fees
        // without accessing the orderbook solely based on the solution.
        let mut token_imbalance = BigInt::zero();
        for executed_order in self.executed_orders.iter() {
            token_imbalance += executed_order.sell_amount;
            token_imbalance -= executed_order.buy_amount;
        }
        token_imbalance /= 2;
        bigint_u256::bigint_to_u256_saturating(&token_imbalance)
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

    #[test]
    fn earned_fee_is_half_total_token_imbalance() {
        let solution = Solution {
            prices: HashMap::new(),
            executed_orders: vec![
                ExecutedOrder {
                    account_id: Address::zero(),
                    order_id: 0,
                    sell_amount: 2,
                    buy_amount: 1,
                },
                ExecutedOrder {
                    account_id: Address::zero(),
                    order_id: 1,
                    sell_amount: 4,
                    buy_amount: 3,
                },
                ExecutedOrder {
                    account_id: Address::zero(),
                    order_id: 2,
                    sell_amount: 6,
                    buy_amount: 4,
                },
            ],
        };
        assert_eq!(solution.earned_fee(), U256::from(2));
    }
}
