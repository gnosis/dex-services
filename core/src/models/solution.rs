use crate::bigint_u256;
use ethcontract::{Address, U256};
use num::{BigInt, Zero as _};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Default, PartialEq, Deserialize)]
pub struct ObjVals {
    pub volume: String,
    pub utility: String,
    pub utility_disreg: String,
    pub utility_disreg_touched: String,
    pub fees: u128,
    pub orders_touched: u128,
}

#[derive(Debug, Default, PartialEq, Deserialize)]
pub struct Solver {
    pub runtime: f64,
    pub runtime_preprocessing: f64,
    pub runtime_solving: f64,
    pub runtime_ring_finding: f64,
    pub runtime_validation: f64,
    pub nr_variables: u128,
    pub nr_bool_variables: u128,
    pub optimality_gap: f64,
    pub obj_val: f64,
    pub obj_val_sc: f64,
}

#[derive(Debug, PartialEq)]
pub struct SolverStats {
    pub obj_vals: ObjVals,
    pub solver: Solver,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutedOrder {
    pub account_id: Address,
    pub order_id: u16,
    pub sell_amount: u128,
    pub buy_amount: u128,
}

#[derive(Clone, Debug, Default, PartialEq)]
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

impl Solution {
    pub fn trivial() -> Self {
        Self::default()
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
