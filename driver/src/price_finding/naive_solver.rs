use dfusion_core::models::{AccountState, Order, Solution, TOKENS};

use crate::price_finding::error::PriceFindingError;
use crate::util::{checked_u256_to_u128, u128_to_u256, u256_to_u128, CeiledDiv};

use super::price_finder_interface::{Fee, PriceFinding};

const BASE_UNIT: u128 = 1_000_000_000_000_000_000u128;
const BASE_PRICE: u128 = BASE_UNIT;

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
    fn trades_fee_token(&self, fee: &Fee) -> bool;
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
        if !self.sufficient_seller_funds(&state)
            || !other.sufficient_seller_funds(&state)
            || !self.attracts(other, fee)
            || !fee
                .as_ref()
                .map(|fee| self.trades_fee_token(fee))
                .unwrap_or(true)
        {
            return None;
        }

        if self.buy_amount <= other.sell_amount && self.sell_amount <= other.buy_amount {
            Some(OrderPairType::LhsFullyFilled)
        } else if self.buy_amount >= other.sell_amount && self.sell_amount >= other.buy_amount {
            Some(OrderPairType::RhsFullyFilled)
        } else {
            Some(OrderPairType::BothFullyFilled)
        }
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

    fn trades_fee_token(&self, fee: &Fee) -> bool {
        self.buy_token == fee.token || self.sell_token == fee.token
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
        &self,
        orders: &[Order],
        state: &AccountState,
    ) -> Result<Solution, PriceFindingError> {
        // Initialize trivial solution (default of zero indicates untouched token).
        let mut prices: Vec<u128> = vec![0; TOKENS as usize];
        let mut exec_buy_amount: Vec<u128> = vec![0; orders.len()];
        let mut exec_sell_amount: Vec<u128> = vec![0; orders.len()];

        'outer: for (i, x) in orders.iter().enumerate() {
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
                break 'outer;
            }
        }

        if let Some(fee) = &self.fee {
            // normalize prices so fee token price is BASE_PRICE
            let pre_normalized_fee_price = prices[fee.token as usize];
            if pre_normalized_fee_price == 0 {
                return Ok(Solution::trivial(orders.len()));
            }
            for price in prices.iter_mut() {
                *price = match normalize_price(*price, pre_normalized_fee_price) {
                    Some(price) => price,
                    None => return Ok(Solution::trivial(orders.len())),
                };
            }

            // apply fee to volumes account for rounding errors, moving them to
            // the fee token
            for (i, order) in orders.iter().enumerate() {
                if order.sell_token == fee.token {
                    let price_buy = prices[order.buy_token as usize];
                    exec_sell_amount[i] =
                        executed_sell_amount(fee, exec_buy_amount[i], price_buy, BASE_PRICE);
                } else {
                    let price_sell = prices[order.sell_token as usize];
                    exec_buy_amount[i] =
                        match executed_buy_amount(fee, exec_sell_amount[i], BASE_PRICE, price_sell)
                        {
                            Some(exec_buy_amt) => exec_buy_amt,
                            None => return Ok(Solution::trivial(orders.len())),
                        };
                }
            }
        }

        let solution = Solution {
            prices,
            executed_sell_amounts: exec_sell_amount,
            executed_buy_amounts: exec_buy_amount,
        };
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
            let fee_denominator = (1.0 / fee.ratio) as u128;
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

/// Normalizes a price base on the pre-normalized fee price.
fn normalize_price(price: u128, pre_normalized_fee_price: u128) -> Option<u128> {
    // upcast to u256 to avoid overflows
    checked_u256_to_u128(
        (u128_to_u256(price) * u128_to_u256(BASE_PRICE))
            .ceiled_div(u128_to_u256(pre_normalized_fee_price)),
    )
}

/// Calculate the executed sell amount from the fee, executed buy amount, and
/// the buy and sell prices of the traded tokens.
fn executed_sell_amount(fee: &Fee, exec_buy_amt: u128, buy_price: u128, sell_price: u128) -> u128 {
    let fee_denominator = (1.0 / fee.ratio) as u128;
    u256_to_u128(
        (((u128_to_u256(exec_buy_amt) * u128_to_u256(buy_price))
            / u128_to_u256(fee_denominator - 1))
            * u128_to_u256(fee_denominator))
            / u128_to_u256(sell_price),
    )
}

/// Calculate the executed buy amount from the fee, executed sell amount, and
/// the buy and sell prices of the traded tokens. This function returns a `None`
/// if no value can be found such that `executed_sell_amount(result, pb, ps) ==
/// xs`. This function acts as an inverse to `executed_sell_amount`.
fn executed_buy_amount(
    fee: &Fee,
    exec_sell_amt: u128,
    buy_price: u128,
    sell_price: u128,
) -> Option<u128> {
    let fee_denominator = (1.0 / fee.ratio) as u128;
    let exec_buy_amt = u256_to_u128(
        (((u128_to_u256(exec_sell_amt) * u128_to_u256(sell_price))
            / u128_to_u256(fee_denominator))
            * u128_to_u256(fee_denominator - 1))
            / u128_to_u256(buy_price),
    );

    // we need to account for rounding errors here, since this function is
    // essentially an inverse of `executed_sell_amount`; so find the maximum
    // error that `exec_buy_amt` can have and find a value that works

    macro_rules! return_if_sell_amount_matches {
        ($proposed_exec_buy_amt:expr) => {
            if exec_sell_amt
                == executed_sell_amount(fee, $proposed_exec_buy_amt, buy_price, sell_price)
            {
                return Some($proposed_exec_buy_amt);
            }
        };
    }

    return_if_sell_amount_matches!(exec_buy_amt);

    let maximum_error = (sell_price / buy_price) + 1;
    for error in 1..=maximum_error {
        return_if_sell_amount_matches!(exec_buy_amt + error);
        return_if_sell_amount_matches!(exec_buy_amt - error);
    }

    None
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::util::u256_to_u128;
    use dfusion_core::models::account_state::test_util::*;
    use std::collections::HashMap;
    use web3::types::{Address, H160, H256, U256};

    #[test]
    fn test_type_left_fully_matched_no_fee() {
        let orders = order_pair_first_fully_matching_second();
        let state = create_account_state_with_balance_for(&orders);

        let solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state).unwrap();

        assert_eq!(
            vec![4 * BASE_UNIT, 52 * BASE_UNIT],
            res.executed_buy_amounts
        );
        assert_eq!(
            vec![52 * BASE_UNIT, 4 * BASE_UNIT],
            res.executed_sell_amounts
        );
        assert_eq!(4 * BASE_UNIT, res.prices[0]);
        assert_eq!(52 * BASE_UNIT, res.prices[1]);

        check_solution(&orders, res, &None).unwrap();
    }

    #[test]
    fn test_type_left_fully_matched_with_fee() {
        let orders = order_pair_first_fully_matching_second();
        let state = create_account_state_with_balance_for(&orders);
        let fee = Some(Fee {
            token: 0,
            ratio: 0.001,
        });

        let solver = NaiveSolver::new(fee.clone());
        let res = solver.find_prices(&orders, &state).unwrap();
        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn test_type_right_fully_matched_no_fee() {
        let mut orders = order_pair_first_fully_matching_second();
        orders.reverse();
        let state = create_account_state_with_balance_for(&orders);

        let solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state).unwrap();

        assert_eq!(
            vec![52 * BASE_UNIT, 4 * BASE_UNIT],
            res.executed_buy_amounts
        );
        assert_eq!(
            vec![4 * BASE_UNIT, 52 * BASE_UNIT],
            res.executed_sell_amounts
        );
        assert_eq!(4 * BASE_UNIT, res.prices[0]);
        assert_eq!(52 * BASE_UNIT, res.prices[1]);

        check_solution(&orders, res, &None).unwrap();
    }

    #[test]
    fn test_type_right_fully_matched_with_fee() {
        let mut orders = order_pair_first_fully_matching_second();
        orders.reverse();
        let state = create_account_state_with_balance_for(&orders);
        let fee = Some(Fee {
            token: 2,
            ratio: 0.001,
        });

        let solver = NaiveSolver::new(fee.clone());
        let res = solver.find_prices(&orders, &state).unwrap();
        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn test_type_both_fully_matched_no_fee() {
        let orders = order_pair_both_fully_matched();
        let state = create_account_state_with_balance_for(&orders);

        let solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state).unwrap();

        assert_eq!(
            vec![16 * BASE_UNIT, 10 * BASE_UNIT],
            res.executed_buy_amounts
        );
        assert_eq!(
            vec![10 * BASE_UNIT, 16 * BASE_UNIT],
            res.executed_sell_amounts
        );
        assert_eq!(10 * BASE_UNIT, res.prices[1]);
        assert_eq!(16 * BASE_UNIT, res.prices[2]);

        check_solution(&orders, res, &None).unwrap();
    }

    #[test]
    fn test_type_both_fully_matched_with_fee() {
        let orders = order_pair_both_fully_matched();
        let state = create_account_state_with_balance_for(&orders);
        let fee = Some(Fee {
            token: 2,
            ratio: 0.001,
        });
        let solver = NaiveSolver::new(fee.clone());
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

        let solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state).unwrap();

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

        let solver = NaiveSolver::new(None);
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

        let solver = NaiveSolver::new(None);
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
            ratio: 0.001,
        });
        let solver = NaiveSolver::new(fee.clone());
        let res = solver.find_prices(&orders, &state).unwrap();

        assert_eq!(res.executed_sell_amounts, [20000, 9990]);
        assert_eq!(res.executed_buy_amounts, [9990, 19961]);
        assert_eq!(res.prices[0], BASE_PRICE);
        assert_eq!(res.prices[1], BASE_PRICE * 2);

        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn stablex_e2e_auction() {
        let users = [Address::from(0), Address::from(1)];
        let state = {
            let mut state = AccountState::default();
            state.num_tokens = u16::max_value();
            state.increment_balance(0, users[0], 3000 * BASE_UNIT);
            state.increment_balance(1, users[1], 3000 * BASE_UNIT);
            state
        };
        let orders = vec![
            Order {
                batch_information: None,
                account_id: users[0],
                sell_token: 0,
                buy_token: 1,
                sell_amount: 2000 * BASE_UNIT,
                buy_amount: 999 * BASE_UNIT,
            },
            Order {
                batch_information: None,
                account_id: users[1],
                sell_token: 1,
                buy_token: 0,
                sell_amount: 999 * BASE_UNIT,
                buy_amount: 1996 * BASE_UNIT,
            },
        ];

        let fee = Some(Fee {
            token: 0,
            ratio: 0.001,
        });
        let solver = NaiveSolver::new(fee.clone());
        let res = solver.find_prices(&orders, &state).unwrap();

        assert!(res.is_non_trivial());
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
            ratio: 0.001,
        });
        let solver = NaiveSolver::new(fee.clone());
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

        let solver = NaiveSolver::new(None);
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
                sell_amount: 52 * BASE_UNIT,
                buy_amount: 4 * BASE_UNIT,
            },
            Order {
                batch_information: None,
                account_id: H160::from(0),
                sell_token: 1,
                buy_token: 0,
                sell_amount: 15 * BASE_UNIT,
                buy_amount: 180 * BASE_UNIT,
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
                sell_amount: 10 * BASE_UNIT,
                buy_amount: 10 * BASE_UNIT,
            },
            Order {
                batch_information: None,
                account_id: H160::from(1),
                sell_token: 1,
                buy_token: 2,
                sell_amount: 16 * BASE_UNIT,
                buy_amount: 8 * BASE_UNIT,
            },
        ]
    }

    fn check_solution(
        orders: &[Order],
        solution: Solution,
        fee: &Option<Fee>,
    ) -> Result<(), String> {
        if !solution.is_non_trivial() {
            // trivial solutions are always OK
            return Ok(());
        }

        if let Some(fee_token) = fee.as_ref().map(|fee| fee.token as usize) {
            if solution.prices[fee_token] != BASE_PRICE {
                return Err(format!(
                    "price of fee token does not match the base price: {} != {}",
                    solution.prices[fee_token], BASE_PRICE
                ));
            }
        }

        let mut token_conservation = HashMap::new();
        for (i, order) in orders.iter().enumerate() {
            let buy_token_price = solution.prices[order.buy_token as usize];
            let sell_token_price = solution.prices[order.sell_token as usize];

            let exec_buy_amount = solution.executed_buy_amounts[i];
            let exec_sell_amount = if sell_token_price > 0 {
                if let Some(fee) = fee {
                    let fee_denominator = (1.0 / fee.ratio) as u128;
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
