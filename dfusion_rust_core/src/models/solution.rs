use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    /// token_id => price
    pub prices: HashMap<u16, u128>,
    pub executed_buy_amounts: Vec<u128>,
    pub executed_sell_amounts: Vec<u128>,
}

impl Solution {
    pub fn trivial(num_orders: usize) -> Self {
        Solution {
            prices: HashMap::new(),
            executed_buy_amounts: vec![0; num_orders],
            executed_sell_amounts: vec![0; num_orders],
        }
    }

    /// Returns the price for a token by ID or 0 if the token was not found.
    pub fn price(&self, token_id: u16) -> Option<u128> {
        self.prices.get(&token_id).copied()
    }

    /// Returns the maximum token id included in the solution's non-zero prices.
    pub fn max_token(&self) -> Option<u16> {
        self.prices.keys().max().copied()
    }

    /// Returns true if a solution is non-trivial and false otherwise
    pub fn is_non_trivial(&self) -> bool {
        self.executed_sell_amounts.iter().any(|&amt| amt > 0)
    }
}

#[cfg(test)]
pub mod unit_test {
    use super::*;
    use crate::models::util::map_from_slice;

    fn generic_non_trivial_solution() -> Solution {
        Solution {
            prices: map_from_slice(&[(0, 42), (2, 42)]),
            executed_buy_amounts: vec![4, 5, 6],
            executed_sell_amounts: vec![1, 2, 3],
        }
    }

    #[test]
    fn test_is_non_trivial() {
        assert!(generic_non_trivial_solution().is_non_trivial());
        assert!(!Solution::trivial(1).is_non_trivial());
    }

    #[test]
    fn test_max_token() {
        assert_eq!(generic_non_trivial_solution().max_token().unwrap(), 2);
        assert_eq!(Solution::trivial(1).max_token(), None);
    }
}
