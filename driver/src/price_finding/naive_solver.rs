use web3::types::U256;

use dfusion_core::models::{AccountState, Order, Solution, TOKENS};

use crate::price_finding::error::PriceFindingError;
use crate::util::{u128_to_u256, CeiledDiv};

use super::price_finder_interface::{Fee, PriceFinding};

pub enum OrderPairType {
    LhsFullyFilled,
    RhsFullyFilled,
    BothFullyFilled,
}

trait Matchable {
    fn attracts(&self, other: &Order, fee: &Option<Fee>) -> bool;
    fn sufficient_seller_funds(&self, state: &AccountState) -> bool;
    fn match_compare(
        &self,
        other: &Order,
        state: &AccountState,
        fee: &Option<Fee>,
    ) -> Option<OrderPairType>;
    fn opposite_tokens(&self, other: &Order) -> bool;
    fn have_price_overlap(&self, other: &Order) -> bool;
    fn surplus(&self, buy_price: u128, exec_buy_amount: u128, exec_sell_amount: u128) -> U256;
}

impl Matchable for Order {
    fn attracts(&self, other: &Order, fee: &Option<Fee>) -> bool {
        // We can only match orders that touch the fee token
        if fee.is_some()
            && ![self.sell_token, self.buy_token].contains(&fee.as_ref().unwrap().token)
        {
            return false;
        }
        self.opposite_tokens(other) && self.have_price_overlap(other)
    }

    fn sufficient_seller_funds(&self, state: &AccountState) -> bool {
        state.read_balance(self.sell_token, self.account_id) >= self.sell_amount
    }

    fn match_compare(
        &self,
        other: &Order,
        state: &AccountState,
        fee: &Option<Fee>,
    ) -> Option<OrderPairType> {
        if self.sufficient_seller_funds(&state)
            && other.sufficient_seller_funds(&state)
            && self.attracts(other, fee)
        {
            if self.buy_amount <= other.sell_amount && self.sell_amount <= other.buy_amount {
                return Some(OrderPairType::LhsFullyFilled);
            } else if self.buy_amount >= other.sell_amount && self.sell_amount >= other.buy_amount {
                return Some(OrderPairType::RhsFullyFilled);
            } else {
                return Some(OrderPairType::BothFullyFilled);
            }
        }
        None
    }
    fn opposite_tokens(&self, other: &Order) -> bool {
        self.buy_token == other.sell_token && self.sell_token == other.buy_token
    }

    fn have_price_overlap(&self, other: &Order) -> bool {
        self.sell_amount > 0
            && other.sell_amount > 0
            && u128_to_u256(self.buy_amount) * u128_to_u256(other.buy_amount)
                <= u128_to_u256(other.sell_amount) * u128_to_u256(self.sell_amount)
    }

    fn surplus(&self, buy_price: u128, exec_buy_amount: u128, exec_sell_amount: u128) -> U256 {
        let relative_buy = (u128_to_u256(self.buy_amount) * u128_to_u256(exec_sell_amount))
            .ceiled_div(u128_to_u256(self.sell_amount));
        (u128_to_u256(exec_buy_amount) - relative_buy) * u128_to_u256(buy_price)
    }
}

pub struct NaiveSolver {
    fee: Option<Fee>,
}

impl NaiveSolver {
    pub fn new(fee: Option<Fee>) -> Self {
        NaiveSolver { fee }
    }
}

impl PriceFinding for NaiveSolver {
    fn find_prices(
        &mut self,
        orders: &[Order],
        state: &AccountState,
    ) -> Result<Solution, PriceFindingError> {
        // Initialize trivial solution (default of zero indicates untouched token).
        let mut prices: Vec<u128> = vec![0; TOKENS as usize];
        let mut exec_buy_amount: Vec<u128> = vec![0; orders.len()];
        let mut exec_sell_amount: Vec<u128> = vec![0; orders.len()];
        let mut total_surplus = U256::zero();

        let mut found_flag = false;

        for (i, x) in orders.iter().enumerate() {
            for j in i + 1..orders.len() {
                // Preprocess order to leave "space" for fee to be taken
                let x = order_with_buffer_for_fee(&x, &self.fee);
                let y = order_with_buffer_for_fee(&orders[j], &self.fee);
                match x.match_compare(&y, &state, &self.fee) {
                    Some(OrderPairType::LhsFullyFilled) => {
                        prices[x.buy_token as usize] = x.sell_amount;
                        prices[y.buy_token as usize] = x.buy_amount;
                        exec_sell_amount[i] = x.sell_amount;
                        exec_sell_amount[j] = x.buy_amount;

                        exec_buy_amount[i] = x.buy_amount;
                        exec_buy_amount[j] = x.sell_amount;
                    }
                    Some(OrderPairType::RhsFullyFilled) => {
                        prices[x.sell_token as usize] = y.sell_amount;
                        prices[y.sell_token as usize] = y.buy_amount;

                        exec_sell_amount[i] = y.buy_amount;
                        exec_sell_amount[j] = y.sell_amount;

                        exec_buy_amount[i] = y.sell_amount;
                        exec_buy_amount[j] = y.buy_amount;
                    }
                    Some(OrderPairType::BothFullyFilled) => {
                        prices[y.buy_token as usize] = y.sell_amount;
                        prices[x.buy_token as usize] = x.sell_amount;

                        exec_sell_amount[i] = x.sell_amount;
                        exec_sell_amount[j] = y.sell_amount;

                        exec_buy_amount[i] = y.sell_amount;
                        exec_buy_amount[j] = x.sell_amount;
                    }
                    None => continue,
                }
                found_flag = true;
                let x_surplus = x.surplus(
                    prices[x.buy_token as usize],
                    exec_buy_amount[i],
                    exec_sell_amount[i],
                );
                let y_surplus = y.surplus(
                    prices[y.buy_token as usize],
                    exec_buy_amount[j],
                    exec_sell_amount[j],
                );
                total_surplus = x_surplus.checked_add(y_surplus).unwrap();
                break;
            }
            if found_flag {
                break;
            }
        }

        // Apply fee to volumes if necessary
        if let Some(fee) = &self.fee {
            for (i, order) in orders.iter().enumerate() {
                // To account for the fee we have to either
                // a) give people less (reduce buyAmount)
                // b) take more (increase sellAmount)
                let fee_denominator = (1.0 / fee.percentage) as u128;
                if order.sell_token == fee.token {
                    exec_sell_amount[i] =
                        exec_sell_amount[i] * fee_denominator / (fee_denominator - 1);
                }
                if order.buy_token == fee.token {
                    exec_buy_amount[i] =
                        (exec_buy_amount[i] * (fee_denominator - 1)).ceiled_div(fee_denominator);
                }
            }
        }

        let solution = Solution {
            surplus: Some(total_surplus),
            prices,
            executed_sell_amounts: exec_sell_amount,
            executed_buy_amounts: exec_buy_amount,
        };
        info!("Solution: {:?}", &solution);
        Ok(solution)
    }
}

fn order_with_buffer_for_fee(order: &Order, fee: &Option<Fee>) -> Order {
    match fee {
        Some(fee) => {
            let mut order = order.clone();
            // In order to make space for a fee in the existing limit, we need to
            // a) receive more stuff (while giving away the same)
            // b) give away less stuff (while receiving the same)
            let fee_denominator = (1.0 / fee.percentage) as u128;
            if fee.token == order.buy_token {
                order.buy_amount =
                    (order.buy_amount * fee_denominator).ceiled_div(fee_denominator - 1)
            } else if fee.token == order.sell_token {
                order.sell_amount = order.sell_amount * (fee_denominator - 1) / fee_denominator
            }
            order
        }
        None => order.clone(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::util::u256_to_u128;
    use dfusion_core::models::account_state::test_util::*;
    use std::collections::HashMap;
    use web3::types::{H160, H256};

    #[test]
    fn test_type_left_fully_matched_no_fee() {
        let orders = order_pair_first_fully_matching_second();
        let state = create_account_state_with_balance_for(&orders);

        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state).unwrap();

        assert_eq!(Some(u128_to_u256(16 * 10u128.pow(36))), res.surplus);
        assert_eq!(
            vec![4 * 10u128.pow(18), 52 * 10u128.pow(18)],
            res.executed_buy_amounts
        );
        assert_eq!(
            vec![52 * 10u128.pow(18), 4 * 10u128.pow(18)],
            res.executed_sell_amounts
        );
        assert_eq!(4 * 10u128.pow(18), res.prices[0]);
        assert_eq!(52 * 10u128.pow(18), res.prices[1]);

        check_solution(&orders, res, &None).unwrap();
    }

    #[test]
    fn test_type_left_fully_matched_with_fee() {
        let orders = order_pair_first_fully_matching_second();
        let state = create_account_state_with_balance_for(&orders);
        let fee = Some(Fee {
            token: 0,
            percentage: 0.001,
        });

        let mut solver = NaiveSolver::new(fee.clone());
        let res = solver.find_prices(&orders, &state).unwrap();
        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn test_type_right_fully_matched_no_fee() {
        let mut orders = order_pair_first_fully_matching_second();
        orders.reverse();
        let state = create_account_state_with_balance_for(&orders);

        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state).unwrap();

        assert_eq!(Some(u128_to_u256(16 * 10u128.pow(36))), res.surplus);
        assert_eq!(
            vec![52 * 10u128.pow(18), 4 * 10u128.pow(18)],
            res.executed_buy_amounts
        );
        assert_eq!(
            vec![4 * 10u128.pow(18), 52 * 10u128.pow(18)],
            res.executed_sell_amounts
        );
        assert_eq!(4 * 10u128.pow(18), res.prices[0]);
        assert_eq!(52 * 10u128.pow(18), res.prices[1]);

        check_solution(&orders, res, &None).unwrap();
    }

    #[test]
    fn test_type_right_fully_matched_with_fee() {
        let mut orders = order_pair_first_fully_matching_second();
        orders.reverse();
        let state = create_account_state_with_balance_for(&orders);
        let fee = Some(Fee {
            token: 2,
            percentage: 0.001,
        });

        let mut solver = NaiveSolver::new(fee.clone());
        let res = solver.find_prices(&orders, &state).unwrap();
        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn test_type_both_fully_matched_no_fee() {
        let orders = order_pair_both_fully_matched();
        let state = create_account_state_with_balance_for(&orders);

        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state).unwrap();

        assert_eq!(Some(u128_to_u256(92 * 10u128.pow(36))), res.surplus);
        assert_eq!(
            vec![16 * 10u128.pow(18), 10 * 10u128.pow(18)],
            res.executed_buy_amounts
        );
        assert_eq!(
            vec![10 * 10u128.pow(18), 16 * 10u128.pow(18)],
            res.executed_sell_amounts
        );
        assert_eq!(10 * 10u128.pow(18), res.prices[1]);
        assert_eq!(16 * 10u128.pow(18), res.prices[2]);

        check_solution(&orders, res, &None).unwrap();
    }

    #[test]
    fn test_type_both_fully_matched_with_fee() {
        let orders = order_pair_both_fully_matched();
        let state = create_account_state_with_balance_for(&orders);
        let fee = Some(Fee {
            token: 2,
            percentage: 0.001,
        });
        let mut solver = NaiveSolver::new(fee.clone());
        let res = solver.find_prices(&orders, &state).unwrap();

        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn test_retreth_example() {
        let orders = vec![
            Order {
                batch_information: None,
                account_id: H160::from(0),
                sell_token: 3,
                buy_token: 2,
                sell_amount: 12,
                buy_amount: 12,
            },
            Order {
                batch_information: None,
                account_id: H160::from(1),
                sell_token: 2,
                buy_token: 3,
                sell_amount: 20,
                buy_amount: 22,
            },
            Order {
                batch_information: None,
                account_id: H160::from(2),
                sell_token: 3,
                buy_token: 1,
                sell_amount: 10,
                buy_amount: 150,
            },
            Order {
                batch_information: None,
                account_id: H160::from(3),
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
            Order {
                batch_information: None,
                account_id: H160::from(4),
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                batch_information: None,
                account_id: H160::from(5),
                sell_token: 1,
                buy_token: 3,
                sell_amount: 280,
                buy_amount: 20,
            },
        ];
        let state = create_account_state_with_balance_for(&orders);

        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state).unwrap();

        assert_eq!(Some(U256::from(16)), res.surplus);
        assert_eq!(vec![0, 0, 0, 52, 4, 0], res.executed_buy_amounts);
        assert_eq!(vec![0, 0, 0, 4, 52, 0], res.executed_sell_amounts);
        assert_eq!(4, res.prices[1]);
        assert_eq!(52, res.prices[2]);

        check_solution(&orders, res, &None).unwrap();
    }

    #[test]
    fn test_insufficient_balance() {
        let state = AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![0; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                batch_information: None,
                account_id: H160::from(1),
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                batch_information: None,
                account_id: H160::from(0),
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
        ];

        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state).unwrap();
        assert_eq!(res, Solution::trivial(orders.len()));
    }

    #[test]
    fn test_no_matches() {
        let orders = vec![
            Order {
                batch_information: None,
                account_id: H160::from(1),
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                batch_information: None,
                account_id: H160::from(0),
                sell_token: 2,
                buy_token: 1,
                sell_amount: 10,
                buy_amount: 180,
            },
        ];
        let state = create_account_state_with_balance_for(&orders);

        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state).unwrap();
        assert_eq!(res, Solution::trivial(orders.len()));
    }

    #[test]
    fn test_stablex_contract_example() {
        let orders = vec![
            Order {
                batch_information: None,
                account_id: H160::from(0),
                sell_token: 0,
                buy_token: 1,
                sell_amount: 20000,
                buy_amount: 9990,
            },
            Order {
                batch_information: None,
                account_id: H160::from(0),
                sell_token: 1,
                buy_token: 0,
                sell_amount: 9990,
                buy_amount: 19960,
            },
        ];
        let state = create_account_state_with_balance_for(&orders);

        let fee = Some(Fee {
            token: 0,
            percentage: 0.001,
        });
        let mut solver = NaiveSolver::new(fee.clone());
        let res = solver.find_prices(&orders, &state).unwrap();

        assert_eq!(res.executed_sell_amounts, [20000, 9990]);
        assert_eq!(res.executed_buy_amounts, [9990, 19961]);
        assert_eq!(res.prices[0], 9990);
        assert_eq!(res.prices[1], 19980);

        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn test_does_not_trade_non_fee_tokens() {
        let orders = vec![
            Order {
                batch_information: None,
                account_id: H160::from(0),
                sell_token: 0,
                buy_token: 1,
                sell_amount: 20000,
                buy_amount: 9990,
            },
            Order {
                batch_information: None,
                account_id: H160::from(0),
                sell_token: 1,
                buy_token: 0,
                sell_amount: 9990,
                buy_amount: 19960,
            },
        ];
        let state = create_account_state_with_balance_for(&orders);

        let fee = Some(Fee {
            token: 2,
            percentage: 0.001,
        });
        let mut solver = NaiveSolver::new(fee.clone());
        let res = solver.find_prices(&orders, &state).unwrap();
        assert_eq!(res, Solution::trivial(orders.len()));
    }

    #[test]
    fn test_empty_sell_volume() {
        let orders = vec![
            Order {
                batch_information: None,
                account_id: H160::from(0),
                sell_token: 0,
                buy_token: 1,
                sell_amount: 0,
                buy_amount: 0,
            },
            Order {
                batch_information: None,
                account_id: H160::from(0),
                sell_token: 1,
                buy_token: 0,
                sell_amount: 0,
                buy_amount: 0,
            },
        ];
        let state = create_account_state_with_balance_for(&orders);

        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state).unwrap();
        assert_eq!(res, Solution::trivial(orders.len()));
    }

    fn order_pair_first_fully_matching_second() -> Vec<Order> {
        vec![
            Order {
                batch_information: None,
                account_id: H160::from(1),
                sell_token: 0,
                buy_token: 1,
                sell_amount: 52 * 10u128.pow(18),
                buy_amount: 4 * 10u128.pow(18),
            },
            Order {
                batch_information: None,
                account_id: H160::from(0),
                sell_token: 1,
                buy_token: 0,
                sell_amount: 15 * 10u128.pow(18),
                buy_amount: 180 * 10u128.pow(18),
            },
        ]
    }

    fn order_pair_both_fully_matched() -> Vec<Order> {
        vec![
            Order {
                batch_information: None,
                account_id: H160::from(1),
                sell_token: 2,
                buy_token: 1,
                sell_amount: 10 * 10u128.pow(18),
                buy_amount: 10 * 10u128.pow(18),
            },
            Order {
                batch_information: None,
                account_id: H160::from(1),
                sell_token: 1,
                buy_token: 2,
                sell_amount: 16 * 10u128.pow(18),
                buy_amount: 8 * 10u128.pow(18),
            },
        ]
    }

    fn check_solution(
        orders: &[Order],
        solution: Solution,
        fee: &Option<Fee>,
    ) -> Result<(), String> {
        let mut token_conservation = HashMap::new();
        for (i, order) in orders.iter().enumerate() {
            let buy_token_price = solution.prices[order.buy_token as usize];
            let sell_token_price = solution.prices[order.sell_token as usize];

            let exec_buy_amount = solution.executed_buy_amounts[i];
            let exec_sell_amount = if sell_token_price > 0 {
                if let Some(fee) = fee {
                    let fee_denominator = (1.0 / fee.percentage) as u128;
                    // We compute:
                    // sell_amount_wo_fee = buy_amount * buy_token_price / sell_token_price
                    // sell_amount_w_fee = sell_amount_wo_fee * fee_denominator / (fee_denominator - 1)
                    // Rearranged to avoid 256 bit overflow and still have minimal rounding error.
                    u256_to_u128(
                        (u128_to_u256(exec_buy_amount) * u128_to_u256(buy_token_price))
                            / u128_to_u256(fee_denominator - 1)
                            * u128_to_u256(fee_denominator)
                            / u128_to_u256(sell_token_price),
                    )
                } else {
                    (exec_buy_amount * buy_token_price) / sell_token_price
                }
            } else {
                0
            };

            if exec_sell_amount > order.sell_amount {
                return Err(format!(
                    "ExecutedSellAmount for order {} bigger than allowed ({} > {})",
                    i, exec_sell_amount, order.sell_amount
                ));
            }

            let limit_lhs = u128_to_u256(exec_sell_amount) * u128_to_u256(order.buy_amount);
            let limit_rhs = u128_to_u256(exec_buy_amount) * u128_to_u256(order.sell_amount);
            if limit_lhs > limit_rhs {
                return Err(format!(
                    "LimitPrice for order {} not satisifed ({} > {})",
                    i, limit_lhs, limit_rhs
                ));
            }

            *token_conservation.entry(order.buy_token).or_insert(0) += exec_buy_amount as i128;
            *token_conservation.entry(order.sell_token).or_insert(0) -= exec_sell_amount as i128;
        }

        for j in 0..solution.prices.len() {
            let balance = token_conservation.entry(j as u16).or_insert(0);
            if *balance != 0 && (fee.is_none() || j as u16 != fee.as_ref().unwrap().token) {
                return Err(format!(
                    "Token balance of token {} not 0 (was {})",
                    j, balance
                ));
            }
        }
        Ok(())
    }
}
