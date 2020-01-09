//! This module implements snapp specific extensions to models. Specifically
//! Snapp objective value calculation as the driver is expected to provide this
//! value on solution submission.

use dfusion_core::models::{Order, Solution};
use thiserror::Error;
use web3::types::U256;

/// Snapp specific extension trait for `Solution`. This extension trait provides
/// an implementation to calculating the Snapp objective value which must be
/// submitted with the solution.
pub trait SnappSolution {
    /// Returns the price for a token by ID or None if the token was not found.
    fn get_token_price(&self, token_id: u16) -> Option<u128>;

    /// Returns the objective value for submitting a solution to the Snapp smart
    /// contract. The objective value is calculated as the total executed
    /// utility of all orders.
    ///
    /// Note that this does not evaluate the validity of the solution and just
    /// calculates the objective value.
    ///
    /// # Returns
    ///
    /// `Ok(U256)` when the solution's objective value is correctly calculated
    /// and `Err` otherwise. Errors can occur when:
    /// - The order book does not match the solution (number of orders)
    /// - The order's token ids are not found
    /// - An invalid order
    /// - An order where the limit price was not respected
    /// - Total utility overflows a solidity `uint256`
    fn snapp_objective_value(&self, orders: &[Order]) -> Result<U256, SnappObjectiveError>;
}

impl SnappSolution for Solution {
    fn get_token_price(&self, token_id: u16) -> Option<u128> {
        self.prices.get(&token_id).cloned()
    }

    fn snapp_objective_value(&self, orders: &[Order]) -> Result<U256, SnappObjectiveError> {
        let mut total_executed_utility = U256::zero();
        for (i, order) in orders.iter().enumerate() {
            total_executed_utility = total_executed_utility
                .checked_add(
                    order.executed_utility(
                        self.get_token_price(order.buy_token)
                            .ok_or(SnappObjectiveError::TokenNotFound)?,
                        *self
                            .executed_buy_amounts
                            .get(i)
                            .ok_or(SnappObjectiveError::OrderBookMismatch)?,
                        *self
                            .executed_sell_amounts
                            .get(i)
                            .ok_or(SnappObjectiveError::OrderBookMismatch)?,
                    )?,
                )
                .ok_or(SnappObjectiveError::TotalUtilityOverflow)?;
        }

        Ok(total_executed_utility)
    }
}

/// Snapp specific extensions for `Order`. This extension trait provides the
/// required functions for calculating the objective value used in the Snapp
/// protocol which is needed for solution submission.
pub trait SnappOrder {
    /// Returns the executed utility of an order based on executed amounts
    /// and closing prices.
    ///
    /// Note that utility is calculated with the following equasion:
    /// `((exec_buy_amt * sell_amt - exec_sell_amt * buy_amt) * buy_price) /
    /// sell_amt`
    ///
    /// # Returns
    ///
    /// `Ok(U256)` when the order executed utility is correctly calculated
    /// and `Err` otherwise.
    fn executed_utility(
        &self,
        buy_price: u128,
        exec_buy_amt: u128,
        exec_sell_amt: u128,
    ) -> Result<U256, SnappObjectiveError>;
}

impl SnappOrder for Order {
    fn executed_utility(
        &self,
        buy_price: u128,
        exec_buy_amt: u128,
        exec_sell_amt: u128,
    ) -> Result<U256, SnappObjectiveError> {
        let buy_price = U256::from(buy_price);
        let exec_buy_amt = U256::from(exec_buy_amt);
        let exec_sell_amt = U256::from(exec_sell_amt);
        let buy_amt = U256::from(self.buy_amount);
        let sell_amt = U256::from(self.sell_amount);

        // the utility is caculated in two parts, this is done to avoid overflows
        let utility_with_error = exec_buy_amt
            .checked_sub(
                (exec_sell_amt * buy_amt)
                    .checked_div(sell_amt)
                    .ok_or(SnappObjectiveError::InvalidOrder)?,
            )
            .ok_or(SnappObjectiveError::LimitPriceNotRespected)?
            * buy_price;
        let utility_error = (((exec_sell_amt * buy_amt) % sell_amt) * buy_price) / sell_amt;

        let utility = utility_with_error
            .checked_sub(utility_error)
            .ok_or(SnappObjectiveError::LimitPriceNotRespected)?;

        Ok(utility)
    }
}

/// Represents and error that can occur during the calculation of the Snapp
/// objective value.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SnappObjectiveError {
    /// A token referenced by an order was not found in the solution.
    #[error("token not found")]
    TokenNotFound,
    /// The order book does not match the solution. This happens when the number
    /// of orders does not equal the number of executed amounts.
    #[error("the order book does not match the solution")]
    OrderBookMismatch,
    /// The order contains invalid values such as a 0 sell amount.
    #[error("an order contains invalid values")]
    InvalidOrder,
    /// The limit price of an order was not respected. This causes an underflow
    /// during the utility calculation and as such the total objective value
    /// cannot be accurately determined.
    #[error("the limit price of an order was not respected")]
    LimitPriceNotRespected,
    /// An overflow happened when calculating the total utility of a solution.
    /// The total utility must fit in a solidity `uint256`.
    #[error("total utility overflows a uint256")]
    TotalUtilityOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use dfusion_core::models::util::map_from_list;

    #[test]
    fn solution_objective_value() {
        let orders = vec![
            Order {
                buy_token: 0,
                sell_token: 1,
                buy_amount: 2 * 10u128.pow(18),
                sell_amount: 10u128.pow(18),
                ..Order::default()
            },
            Order {
                buy_token: 1,
                sell_token: 0,
                buy_amount: 10u128.pow(18),
                sell_amount: 3 * 10u128.pow(18),
                ..Order::default()
            },
        ];

        let solution = Solution {
            prices: map_from_list(&[(0, 10u128.pow(18)), (1, 2_500_000_000_000_000_000)]),
            executed_buy_amounts: vec![2_497_500_000_000_000_000, 10u128.pow(18)],
            executed_sell_amounts: vec![10u128.pow(18), 2_502_502_502_502_502_502],
        };

        // U0 = ((xb * os - xs * ob) * pb) / os
        //    = ((2.497eth * 1eth - 1eth * 2eth) * 1eth) / 1eth
        //    = 497500000000000000000000000000000000
        // U1 = ((xb * os - xs * ob) * pb) / os
        //    = ((1eth * 3eth - 2.502eth * 1eth) * 2.5eth) / 3eth
        //    = 414581247914581248333333333333333333
        //    = 414581247914581248333333333333333334 -- with rounding error
        //  O = U0 + U1
        //    = 912081247914581248333333333333333334

        assert_eq!(
            solution.snapp_objective_value(&orders),
            Ok(U256::from_dec_str("912081247914581248333333333333333334").unwrap())
        );
    }

    #[test]
    fn trivial_solution_objective_value() {
        let orders: Vec<_> = (0..5)
            .map(|i| Order {
                buy_token: i,
                sell_token: 5 - i,
                buy_amount: 100,
                sell_amount: 100,
                ..Default::default()
            })
            .collect();
        assert_eq!(
            Solution::trivial(0).snapp_objective_value(&[]),
            Ok(U256::zero())
        );
        assert_eq!(
            Solution::trivial(1).snapp_objective_value(&orders[..1]),
            Result::Err(SnappObjectiveError::TokenNotFound)
        );
        assert_eq!(
            Solution::trivial(5).snapp_objective_value(&orders),
            Result::Err(SnappObjectiveError::TokenNotFound)
        );
    }

    #[test]
    fn solution_objective_value_token_not_found() {
        let orders = vec![Order {
            buy_token: 1,
            sell_token: 0,
            buy_amount: 10u128.pow(18),
            sell_amount: 10u128.pow(18),
            ..Order::default()
        }];

        let solution = Solution {
            prices: map_from_list(&[(0, 10u128.pow(18))]),
            executed_buy_amounts: vec![10u128.pow(18)],
            executed_sell_amounts: vec![10u128.pow(18)],
        };

        assert_eq!(
            solution.snapp_objective_value(&orders),
            Err(SnappObjectiveError::TokenNotFound)
        );
    }

    #[test]
    fn solution_objective_value_order_book_mismatch() {
        let orders = vec![
            Order {
                buy_token: 0,
                sell_token: 1,
                buy_amount: 10u128.pow(18),
                sell_amount: 10u128.pow(18),
                ..Order::default()
            },
            Order {
                buy_token: 1,
                sell_token: 0,
                buy_amount: 10u128.pow(18),
                sell_amount: 10u128.pow(18),
                ..Order::default()
            },
        ];

        let solution = Solution {
            prices: map_from_list(&[(0, 10u128.pow(18)), (1, 10u128.pow(18))]),
            executed_buy_amounts: vec![10u128.pow(18)],
            executed_sell_amounts: vec![10u128.pow(18)],
        };

        assert_eq!(
            solution.snapp_objective_value(&orders),
            Err(SnappObjectiveError::OrderBookMismatch)
        );
    }

    #[test]
    fn solution_objective_value_order_utility_error() {
        let orders = vec![Order {
            buy_token: 0,
            sell_token: 1,
            buy_amount: 10u128.pow(18),
            sell_amount: 0,
            ..Order::default()
        }];

        let solution = Solution {
            prices: map_from_list(&[(0, 10u128.pow(18)), (1, 10u128.pow(18))]),
            executed_buy_amounts: vec![10u128.pow(18)],
            executed_sell_amounts: vec![10u128.pow(18)],
        };

        assert_eq!(
            solution.snapp_objective_value(&orders),
            Err(SnappObjectiveError::InvalidOrder)
        );
    }

    #[test]
    fn solution_objective_value_overflow() {
        let orders = vec![
            Order {
                buy_token: 0,
                sell_token: 1,
                buy_amount: 10u128.pow(18),
                sell_amount: 10u128.pow(18),
                ..Order::default()
            },
            Order {
                buy_token: 0,
                sell_token: 1,
                buy_amount: 10u128.pow(18),
                sell_amount: 10u128.pow(18),
                ..Order::default()
            },
        ];

        let solution = Solution {
            prices: map_from_list(&[(0, u128::max_value()), (1, 10u128.pow(18))]),
            executed_buy_amounts: vec![u128::max_value(), u128::max_value()],
            executed_sell_amounts: vec![0, 0],
        };

        assert_eq!(
            solution.snapp_objective_value(&orders),
            Err(SnappObjectiveError::TotalUtilityOverflow)
        );
    }

    #[test]
    fn small_order_executed_utility() {
        let order = Order {
            buy_amount: 10,
            sell_amount: 100,
            ..Default::default()
        };
        let buy_price = 9;
        let exec_buy_amt = 10;
        let exec_sell_amt = 90;

        assert_eq!(
            order.executed_utility(buy_price, exec_buy_amt, exec_sell_amt),
            // u = ((xb * os - xs * ob) * bp) / os
            //   = ((10 * 100 - 90 * 10) * 9) / 100
            //   = 9
            Ok(9.into())
        );
    }

    #[test]
    fn large_order_executed_utility() {
        let order = Order {
            buy_amount: 10u128.pow(18),
            sell_amount: 2 * 10u128.pow(18),
            ..Default::default()
        };
        let buy_price = 2 * 10u128.pow(18);
        let exec_buy_amt = 2 * 10u128.pow(18);
        let exec_sell_amt = 10u128.pow(18);

        assert_eq!(
            order.executed_utility(buy_price, exec_buy_amt, exec_sell_amt),
            // u = ((2e18 * 2e18 - 1e18 * 1e18) * 2e18) / 2e18
            //   = 3e36
            Ok(U256::from(3) * U256::from(10).pow(36.into()))
        );
    }

    #[test]
    fn exact_order_executed_utility() {
        let order = Order {
            buy_amount: 10u128.pow(18),
            sell_amount: 2 * 10u128.pow(18),
            ..Default::default()
        };
        let buy_price = 10u128.pow(18);

        assert_eq!(
            order.executed_utility(buy_price, order.buy_amount, order.sell_amount),
            Ok(U256::zero())
        );
    }

    #[test]
    fn order_executed_utility_err_invalid_order() {
        let order = Order {
            buy_amount: 10u128.pow(18),
            sell_amount: 0,
            ..Default::default()
        };
        let buy_price = 10u128.pow(18);
        let exec_sell_amt = 10u128.pow(18);

        assert_eq!(
            order.executed_utility(buy_price, order.buy_amount, exec_sell_amt),
            Err(SnappObjectiveError::InvalidOrder)
        );
    }

    #[test]
    fn order_executed_utility_err_non_respected_limit_price() {
        let order = Order {
            buy_amount: 10u128.pow(18),
            sell_amount: 2 * 10u128.pow(18),
            ..Default::default()
        };
        let buy_price = 10u128.pow(18);
        let exec_sell_amt = order.sell_amount * 2;

        assert_eq!(
            order.executed_utility(buy_price, order.buy_amount, exec_sell_amt),
            Err(SnappObjectiveError::LimitPriceNotRespected)
        );
    }
}
