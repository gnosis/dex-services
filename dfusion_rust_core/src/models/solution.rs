use crate::models::*;
use crate::util::u128_to_u256;

use log::info;

use web3::types::U256;

use std::iter::once;

#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    pub prices: Vec<u128>,
    pub executed_buy_amounts: Vec<u128>,
    pub executed_sell_amounts: Vec<u128>,
}

impl Solution {
    pub fn trivial(num_orders: usize) -> Self {
        Solution {
            prices: vec![0; TOKENS as usize],
            executed_buy_amounts: vec![0; num_orders],
            executed_sell_amounts: vec![0; num_orders],
        }
    }

    /// Returns the token price for token or `None` if the token is invalid.
    fn price(&self, token: u16) -> Option<u128> {
        self.prices.get(token as usize).cloned()
    }

    /// Compute the result of the objective function for the solution. This is
    /// currently defined as *U - dUT* where *U* is the total utility of the
    /// trade and *dUT* is the total disreguarded utility of touched orders.
    /// Note that this does not check the validity of the solution.
    ///
    /// # Returns
    ///
    /// Returns the objective value for the solution or None if there the orders
    /// do not match the solution or there was an error in the calculation such
    /// as overflow or divide by 0.
    pub fn objective_value(&self, orders: &[Order]) -> Option<U256> {
        if orders.len() != self.executed_buy_amounts.len()
            || orders.len() != self.executed_sell_amounts.len()
        {
            return None;
        }

        // note that we don't actually have to do the same checking here (such as
        // account balances and such) as we do in the smart contract - we only
        // care if the solution is valid given the provided orders

        let mut total_utility = U256::zero();
        let mut total_disregarded_utility = U256::zero();

        let mut fee_buy_amount = U256::zero();
        let mut fee_sell_amount = U256::zero();

        for (i, order) in orders.iter().enumerate() {
            let buy_price = self.price(order.buy_token)?;
            let sell_price = self.price(order.sell_token)?;
            let exec_buy_amount = self.executed_buy_amounts[i];
            let exec_sell_amount = self.executed_sell_amounts[i];

            if exec_buy_amount == 0 && exec_sell_amount == 0 {
                // this order was not touched, so skip
                continue;
            }

            total_utility = total_utility.checked_add(order.utility(
                buy_price,
                exec_buy_amount,
                exec_sell_amount,
            )?)?;
            total_disregarded_utility = total_disregarded_utility.checked_add(
                order.disregarded_utility(buy_price, sell_price, exec_sell_amount)?,
            )?;

            if order.buy_token == 0 {
                fee_buy_amount = fee_buy_amount.checked_add(u128_to_u256(exec_buy_amount))?;
            }
            if order.sell_token == 0 {
                fee_sell_amount = fee_buy_amount.checked_add(u128_to_u256(exec_buy_amount))?;
            }
        }

        total_utility
            .checked_sub(total_disregarded_utility)?
            .checked_add(fee_sell_amount.checked_sub(fee_buy_amount)? / 2)
    }
}

impl Serializable for Solution {
    fn bytes(&self) -> Vec<u8> {
        let alternating_buy_sell_amounts: Vec<u128> = self
            .executed_buy_amounts
            .iter()
            .zip(self.executed_sell_amounts.iter())
            .flat_map(|tup| once(tup.0).chain(once(tup.1)))
            .cloned()
            .collect();
        [&self.prices, &alternating_buy_sell_amounts]
            .iter()
            .flat_map(|list| list.iter())
            .flat_map(Serializable::bytes)
            .collect()
    }
}

impl Deserializable for Solution {
    fn from_bytes(mut bytes: Vec<u8>) -> Self {
        let volumes = bytes.split_off(TOKENS as usize * 12);
        let prices = bytes
            .chunks_exact(12)
            .map(|chunk| util::read_amount(&util::get_amount_from_slice(chunk)))
            .collect();
        info!("Recovered prices as: {:?}", prices);

        let mut executed_buy_amounts: Vec<u128> = vec![];
        let mut executed_sell_amounts: Vec<u128> = vec![];
        volumes.chunks_exact(2 * 12).for_each(|chunk| {
            executed_buy_amounts.push(util::read_amount(&util::get_amount_from_slice(
                &chunk[0..12],
            )));
            executed_sell_amounts.push(util::read_amount(&util::get_amount_from_slice(
                &chunk[12..24],
            )));
        });
        Solution {
            prices,
            executed_buy_amounts,
            executed_sell_amounts,
        }
    }
}

#[cfg(test)]
pub mod unit_test {
    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        let solution = Solution {
            prices: vec![42; TOKENS as usize],
            executed_buy_amounts: vec![4, 5, 6],
            executed_sell_amounts: vec![1, 2, 3],
        };

        let bytes = solution.bytes();
        let parsed_solution = Solution::from_bytes(bytes);

        assert_eq!(solution, parsed_solution);
    }

    #[test]
    fn test_deserialize_e2e_example() {
        let bytes = vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 0, 0,
            0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 13, 224, 182,
            179, 167, 100, 0, 0, 0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 0, 0, 0, 0, 0, 13,
            224, 182, 179, 167, 100, 0, 0, 0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0,
        ];
        let parsed_solution = Solution::from_bytes(bytes);
        let expected = Solution {
            prices: vec![
                1,
                10u128.pow(18),
                10u128.pow(18),
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
                1,
            ],
            executed_buy_amounts: vec![10u128.pow(18), 10u128.pow(18)],
            executed_sell_amounts: vec![10u128.pow(18), 10u128.pow(18)],
        };
        assert_eq!(parsed_solution, expected);
    }

    #[test]
    fn test_objective_value_is_zero_for_trivial_solution() {
        // assert that the trivial solution has an objective value of 0
        // regardless of number of orders.

        let orders = {
            let mut orders = Vec::with_capacity(5);
            for i in 0..5 {
                orders.push(Order {
                    batch_information: None,
                    account_id: 1.into(),
                    buy_token: i,
                    sell_token: 5 - i,
                    buy_amount: 100,
                    sell_amount: 100,
                })
            }
            orders
        };

        assert_eq!(
            Solution::trivial(1).objective_value(&orders[..1]),
            Some(U256::zero())
        );
        assert_eq!(
            Solution::trivial(5).objective_value(&orders),
            Some(U256::zero())
        );
    }

    #[test]
    fn test_objective_value_for_non_trivial_solution() {}
}
