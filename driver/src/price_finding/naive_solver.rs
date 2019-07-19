use web3::types::U256;

use dfusion_core::models::{Order, State, TOKENS};

use crate::price_finding::error::PriceFindingError;

use super::price_finder_interface::{PriceFinding, Solution};

pub enum OrderPairType {
    LhsFullyFilled,
    RhsFullyFilled,
    BothFullyFilled,
}

fn u128_to_u256(x: u128) -> U256 {
    U256::from_big_endian(&x.to_be_bytes())
}
trait Matchable {
    fn attracts(&self, other: &Order) -> bool;
    fn sufficient_seller_funds(&self, state: &State) -> bool;
    fn match_compare(&self, other: &Order, state: &State) -> Option<OrderPairType>;
    fn opposite_tokens(&self, other: &Order) -> bool;
    fn have_price_overlap(&self, other: &Order) -> bool;
    fn surplus(&self, buy_price: u128, exec_buy_amount: u128, exec_sell_amount: u128) -> U256;

}

impl Matchable for Order {
    fn attracts(&self, other: &Order) -> bool {
        self.opposite_tokens(other) && self.have_price_overlap(other)
    }

    fn sufficient_seller_funds(&self, state: &State) -> bool {
        state.read_balance(self.sell_token, self.account_id) >= self.sell_amount
    }

    fn match_compare(&self, other: &Order, state: &State) -> Option<OrderPairType> {
        if self.sufficient_seller_funds(&state) && other.sufficient_seller_funds(&state) && self.attracts(other) {
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
        u128_to_u256(self.buy_amount) * u128_to_u256(other.buy_amount) <= u128_to_u256(other.sell_amount) * u128_to_u256(self.sell_amount)
    }
    fn surplus(
        &self,
        buy_price: u128,
        exec_buy_amount: u128,
        exec_sell_amount: u128,
    ) -> U256 {
        // Note that: ceil(p / float(q)) == (p + q - 1) // q
        let relative_buy = (u128_to_u256(self.buy_amount) * u128_to_u256(exec_sell_amount) + u128_to_u256(self.sell_amount) - 1) / u128_to_u256(self.sell_amount);
        (u128_to_u256(exec_buy_amount) - relative_buy) * u128_to_u256(buy_price)
    }
}

pub struct NaiveSolver {}

impl PriceFinding for NaiveSolver {
    fn find_prices(
        &mut self, 
        orders: &[Order],
        state: &State
    ) -> Result<Solution, PriceFindingError> {
        // Initialize trivial solution
        let mut prices: Vec<u128> = vec![1; TOKENS as usize];
        let mut exec_buy_amount: Vec<u128> = vec![0; orders.len()];
        let mut exec_sell_amount: Vec<u128> = vec![0; orders.len()];
        let mut total_surplus = U256::zero();

        let mut found_flag = false;

        for (i, x) in orders.iter().enumerate() {
            for j in i + 1..orders.len() {
                let y = &orders[j];
                match x.match_compare(y, &state) {
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
            if found_flag {
                break;
            }
        }
        let solution = Solution {
            surplus: total_surplus,
            prices,
            executed_sell_amounts: exec_sell_amount,
            executed_buy_amounts: exec_buy_amount,
        };
        info!("Solution: {:?}", &solution);
        Ok(solution)
    }
}


#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_type_ia() {
        let state = State::new(
            "test".to_string(),
            0,
            vec![200; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                account_id: 1,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                account_id: 0,
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
        let state = State::new(
            "test".to_string(),
            0,
            vec![200; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                account_id: 0,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
            Order {
                account_id: 1,
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
        let state = State::new(
            "test".to_string(),
            0,
            vec![200; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                account_id: 0,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 10,
                buy_amount: 10,
            },
            Order {
                account_id: 1,
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
        let state = State::new(
            "test".to_string(),
            0,
            vec![200; (TOKENS * 6) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                account_id: 0,
                sell_token: 3,
                buy_token: 2,
                sell_amount: 12,
                buy_amount: 12,
            },
            Order {
                account_id: 1,
                sell_token: 2,
                buy_token: 3,
                sell_amount: 20,
                buy_amount: 22,
            },
            Order {
                account_id: 2,
                sell_token: 3,
                buy_token: 1,
                sell_amount: 10,
                buy_amount: 150,
            },
            Order {
                account_id: 3,
                sell_token: 2,
                buy_token: 1,
                sell_amount: 15,
                buy_amount: 180,
            },
            Order {
                account_id: 4,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                account_id: 5,
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
        let state = State::new(
            "test".to_string(),
            0,
            vec![0; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                account_id: 1,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                account_id: 0,
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
        let state = State::new(
            "test".to_string(),
            0,
            vec![200; (TOKENS * 2) as usize],
            TOKENS,
        );
        let orders = vec![
            Order {
                account_id: 1,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 52,
                buy_amount: 4,
            },
            Order {
                account_id: 0,
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
