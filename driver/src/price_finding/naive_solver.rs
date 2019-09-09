use web3::types::U256;

use dfusion_core::models::{AccountState, Order, Solution, TOKENS};

use crate::price_finding::error::PriceFindingError;

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
        u128_to_u256(self.buy_amount) * u128_to_u256(other.buy_amount)
            <= u128_to_u256(other.sell_amount) * u128_to_u256(self.sell_amount)
    }

    fn surplus(&self, buy_price: u128, exec_buy_amount: u128, exec_sell_amount: u128) -> U256 {
        // Note that: ceil(p / float(q)) == (p + q - 1) // q
        let relative_buy = (u128_to_u256(self.buy_amount) * u128_to_u256(exec_sell_amount)
            + u128_to_u256(self.sell_amount)
            - 1)
            / u128_to_u256(self.sell_amount);
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
        // Initialize trivial solution
        let mut prices: Vec<u128> = vec![1; TOKENS as usize];
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
                        prices[x.buy_token as usize] = y.sell_amount;
                        prices[y.buy_token as usize] = x.sell_amount;

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
                if order.sell_token == fee.token {
                    exec_sell_amount[i] =
                        (exec_sell_amount[i] as f64 / (1.0 - fee.percentage)).round() as u128;
                }
                if order.buy_token == fee.token {
                    exec_buy_amount[i] =
                        (exec_buy_amount[i] as f64 * (1.0 - fee.percentage)).round() as u128;
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

fn u128_to_u256(x: u128) -> U256 {
    U256::from_big_endian(&x.to_be_bytes())
}

fn order_with_buffer_for_fee(order: &Order, fee: &Option<Fee>) -> Order {
    match fee {
        Some(fee) => {
            let mut order = order.clone();
            // In order to make space for a fee in the existing limit, we need to
            // a) receive more stuff (while giving away the same)
            // b) give away less stuff (while receiving the same)
            if fee.token == order.buy_token {
                order.buy_amount =
                    (order.buy_amount as f64 / (1.0 - fee.percentage)).round() as u128
            } else if fee.token == order.sell_token {
                order.sell_amount =
                    (order.sell_amount as f64 * (1.0 - fee.percentage).round()) as u128
            }
            order
        }
        None => order.clone(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use web3::types::H256;

    #[test]
    fn test_type_ia() {
        let state = AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![200; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                batch_information: None,
                account_id: 1,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                batch_information: None,
                account_id: 0,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
        ];
        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state);
        assert_eq!(Some(U256::from(16)), res.unwrap().surplus);
    }

    #[test]
    fn test_type_ib() {
        let state = AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![200; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                batch_information: None,
                account_id: 0,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
            Order {
                batch_information: None,
                account_id: 1,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
        ];
        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state);
        assert_eq!(Some(U256::from(16)), res.unwrap().surplus);
    }

    #[test]
    fn test_type_ii() {
        let state = AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![200; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                batch_information: None,
                account_id: 0,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 10,
                buy_amount: 10,
            },
            Order {
                batch_information: None,
                account_id: 1,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 16,
                buy_amount: 8,
            },
        ];
        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state);
        assert_eq!(Some(U256::from(116)), res.unwrap().surplus);
    }

    #[test]
    fn test_retreth_example() {
        let state = AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![200; (TOKENS * 6) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                batch_information: None,
                account_id: 0,
                sell_token: 3,
                buy_token: 2,
                sell_amount: 12,
                buy_amount: 12,
            },
            Order {
                batch_information: None,
                account_id: 1,
                sell_token: 2,
                buy_token: 3,
                sell_amount: 20,
                buy_amount: 22,
            },
            Order {
                batch_information: None,
                account_id: 2,
                sell_token: 3,
                buy_token: 1,
                sell_amount: 10,
                buy_amount: 150,
            },
            Order {
                batch_information: None,
                account_id: 3,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
            Order {
                batch_information: None,
                account_id: 4,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                batch_information: None,
                account_id: 5,
                sell_token: 1,
                buy_token: 3,
                sell_amount: 280,
                buy_amount: 20,
            },
        ];
        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state);
        assert_eq!(Some(U256::from(16)), res.unwrap().surplus);
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
                account_id: 1,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                batch_information: None,
                account_id: 0,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
        ];
        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state);
        assert_eq!(Some(U256::from(0)), res.unwrap().surplus);
    }

    #[test]
    fn test_no_matches() {
        let state = AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![200; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                batch_information: None,
                account_id: 1,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                batch_information: None,
                account_id: 0,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 10,
                buy_amount: 180,
            },
        ];
        let mut solver = NaiveSolver::new(None);
        let res = solver.find_prices(&orders, &state);
        assert_eq!(Some(U256::from(0)), res.unwrap().surplus);
    }

    #[test]
    fn test_solution_with_fee() {
        let state = AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![100_000; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                batch_information: None,
                account_id: 0,
                sell_token: 0,
                buy_token: 1,
                sell_amount: 20000,
                buy_amount: 9990,
            },
            Order {
                batch_information: None,
                account_id: 0,
                sell_token: 1,
                buy_token: 0,
                sell_amount: 9990,
                buy_amount: 19960,
            },
        ];

        let mut solver = NaiveSolver::new(Some(Fee {
            token: 0,
            percentage: 0.001,
        }));
        let res = solver.find_prices(&orders, &state).unwrap();
        assert_eq!(res.executed_sell_amounts, [20000, 9990]);
        assert_eq!(res.executed_buy_amounts, [9990, 19960])
    }

    #[test]
    fn test_does_not_trade_non_fee_tokens() {
        let state = AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![100_000; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                batch_information: None,
                account_id: 0,
                sell_token: 0,
                buy_token: 1,
                sell_amount: 20000,
                buy_amount: 9990,
            },
            Order {
                batch_information: None,
                account_id: 0,
                sell_token: 1,
                buy_token: 0,
                sell_amount: 9990,
                buy_amount: 19960,
            },
        ];

        let mut solver = NaiveSolver::new(Some(Fee {
            token: 2,
            percentage: 0.001,
        }));
        let res = solver.find_prices(&orders, &state).unwrap();
        assert_eq!(res.executed_sell_amounts, [0, 0]);
    }
}
