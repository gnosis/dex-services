use web3::types::U256;

use crate::models::{Order, State, TOKENS};

use super::price_finder_interface::{PriceFinding, Solution};
use crate::price_finding::error::PriceFindingError;

pub enum OrderPairType {
    LhsCompletelyFulfilled,
    RhsCompletelyFulfilled,
    BothFullyFilled,
}

impl Order {
    fn attracts(&self, other: &Order) -> bool {
        self.opposite_tokens(other) && self.have_price_overlap(other)
    }

    fn sufficient_seller_funds(&self, state: &State) -> bool {
        let balance_index = (self.sell_token - 1) as usize + (self.account_id - 1) as usize * TOKENS as usize;
        state.balances[balance_index] >= self.sell_amount
    }

    fn match_compare(&self, other: &Order, state: &State) -> Option<OrderPairType> {
        if self.sufficient_seller_funds(&state) && other.sufficient_seller_funds(&state) && self.attracts(other) {
            if self.buy_amount <= other.sell_amount && self.sell_amount <= other.buy_amount {
                return Some(OrderPairType::LhsCompletelyFulfilled);
            } else if self.buy_amount >= other.sell_amount && self.sell_amount >= other.buy_amount {
                return Some(OrderPairType::RhsCompletelyFulfilled);
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
        self.buy_amount * other.buy_amount <= other.sell_amount * self.sell_amount
    }
    fn surplus(
        &self,
        buy_price: u128,
        exec_buy_amount: u128,
        exec_sell_amount: u128,
    ) -> U256 {
        // TODO - Refer to Alex's Lemma [ceil(p/float(q)) == (p + q - 1) // q]
        let relative_buy = (self.buy_amount * exec_sell_amount + self.sell_amount - 1) / self.sell_amount;
        let res = (exec_buy_amount - relative_buy) * buy_price;
        U256::from_big_endian(&res.to_be_bytes())
    }
}

struct NaiveSolver {}

impl PriceFinding for NaiveSolver {
    fn find_prices(
        &mut self, 
        orders: &Vec<Order>, 
        state: &State
    ) -> Result<Solution, PriceFindingError> {
        // Initialize trivial solution
        let mut prices: Vec<u128> = vec![1; 1 + TOKENS as usize];
        let mut exec_buy_amount: Vec<u128> = vec![0; orders.len()];
        let mut exec_sell_amount: Vec<u128> = vec![0; orders.len()];
        let mut total_surplus = U256::zero();

        let mut found_flag = false;

        for (i, x) in orders.iter().enumerate() {
            for j in i + 1..orders.len() {
                let y = &orders[j];
                match x.match_compare(y, &state) {
                    Some(OrderPairType::LhsCompletelyFulfilled) => {
                        prices[x.buy_token as usize] = x.sell_amount;
                        prices[y.buy_token as usize] = x.buy_amount;
                        exec_sell_amount[i] = x.sell_amount;
                        exec_sell_amount[j] = x.buy_amount;

                        exec_buy_amount[i] = x.buy_amount;
                        exec_buy_amount[j] = x.sell_amount;
                    }
                    Some(OrderPairType::RhsCompletelyFulfilled) => {
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
                    None => continue
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
            if found_flag == true {
                break;
            }
        }
        Ok(Solution {
            surplus: total_surplus,
            prices,
            executed_sell_amounts: exec_sell_amount,
            executed_buy_amounts: exec_buy_amount,
        })
    }
}


#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_type_ia() {
        let state = State {
            state_hash: "test".to_string(),
            state_index: 0,
            balances: vec![200; (TOKENS * 2) as usize]
        };
        let orders = vec![
            Order {
                slot_index: 0,
                account_id: 2,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                slot_index: 1,
                account_id: 1,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
        ];
        let mut solver = NaiveSolver{};
        let res = solver.find_prices(&orders, &state);
        assert_eq!(U256::from(16), res.unwrap().surplus);
    }

    #[test]
    fn test_type_ib() {
        let state = State {
            state_hash: "test".to_string(),
            state_index: 0,
            balances: vec![200; (TOKENS * 2) as usize]
        };
        let orders = vec![
            Order {
                slot_index: 0,
                account_id: 1,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
            Order {
                slot_index: 1,
                account_id: 2,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            }
        ];
        let mut solver = NaiveSolver{};
        let res = solver.find_prices(&orders, &state);
        assert_eq!(U256::from(16), res.unwrap().surplus);
    }

    #[test]
    fn test_type_ii() {
        let state = State {
            state_hash: "test".to_string(),
            state_index: 0,
            balances: vec![200; (TOKENS * 2) as usize]
        };
        let orders = vec![
            Order {
                slot_index: 0,
                account_id: 1,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 10,
                buy_amount: 10,
            },
            Order {
                slot_index: 1,
                account_id: 2,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 16,
                buy_amount: 8,
            }
        ];
        let mut solver = NaiveSolver{};
        let res = solver.find_prices(&orders, &state);
        assert_eq!(U256::from(116), res.unwrap().surplus);
    }

    #[test]
    fn test_retreth_example() {
        let state = State {
            state_hash: "test".to_string(),
            state_index: 0,
            balances: vec![200; (TOKENS * 6) as usize]
        };
        let orders = vec![
            Order {
                slot_index: 0,
                account_id: 1,
                sell_token: 3,
                buy_token: 2,
                sell_amount: 12,
                buy_amount: 12,
            },
            Order {
                slot_index: 1,
                account_id: 2,
                sell_token: 2,
                buy_token: 3,
                sell_amount: 20,
                buy_amount: 22,
            },
            Order {
                slot_index: 2,
                account_id: 3,
                sell_token: 3,
                buy_token: 1,
                sell_amount: 10,
                buy_amount: 150,
            },
            Order {
                slot_index: 3,
                account_id: 4,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
            Order {
                slot_index: 4,
                account_id: 5,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                slot_index: 5,
                account_id: 6,
                sell_token: 1,
                buy_token: 3,
                sell_amount: 280,
                buy_amount: 20,
            }
        ];
        let mut solver = NaiveSolver{};
        let res = solver.find_prices(&orders, &state);
        assert_eq!(U256::from(16), res.unwrap().surplus);
    }

    #[test]
    fn test_insufficient_balance() {
        let state = State {
            state_hash: "test".to_string(),
            state_index: 0,
            balances: vec![0; (TOKENS * 2) as usize]
        };
        let orders = vec![
            Order {
                slot_index: 0,
                account_id: 2,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                slot_index: 1,
                account_id: 1,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
        ];
        let mut solver = NaiveSolver{};
        let res = solver.find_prices(&orders, &state);
        assert_eq!(U256::from(0), res.unwrap().surplus);
    }

    #[test]
    fn test_no_matches() {
        let state = State {
            state_hash: "test".to_string(),
            state_index: 0,
            balances: vec![200; (TOKENS * 2) as usize]
        };
        let orders = vec![
            Order {
                slot_index: 0,
                account_id: 2,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                slot_index: 1,
                account_id: 1,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 10,
                buy_amount: 180,
            },
        ];
        let mut solver = NaiveSolver{};
        let res = solver.find_prices(&orders, &state);
        assert_eq!(U256::from(0), res.unwrap().surplus);
    }
}
