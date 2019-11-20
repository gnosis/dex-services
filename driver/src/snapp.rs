//! This module implements snapp specific extensions to models. Specifically
//! Snapp objective value calculation as the driver is expected to provide this
//! value on solution submission.

use crate::util::u128_to_u256;
use dfusion_core::models::{Order, Solution};
use thiserror::Error;
use web3::types::U256;

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
        let buy_price = u128_to_u256(buy_price);
        let exec_buy_amt = u128_to_u256(exec_buy_amt);
        let exec_sell_amt = u128_to_u256(exec_sell_amt);
        let buy_amt = u128_to_u256(self.buy_amount);
        let sell_amt = u128_to_u256(self.sell_amount);

        // the utility is caculated in two parts, this is done to avoid overflows
        let utility_with_error = exec_buy_amt
            .checked_sub(
                (exec_sell_amt * buy_amt)
                    .checked_div(sell_amt)
                    .ok_or(SnappObjectiveError::BadOrder)?,
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
    /// The order contains invalid values such as a 0 sell amount.
    #[error("an order contains invalid values")]
    BadOrder,
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
    fn order_executed_utility_err_bad_order() {
        let order = Order {
            buy_amount: 10u128.pow(18),
            sell_amount: 0,
            ..Default::default()
        };
        let buy_price = 10u128.pow(18);
        let exec_sell_amt = 10u128.pow(18);

        assert_eq!(
            order.executed_utility(buy_price, order.buy_amount, exec_sell_amt),
            Err(SnappObjectiveError::BadOrder)
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
