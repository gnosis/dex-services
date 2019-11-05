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
                fee_sell_amount = fee_sell_amount.checked_add(u128_to_u256(exec_sell_amount))?;
            }
        }

        let fee_token_conservation = fee_sell_amount.checked_sub(fee_buy_amount)?;

        println!("{} -\n {}", total_utility, total_disregarded_utility);
        total_utility
            .checked_sub(total_disregarded_utility)?
            .checked_add(fee_token_conservation / 2)
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

        let orders: Vec<_> = (0..5)
            .map(|i| Order {
                batch_information: None,
                account_id: 1.into(),
                buy_token: i,
                sell_token: 5 - i,
                buy_amount: 100,
                sell_amount: 100,
            })
            .collect();

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
    fn test_objective_value_for_cd_haircut_without_fees() {
        // this is using our used CD for a haircut example with $ fee token

        let orders = vec![
            Order {
                // wants to buy 1 haircut for at most 11$
                buy_token: 2,
                sell_token: 0,
                buy_amount: 1,
                sell_amount: 11,
                ..Order::default()
            },
            Order {
                // wants to sell used Tool CD for at least 1$
                buy_token: 0,
                sell_token: 1,
                buy_amount: 1,
                sell_amount: 1,
                ..Order::default()
            },
            Order {
                // wants to trade a hair cut to a used Tool CD
                buy_token: 1,
                sell_token: 2,
                buy_amount: 1,
                sell_amount: 1,
                ..Order::default()
            },
        ];

        let solution = Solution {
            prices: vec![
                1, // $
                6, // CD
                6, // haircut
            ],
            executed_buy_amounts: vec![1, 6, 1],
            executed_sell_amounts: vec![6, 1, 1],
        };
        assert_eq!(solution.objective_value(&orders), Some(6.into()));

        let solution = Solution {
            prices: vec![
                1,  // $
                11, // CD
                11, // haircut
            ],
            executed_buy_amounts: vec![1, 11, 1],
            executed_sell_amounts: vec![11, 1, 1],
        };
        assert_eq!(solution.objective_value(&orders), Some(10.into()));
    }

    #[test]
    #[allow(clippy::identity_op)]
    fn test_objective_value_for_simple_case_with_fees() {
        // trading 2 tokens

        let eth: u128 = 10u128.pow(18);
        let orders = vec![
            Order {
                buy_token: 0,
                sell_token: 1,
                buy_amount: 2 * eth,
                sell_amount: 1 * eth,
                ..Order::default()
            },
            Order {
                buy_token: 1,
                sell_token: 0,
                buy_amount: 1 * eth,
                sell_amount: 3 * eth,
                ..Order::default()
            },
        ];

        let solution = Solution {
            prices: vec![1 * eth, 2_500_000_000_000_000_000],
            executed_buy_amounts: vec![2_497_500_000_000_000_000, 1 * eth],
            executed_sell_amounts: vec![1 * eth, 2_502_502_502_502_502_502],
        };

        //  U0 = ((xb * os - xs * ob) * pb) / os
        //     = ((2.497eth * 1eth - 1eth * 2eth) * 1eth) / 1eth
        //     = 497500000000000000000000000000000000
        // dU0 = ((ps * os - ob * pb) * (os - xs)) / os
        //     = ((2.5eth * 1eth - 2eth * 1eth) * (1eth - 1eth)) / 1eth
        //     = 0
        //  U1 = ((xb * os - xs * ob) * pb) / os
        //     = ((1eth * 3eth - 2.502eth * 1eth) * 2.5eth) / 3eth
        //     = 414581247914581248333333333333333333
        //     = 414581247914581248333333333333333334 -- with rounding error
        // dU1 = ((ps * os - ob * pb) * (os - xs)) / os
        //     = ((1eth * 3eth - 1eth * 2.5eth) * (3eth - 2.502eth)) / 3eth
        //     =  82916249582916249666666666666666666
        // fee = xs1 - xb0
        //     = 2.502eth - 2.497eth
        //     =                     5002502502502502
        // O   = U0 + U1 - dU0 - dU1 + (fee / 2)
        //     = 829164998331664998669167917917917919

        assert_eq!(
            solution.objective_value(&orders),
            Some(U256::from_dec_str("829164998331664998669167917917917919").unwrap())
        );

        /* 497500000000000000000000000000000000-0+0-2497500000000000000 */
    }

    #[test]
    #[allow(clippy::identity_op)]
    fn test_objective_value_for_large_market_maker_with_buyer_case_with_fees() {
        // trading WETH for DAI with DAI as the price token

        let eth: u128 = 10u128.pow(18);
        let orders = vec![
            Order {
                // wants to buy 1 WETH for 185 DAI
                buy_token: 1,
                sell_token: 0,
                buy_amount: 1 * eth,
                sell_amount: 185 * eth,
                ..Order::default()
            },
            Order {
                // large WETH seller selling 1 WETH for 184 DAI
                buy_token: 0,
                sell_token: 1,
                buy_amount: 184_000 * eth,
                sell_amount: 1_000 * eth,
                ..Order::default()
            },
        ];

        let solution = Solution {
            prices: vec![
                1 * eth,
                184_184_184_184_184_185_000, // market maker's price adjusted for fees and fixed for rounding errors
            ],
            executed_buy_amounts: vec![1 * eth, 184_000_000_000_000_000_815],
            executed_sell_amounts: vec![184_368_552_736_921_106_106, 1 * eth],
        };

        // TODO(nlordell): this solution is currently invalid as the disregarded
        //   utility of the large market maker order is very large, and the price
        //   cannot be any closer to the limit price without violating it; there
        //   is a discussion started at:
        //   https://github.com/gnosis/dex-contracts/issues/276
        assert_eq!(solution.objective_value(&orders), None);
    }
}
