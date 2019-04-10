use web3::types::U256;

use crate::models::{Order, TOKENS};

use super::price_finder_interface::Solution;

pub enum OrderPairType {
    TypeIa,
    TypeIb,
    TypeII,
}

impl Order {
    fn attracts(&self, other: &Order) -> bool {
        self.opposite_tokens(other) && self.have_price_overlap(other)
    }
    fn match_compare(&self, other: &Order) -> Option<OrderPairType> {
        if self.attracts(other) {
            if self.buy_amount <= other.sell_amount && self.sell_amount <= other.buy_amount {
                Some(OrderPairType::TypeIa);
            } else if self.buy_amount >= other.sell_amount && self.sell_amount >= other.buy_amount {
                Some(OrderPairType::TypeIb);
            } else {
                Some(OrderPairType::TypeII);
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
        price: u128,
        exec_buy_amount: u128,
        exec_sell_amount: u128,
    ) -> U256 {
        // TODO - Refer to Alex's Lemma
        let res = (exec_buy_amount - (self.buy_amount * exec_sell_amount + self.sell_amount - 1) / self.sell_amount) * price;
        U256::from_big_endian(&res.to_be_bytes())
    }
}

pub fn solve(orders: &Vec<Order>) -> Solution {
//    TODO - include account balances and make sure they agree.
    let mut prices: Vec<u128> = vec![0; TOKENS as usize];
    let mut exec_buy_amount: Vec<u128> = vec![0; orders.len()];
    let mut exec_sell_amount: Vec<u128> = vec![0; orders.len()];
    let mut total_surplus = U256::zero();

    let mut found_flag = false;

    for (i, x) in orders.iter().enumerate() {
        for j in i + 1..orders.len() {
            let y = &orders[j];

            match x.match_compare(y) {
                Some(OrderPairType::TypeIa) => {
                    prices[(x.buy_token - 1) as usize] = x.sell_amount;
                    prices[(y.buy_token - 1) as usize] = x.buy_amount;

                    exec_sell_amount[i] = x.sell_amount;
                    exec_sell_amount[j] = x.buy_amount;

                    exec_buy_amount[i] = x.buy_amount;
                    exec_buy_amount[j] = x.sell_amount;
                }
                Some(OrderPairType::TypeIb) => {
                    prices[(x.sell_token - 1) as usize] = y.sell_amount;
                    prices[(y.sell_token - 1) as usize] = y.buy_amount;

                    exec_sell_amount[i] = y.buy_amount;
                    exec_sell_amount[j] = y.sell_amount;

                    exec_buy_amount[i] = y.sell_amount;
                    exec_buy_amount[j] = y.buy_amount;
                }
                Some(OrderPairType::TypeII) => {
                    prices[(x.buy_token - 1) as usize] = y.sell_amount;
                    prices[(y.buy_token - 1) as usize] = x.sell_amount;

                    exec_sell_amount[i] = x.sell_amount;
                    exec_sell_amount[j] = y.sell_amount;

                    exec_buy_amount[i] = y.sell_amount;
                    exec_buy_amount[j] = x.sell_amount;
                }
                None => continue
            }
            found_flag = true;
            let x_surplus = x.surplus(prices[x.buy_token as usize], exec_buy_amount[i], exec_sell_amount[i]);
            let y_surplus = y.surplus(prices[y.buy_token as usize], exec_buy_amount[j], exec_sell_amount[j]);
            total_surplus = x_surplus.checked_add(y_surplus).unwrap();
            break;
        }
        if found_flag == true {
            break;
        }
    }
    Solution {
        surplus: total_surplus,
        prices,
        executed_sell_amounts: exec_sell_amount,
        executed_buy_amounts: exec_buy_amount,
    }
}